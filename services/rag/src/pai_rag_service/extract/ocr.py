"""OCR bằng mô hình vision, cho những trang mà lớp chữ không đọc được.

# Vì sao tự dựng ảnh trang thay vì để markitdown lo

``markitdown-ocr`` đọc **ảnh nằm trong** tài liệu. Một PDF quét thì khác: cả trang là một
tấm ảnh, và thứ cần đọc là chính trang đó. Nên đường ở đây là dựng ảnh trang bằng
``pypdfium2`` rồi đưa thẳng cho VLM — không đi qua lớp plugin, vì lớp ấy giải một bài
khác.

# Vì sao gọi qua giao thức OpenAI cho cả Ollama

Ollama phơi ``/v1/chat/completions`` nhận ``image_url`` dạng data URI, y hệt OpenAI. Một
đường mã cho cả hai nghĩa là một chỗ để sửa khi lỗi, thay vì hai nhánh trôi ra khỏi nhau.

# Một trang hỏng chỉ làm hỏng chính nó

Trang thứ bảy làm VLM trả về rác không được phép giết cả tệp. :func:`read_pdf_pages` vì
thế bắt lỗi ở **từng trang**, và một trang không đọc được thành chuỗi rỗng — tệp vẫn có
mười chín trang còn lại.
"""

from __future__ import annotations

import asyncio
import base64
import logging

import httpx

from pai_rag_service.config import OcrConfig, ProviderConfig
from pai_rag_service.errors import ExtractError

__all__ = ["PROMPT", "ocr_image_bytes", "read_pdf_pages", "render_pdf_pages"]

log = logging.getLogger(__name__)

#: Nói rõ "đừng tóm tắt, đừng dịch, đừng bịa". Không có ba mệnh lệnh đó thì mô hình
#: vision hay trả về một câu mô tả tấm ảnh — "một trang văn bản tiếng Việt về hợp đồng" —
#: thứ đọc lên như đã OCR thành công nhưng không chứa một chữ nào của tài liệu.
PROMPT = (
    "Trích xuất toàn bộ chữ nhìn thấy được trong ảnh này. Giữ nguyên tiêu đề, danh sách "
    "và bảng ở dạng Markdown. Không tóm tắt, không dịch, không thêm bất cứ chữ nào không "
    "có trong ảnh. Nếu ảnh không có chữ nào, trả về đúng một dòng trống."
)

#: Một trang chữ dày đặc chạy tới vài nghìn token đầu ra. 4096 đủ cho mọi trang thật và
#: chặn được vòng lặp sinh chữ vô tận mà mô hình nhỏ đôi khi rơi vào.
MAX_TOKENS = 4096
#: Một trang có thể mất vài chục giây trên mô hình vision lớn ở lần nạp đầu tiên.
PAGE_TIMEOUT = httpx.Timeout(180.0, connect=10.0)


def render_pdf_pages(data: bytes, *, scale: float, limit: int) -> list[bytes]:
    """Dựng từng trang PDF thành ảnh PNG.

    Trả về đúng ``limit`` trang đầu khi tệp dài hơn thế — người gọi biết mình bị cắt vì
    nó so số trang trả về với số trang thật.
    """
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
    # Ollama không kiểm khoá nhưng cũng không phiền vì có nó; OpenAI thì bắt buộc. Gửi
    # khi có, im lặng khi không — một nhánh `if` ở đây rẻ hơn hai đường mã.
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
    """Đọc chữ trong một tấm ảnh. Trả về chuỗi rỗng khi ảnh không có chữ."""
    if not provider.model.strip():
        raise ExtractError("<ảnh>", "chưa chọn mô hình vision — không có gì để đọc ảnh")

    url, headers = _endpoint(provider)
    payload = {
        "model": provider.model,
        "max_tokens": MAX_TOKENS,
        # Nhiệt độ 0: đây là việc sao chép chữ, không phải việc sáng tác. Một chút ngẫu
        # nhiên ở đây đổi thành chữ bịa trong tài liệu của người dùng.
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
        # Kèm thân trả về: một `404` trơ trọi không phân biệt được "chưa kéo model về"
        # với "endpoint sai", mà đó là hai việc phải làm khác hẳn nhau.
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
    """Đọc một PDF quét, trang một, bằng VLM.

    Trả về ``(chữ theo trang, số trang đã bỏ qua vì chạm trần)``.
    """
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
                # Một trang hỏng là một trang trống, không phải một tệp hỏng.
                log.warning("OCR trang %d của %s hỏng: %s", number, path, err)
                pages.append("")
    return pages, skipped
