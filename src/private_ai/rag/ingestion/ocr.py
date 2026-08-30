"""Vision-LLM OCR, wrapped so the rest of the app only sees a LangChain loader.

MarkItDown is the engine: the ``markitdown-ocr`` plugin is what actually reads an image,
and it is only loaded when ``enable_plugins`` is on — so turning OCR off has to drop the
plugin set along with the vision model, not merely stop passing a model name.

The OCR client is OpenAI-shaped but must point at whichever provider the user has
selected, not at a hard-coded local Ollama, which is why it resolves the endpoint from
the ``ModelRouter`` at the moment a converter is built.
"""

from __future__ import annotations

import asyncio
import threading
from collections.abc import AsyncIterator
from pathlib import Path
from typing import TYPE_CHECKING

from langchain_core.document_loaders import BaseLoader
from langchain_core.documents import Document
from markitdown import MarkItDown
from openai import OpenAI

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.llm.router import ModelRouter

__all__ = [
    "DESCRIPTION_MARKER",
    "IMAGE_OCR_PROMPT",
    "OCR_MARKER",
    "MarkItDownConverter",
    "VisionOcrLoader",
    "ocr_gap",
]

IMAGE_OCR_PROMPT = (
    "Extract every visible word from this image. Preserve headings, lists and tables as "
    "Markdown. Do not summarize, translate or invent missing text."
)
# What markitdown-ocr stamps on text it read out of a picture, and what its image
# describer writes when it only managed a caption. Either one proves OCR actually ran.
OCR_MARKER = "*[Image OCR]"
DESCRIPTION_MARKER = "# Description:"

FALLBACK_VISION_BASE_URL = "http://127.0.0.1:11434/v1"
FALLBACK_VISION_API_KEY = "ollama"


def ocr_gap(ocr_allowed: bool, vision_model: str) -> str:
    """Why a file produced no text, in the terms the user can act on."""
    if not ocr_allowed:
        return "OCR is off for this file, and it has no readable text layer"
    if not vision_model:
        return (
            "Nhà cung cấp đang bật không có mô hình nào đọc được ảnh. Cài hoặc chọn một "
            "mô hình vision, rồi bấm đọc lại."
        )
    return f"Mô hình {vision_model} đã chạy nhưng không đọc được chữ nào trong tệp này"


class MarkItDownConverter:
    """One cached MarkItDown per ``(model, ocr)`` pair.

    Building a converter loads the plugin set, so it is kept between files; but the
    signature it was built for is remembered, because a converter made without OCR
    silently returns an empty image and one made for the wrong model reads with it.
    """

    def __init__(self, router: ModelRouter | None = None, *, vision_model: str = "") -> None:
        self.router = router
        self.vision_model = vision_model.strip()
        self._markitdown: MarkItDown | None = None
        self._signature: tuple[str, bool] | None = None
        # MarkItDown is not thread-safe and conversions run on worker threads.
        self._lock = threading.RLock()

    def vision_endpoint(self) -> tuple[str, str]:
        """Base URL and key of the provider OCR should read through."""
        if self.router is None:
            return FALLBACK_VISION_BASE_URL, FALLBACK_VISION_API_KEY
        try:
            config = self.router.active_config()
        except Exception:
            return FALLBACK_VISION_BASE_URL, FALLBACK_VISION_API_KEY
        if config.kind == "ollama":
            return f"{config.base_url.rstrip('/')}/v1", config.api_key or "ollama"
        from private_ai.llm.router import openai_base_url

        return openai_base_url(config.base_url), config.api_key or "unused"

    def _vision_client(self) -> OpenAI:
        base_url, api_key = self.vision_endpoint()
        return OpenAI(base_url=base_url, api_key=api_key or "unused")

    def converter(self, ocr: bool, vision_model: str = "") -> MarkItDown:
        model = (vision_model.strip() or self.vision_model) if ocr else ""
        signature = (model, ocr)
        with self._lock:
            if self._markitdown is not None and self._signature == signature:
                return self._markitdown
            # The plugin set is where markitdown-ocr lives, so turning OCR off has to drop
            # it along with the vision model.
            options: dict[str, object] = {"enable_plugins": ocr}
            if model:
                options.update(
                    {
                        "llm_client": self._vision_client(),
                        "llm_model": model,
                        "llm_prompt": IMAGE_OCR_PROMPT,
                    }
                )
            self._markitdown = MarkItDown(**options)
            self._signature = signature
            return self._markitdown

    def convert(self, path: Path, ocr: bool, vision_model: str = "") -> str:
        with self._lock:
            return self.converter(ocr, vision_model).convert_local(path).markdown.strip()

    async def aconvert(self, path: Path, ocr: bool, vision_model: str = "") -> str:
        return await asyncio.to_thread(self.convert, path, ocr, vision_model)


class VisionOcrLoader(BaseLoader):
    """Read one image through the vision model, or read nothing at all.

    MarkItDown answers for every image it is handed, including ones it could not read, so
    the output is only trusted when it carries a marker the OCR plugin wrote. Anything
    else is dropped rather than stored as if it were the file's text — an empty result is
    what tells the pipeline to report ``needs_ocr`` with a reason.
    """

    def __init__(
        self,
        path: str | Path,
        *,
        converter: MarkItDownConverter | None = None,
        ocr: bool = True,
        vision_model: str = "",
    ) -> None:
        self.path = Path(path)
        self.converter = converter or MarkItDownConverter()
        self.ocr = ocr
        self.vision_model = vision_model

    async def alazy_load(self) -> AsyncIterator[Document]:
        try:
            converted = await self.converter.aconvert(self.path, self.ocr, self.vision_model)
        except Exception as exc:
            raise RuntimeError(f"OCR không thể xử lý {self.path.name}: {exc}") from exc
        if DESCRIPTION_MARKER not in converted and OCR_MARKER not in converted:
            return
        yield Document(
            page_content=f"<!-- private-ai-page:1 -->\n# Image OCR\n\n{converted}",
            metadata={"source": str(self.path), "loader": "image", "ocr": True},
        )
