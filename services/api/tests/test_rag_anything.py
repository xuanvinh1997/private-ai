from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest

from private_ai_api.services.rag_anything import RagAnythingOrchestrator, _content_list


def test_content_list_preserves_private_ai_page_boundaries() -> None:
    assert _content_list(
        "<!-- private-ai-page:1 -->\nTrang một\n\n<!-- private-ai-page:3 -->\nTrang ba"
    ) == [
        {"type": "text", "text": "Trang một", "page_idx": 0},
        {"type": "text", "text": "Trang ba", "page_idx": 2},
    ]


@pytest.mark.asyncio
async def test_orchestrator_routes_content_and_forwards_progress(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    registered_parsers: dict[str, type] = {}
    inserted: dict[str, Any] = {}

    class Parser:
        pass

    class ParserModule:
        @staticmethod
        def get_supported_parsers() -> tuple[str, ...]:
            return tuple(registered_parsers)

        @staticmethod
        def register_parser(name: str, parser: type) -> None:
            registered_parsers[name] = parser

    ParserModule.Parser = Parser

    class ProcessingCallback:
        pass

    class CallbacksModule:
        pass

    CallbacksModule.ProcessingCallback = ProcessingCallback

    class CallbackManager:
        def __init__(self) -> None:
            self.callbacks: list[Any] = []

        def register(self, callback: Any) -> None:
            self.callbacks.append(callback)

        def unregister(self, callback: Any) -> None:
            self.callbacks.remove(callback)

    class Config:
        def __init__(self, **values: Any) -> None:
            self.values = values

    class RagAnything:
        def __init__(self, *, lightrag: Any, config: Config) -> None:
            self.lightrag = lightrag
            self.config = config
            self.callback_manager = CallbackManager()

        def close(self) -> None:
            return None

        async def insert_content_list(
            self, content: list[dict[str, object]], **values: Any
        ) -> None:
            inserted.update({"content": content, **values})
            for callback in self.callback_manager.callbacks:
                callback.on_text_insert_start()
                callback.on_text_insert_complete()

    orchestrator = RagAnythingOrchestrator(tmp_path)
    monkeypatch.setattr(
        orchestrator,
        "_bindings",
        lambda: (RagAnything, Config, ParserModule, CallbacksModule),
    )
    events: list[dict[str, object]] = []

    await orchestrator.index_text(
        namespace="workspace",
        lightrag=object(),
        document_id="document-1",
        filename="guide.pdf",
        text="Nội dung",
        on_progress=events.append,
    )

    assert inserted == {
        "content": [{"type": "text", "text": "Nội dung", "page_idx": 0}],
        "file_path": "guide.pdf",
        "doc_id": "document-1",
        "display_stats": False,
    }
    assert registered_parsers
    assert [event["step"] for event in events] == ["chunking", "finalizing"]
    assert all(event["engine"] == "rag-anything" for event in events)
