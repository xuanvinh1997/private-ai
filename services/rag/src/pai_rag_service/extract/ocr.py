"""OCR with a vision model, for pages whose text layer is unreadable.
Page images are rendered here with pypdfium2 and sent over the OpenAI protocol (Ollama
included); a page that fails becomes an empty string rather than killing the file."""

from __future__ import annotations

import asyncio
import base64
import logging

import httpx

from pai_rag_service.config import OcrConfig, ProviderConfig
from pai_rag_service.errors import ExtractError

__all__ = ["PROMPT", "ocr_image_bytes", "read_pdf_pages", "render_pdf_pages"]

log = logging.getLogger(__name__)

#: The three prohibitions matter: without them a vision model describes the image instead of transcribing it.
PROMPT = (
    "Trích xuất toàn bộ chữ nhìn thấy được trong ảnh này. Giữ nguyên tiêu đề, danh sách "
    "và bảng ở dạng Markdown. Không tóm tắt, không dịch, không thêm bất cứ chữ nào không "
    "có trong ảnh. Nếu ảnh không có chữ nào, trả về đúng một dòng trống."
)

#: A dense page runs to a few thousand output tokens; 4096 also stops a small model looping forever.
MAX_TOKENS = 4096
#: A page can take tens of seconds on a large vision model during its first load.
PAGE_TIMEOUT = httpx.Timeout(180.0, connect=10.0)


def render_pdf_pages(data: bytes, *, scale: float, limit: int) -> list[bytes]:
    """Render each PDF page to a PNG; only the first `limit` pages, which the caller detects by count."""
    import pypdfium2

    out: list[bytes] = []
    document = pypdfium2.PdfDocument(data)
    try:
        for index in range(min(len(document), limit)):
            page = document[index]
            image = page.render(scale=scale).to_pil()
            from io import BytesIO

            buffer = BytesIO()
            image.save(buffer, format="PNG")
            out.append(buffer.getvalue())
    finally:
        document.close()
    return out


def _endpoint(provider: ProviderConfig) -> tuple[str, dict[str, str]]:
    root = provider.root()
    url = f"{root}/v1/chat/completions"
    headers = {"Content-Type": "application/json"}
    # Ollama ignores the key but does not mind it; OpenAI requires it. One `if` beats two code paths.
    if provider.api_key:
        headers["Authorization"] = f"Bearer {provider.api_key}"
    return url, headers


async def ocr_image_bytes(
    client: httpx.AsyncClient,
    image: bytes,
    *,
    provider: ProviderConfig,
    prompt: str = PROMPT,
) -> str:
    """Read the text in one image. Returns an empty string when the image has no text."""
    if not provider.model.strip():
        raise ExtractError("<ảnh>", "chưa chọn mô hình vision — không có gì để đọc ảnh")

    url, headers = _endpoint(provider)
    payload = {
        "model": provider.model,
        "max_tokens": MAX_TOKENS,
        # Temperature 0: this is transcription, not composition, and randomness here becomes invented text.
        "temperature": 0,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "data:image/png;base64,"
                            + base64.b64encode(image).decode("ascii")
                        },
                    },
                ],
            }
        ],
    }

    response = await client.post(url, headers=headers, json=payload, timeout=PAGE_TIMEOUT)
    if response.status_code >= 400:
        # Include the body: a bare `404` cannot distinguish "model not pulled" from "wrong endpoint".
        head = response.text.strip().splitlines()
        detail = head[0][:200] if head else ""
        raise ExtractError("<ảnh>", f"máy chủ vision trả {response.status_code}: {detail}")

    body = response.json()
    choices = body.get("choices") or []
    if not choices:
        raise ExtractError("<ảnh>", "máy chủ vision trả về phản hồi không có `choices`")
    content = (choices[0].get("message") or {}).get("content") or ""
    return content.strip()


async def read_pdf_pages(
    data: bytes,
    *,
    path: str,
    provider: ProviderConfig,
    ocr: OcrConfig,
) -> tuple[list[str], int]:
    """Read a scanned PDF page by page with the VLM; returns (text per page, pages skipped at the cap)."""
    images = await asyncio.to_thread(
        render_pdf_pages, data, scale=ocr.scale, limit=ocr.max_pages
    )
    if not images:
        raise ExtractError(path, "không dựng được trang nào thành ảnh")

    import pypdfium2

    document = pypdfium2.PdfDocument(data)
    try:
        total = len(document)
    finally:
        document.close()
    skipped = max(0, total - len(images))

    pages: list[str] = []
    async with httpx.AsyncClient() as client:
        for number, image in enumerate(images, start=1):
            try:
                pages.append(await ocr_image_bytes(client, image, provider=provider))
            except (ExtractError, httpx.HTTPError) as err:
                # A failed page is an empty page, not a failed file.
                log.warning("OCR failed on page %d of %s: %s", number, path, err)
                pages.append("")
    return pages, skipped
