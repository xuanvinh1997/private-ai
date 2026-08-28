from __future__ import annotations

import atexit
import re
from collections.abc import Callable
from contextlib import suppress
from importlib import import_module
from pathlib import Path
from typing import Any

PAGE_MARKER = re.compile(r"^<!--\s*private-ai-page:(\d+)\s*-->$")
PARSER_NAME = "private-ai-passthrough"
ProgressCallback = Callable[[dict[str, object]], None]


class RagAnythingUnavailable(RuntimeError):
    """Raised when the optional RAG-Anything runtime is not installed."""


def _content_list(text: str) -> list[dict[str, object]]:
    """Turn the app's page-aware Markdown into RAG-Anything content blocks."""
    blocks: list[dict[str, object]] = []
    page_index = 0
    current: list[str] = []

    def flush() -> None:
        content = "\n".join(current).strip()
        if content:
            blocks.append({"type": "text", "text": content, "page_idx": page_index})
        current.clear()

    for line in text.splitlines():
        marker = PAGE_MARKER.fullmatch(line.strip())
        if marker:
            flush()
            page_index = max(0, int(marker.group(1)) - 1)
        else:
            current.append(line)
    flush()
    return blocks or [{"type": "text", "text": text, "page_idx": 0}]


class RagAnythingOrchestrator:
    """Route pre-extracted document blocks through RAG-Anything into LightRAG.

    Private AI already owns extraction/OCR and needs the resulting Markdown for its
    document viewer. RAG-Anything therefore starts at its documented content-list API:
    it owns content-type routing and insertion while the existing LightRAG instance keeps
    workspace isolation, retrieval, deletion and graph inspection.
    """

    def __init__(self, working_dir: Path) -> None:
        self.working_dir = working_dir
        self._instances: dict[str, Any] = {}

    @staticmethod
    def _bindings() -> tuple[Any, Any, Any, Any]:
        try:
            package = import_module("raganything")
            parser_module = import_module("raganything.parser")
            callbacks_module = import_module("raganything.callbacks")
        except (ImportError, ModuleNotFoundError) as exc:
            raise RagAnythingUnavailable("RAG-Anything is not installed") from exc
        return (
            package.RAGAnything,
            package.RAGAnythingConfig,
            parser_module,
            callbacks_module,
        )

    @staticmethod
    def _register_passthrough_parser(parser_module: Any) -> None:
        if PARSER_NAME in parser_module.get_supported_parsers():
            return

        class PrivateAIParser(parser_module.Parser):
            """Satisfy RAG-Anything's parser contract for direct content insertion."""

            def check_installation(self) -> bool:
                return True

            def parse_document(self, *_args: Any, **_kwargs: Any) -> list[dict[str, object]]:
                raise RuntimeError("Private AI supplies pre-extracted content blocks")

        parser_module.register_parser(PARSER_NAME, PrivateAIParser)

    def _instance(self, namespace: str, lightrag: Any) -> tuple[Any, Any]:
        _, _, _, callbacks_module = self._bindings()
        existing = self._instances.get(namespace)
        if existing is not None:
            return existing, callbacks_module

        rag_class, config_class, parser_module, _ = self._bindings()
        self._register_passthrough_parser(parser_module)
        workdir = self.working_dir / namespace
        workdir.mkdir(parents=True, exist_ok=True)
        config = config_class(
            working_dir=str(workdir),
            parser_output_dir=str(workdir / "parser-output"),
            parser=PARSER_NAME,
            display_content_stats=False,
        )
        instance = rag_class(lightrag=lightrag, config=config)
        # LightRAGStore owns the shared LightRAG lifecycle. Letting both atexit handlers
        # finalize it can close the same storages twice during desktop shutdown.
        with suppress(Exception):
            atexit.unregister(instance.close)
        self._instances[namespace] = instance
        return instance, callbacks_module

    async def index_text(
        self,
        *,
        namespace: str,
        lightrag: Any,
        document_id: str,
        filename: str,
        text: str,
        on_progress: ProgressCallback,
    ) -> None:
        instance, callbacks_module = self._instance(namespace, lightrag)
        callback_base = callbacks_module.ProcessingCallback

        class ProgressAdapter(callback_base):
            def on_text_insert_start(self, **_kwargs: Any) -> None:
                on_progress(
                    {
                        "step": "chunking",
                        "progress": 0.45,
                        "detail": "RAG-Anything đang phân luồng nội dung và chia đoạn",
                        "engine": "rag-anything",
                    }
                )

            def on_text_insert_complete(self, **_kwargs: Any) -> None:
                on_progress(
                    {
                        "step": "finalizing",
                        "progress": 0.96,
                        "detail": "RAG-Anything đã hoàn tất embedding và graph memory",
                        "engine": "rag-anything",
                    }
                )

            def on_multimodal_start(self, item_count: int = 0, **_kwargs: Any) -> None:
                on_progress(
                    {
                        "step": "multimodal",
                        "progress": 0.78,
                        "detail": f"Đang xử lý {item_count} nội dung đa phương thức",
                        "engine": "rag-anything",
                    }
                )

            def on_multimodal_item_complete(
                self,
                item_index: int = 0,
                total_items: int = 0,
                **_kwargs: Any,
            ) -> None:
                ratio = (item_index + 1) / max(total_items, 1)
                on_progress(
                    {
                        "step": "multimodal",
                        "progress": 0.78 + min(ratio, 1.0) * 0.16,
                        "detail": (
                            f"Đã xử lý {item_index + 1}/{total_items} nội dung đa phương thức"
                        ),
                        "engine": "rag-anything",
                    }
                )

        callback = ProgressAdapter()
        instance.callback_manager.register(callback)
        try:
            await instance.insert_content_list(
                _content_list(text),
                file_path=filename,
                doc_id=document_id,
                display_stats=False,
            )
        finally:
            instance.callback_manager.unregister(callback)

    def clear(self) -> None:
        self._instances.clear()
