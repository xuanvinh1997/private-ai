"""Tệp → chữ. Một bất biến, và nó là lý do cả gói này tồn tại.

> **Một tệp hỏng chỉ được làm hỏng chính nó.**

Người dùng thả hai mươi tệp vào một lúc; tệp thứ bảy là một PDF cụt lúc tải về. Nếu tệp
đó giết cả mẻ thì mười ba tệp còn lại không bao giờ được nạp, và người dùng không có cách
nào biết tệp nào là thủ phạm. Vì thế mọi đường ở đây trả về :class:`Extracted` hoặc ném
:class:`~pai_rag_service.errors.ExtractError` **có kèm đường dẫn** — không có đường nào
ném ra một lỗi không nói được nó nói về tệp nào.

# Cascade của PDF, và vì sao nó cần thiết

PDF là định dạng duy nhất mà "đọc được" không phải một câu trả lời có hoặc không. Một tệp
xuất từ Word có lớp chữ hoàn hảo; một tệp quét từ máy photocopy có lớp chữ **rỗng** nhưng
vẫn là một PDF hợp lệ mở được bình thường. Đọc lớp chữ rồi dừng lại ở đó nghĩa là tệp
quét vào thư viện với 0 đoạn, và ở tầng Rust nó nằm lại vĩnh viễn trong bảng ``failures``
vì ``mtime`` không đổi nên không lần quét nào chạm lại vào nó.

Nên: đọc lớp chữ trước — nó nhanh và chính xác tuyệt đối khi có. Đếm ký tự trên mỗi
trang. Dưới ngưỡng thì trang đó đi qua VLM. Ngưỡng chứ không phải "rỗng hay không", vì
một tệp quét vẫn thường có vài chục ký tự rác từ header hoặc số trang.

# Định dạng suy từ phần mở rộng

Đoán theo magic bytes nghe chắc chắn hơn nhưng sai ở đúng chỗ quan trọng: một tệp ``.md``
viết bằng chữ Việt trông y hệt một tệp ``.txt``, và người dùng đã nói ra ý định của họ
ngay trong cái tên.
"""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
from pathlib import Path

from pai_rag_service.config import OcrConfig, ProviderConfig
from pai_rag_service.errors import ExtractError, UnsupportedFile
from pai_rag_service.extract.ocr import PROMPT, ocr_image_bytes, read_pdf_pages
from pai_rag_service.extract.pages import split_pages, strip_markers

__all__ = [
    "EXTRACT_VERSION",
    "Extracted",
    "MAX_FILE_BYTES",
    "SUPPORTED_EXTENSIONS",
    "extract",
    "format_for",
]

log = logging.getLogger(__name__)

#: Bộ rút chữ đang chạy là bản mấy.
#:
#: Chỉ mục là tăng dần: tệp có ``mtime`` và kích thước không đổi thì **không** đi qua bộ
#: rút chữ lần nữa. Bất biến đó tiết kiệm cả một buổi chờ, và nó cũng có nghĩa là một tệp
#: từng đọc ra rác sẽ ở lại dạng rác vĩnh viễn. Tăng số này mỗi khi bộ rút chữ **đọc ra
#: kết quả khác** trên cùng một tệp; lần mở kho sau đó sẽ vô hiệu hoá dấu vân tay cũ.
EXTRACT_VERSION = 1

#: 64 MiB. Không phải một con số thiêng liêng — nó là chỗ mà cả markitdown lẫn phần dựng
#: ảnh trang còn nằm gọn trong RAM. Cả hai **đọc cả tệp vào bộ nhớ**.
MAX_FILE_BYTES = 64 * 1024 * 1024

#: Bao nhiêu byte đầu được soi để kết luận "đây là tệp nhị phân".
BINARY_PROBE = 4096

TEXT_EXTENSIONS = frozenset(
    {".txt", ".text", ".log", ".rst", ".adoc", ".org", ".md", ".markdown", ".mdx"}
)
#: Định dạng markitdown đọc tốt hơn bất cứ thứ gì tự viết được.
OFFICE_EXTENSIONS = frozenset({".docx", ".xlsx", ".xlsm", ".pptx", ".doc", ".xls", ".ppt"})
DATA_EXTENSIONS = frozenset({".csv", ".tsv", ".json", ".xml", ".yaml", ".yml"})
WEB_EXTENSIONS = frozenset({".html", ".htm", ".xhtml"})
IMAGE_EXTENSIONS = frozenset(
    {".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".tif", ".tiff"}
)
PDF_EXTENSIONS = frozenset({".pdf"})
#: Mã nguồn nằm trong thư viện tài liệu là chuyện có thật — một thư mục tài liệu kỹ thuật
#: hay có tệp ví dụ kèm theo. Đọc như văn bản thuần.
CODE_EXTENSIONS = frozenset(
    {
        ".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".kt", ".c", ".h",
        ".cpp", ".hpp", ".cs", ".rb", ".php", ".swift", ".sh", ".ps1", ".sql", ".toml",
        ".ini", ".cfg", ".conf", ".gradle", ".lua", ".r", ".m", ".scala", ".dart",
    }
)

SUPPORTED_EXTENSIONS = (
    TEXT_EXTENSIONS
    | OFFICE_EXTENSIONS
    | DATA_EXTENSIONS
    | WEB_EXTENSIONS
    | IMAGE_EXTENSIONS
    | PDF_EXTENSIONS
    | CODE_EXTENSIONS
)


def format_for(path: Path) -> str:
    """Nhãn định dạng, hoặc ``""`` khi tệp nằm ngoài tập đọc được."""
    suffix = path.suffix.lower()
    if suffix in PDF_EXTENSIONS:
        return "pdf"
    if suffix in OFFICE_EXTENSIONS:
        return "office"
    if suffix in IMAGE_EXTENSIONS:
        return "image"
    if suffix in WEB_EXTENSIONS:
        return "html"
    if suffix in DATA_EXTENSIONS:
        return "data"
    if suffix in {".md", ".markdown", ".mdx"}:
        return "markdown"
    if suffix in CODE_EXTENSIONS:
        return "code"
    if suffix in TEXT_EXTENSIONS or suffix == "":
        return "text"
    return ""


@dataclass(slots=True)
class Extracted:
    """Chữ của một tệp, cùng những gì cần nói ra về cách nó được đọc."""

    text: str
    format: str
    title: str
    #: Số trang, khi khái niệm ấy có nghĩa với định dạng này.
    pages: int = 0
    #: Trang nào đã phải đi qua VLM. Giao diện dùng nó để nói "12/40 trang đọc bằng OCR",
    #: và đó là thứ giải thích vì sao một tệp nạp lâu hơn hẳn những tệp khác.
    ocr_pages: list[int] = field(default_factory=list)
    #: Trang bị bỏ qua vì chạm trần ``OcrConfig.max_pages``. Nói ra chứ không nuốt: một
    #: thư viện thiếu chữ mà không giải thích là đúng lỗi mà cả gói này sinh ra để sửa.
    skipped_pages: int = 0

    @property
    def chars(self) -> int:
        return len(strip_markers(self.text))


def _looks_binary(data: bytes) -> bool:
    return b"\x00" in data[:BINARY_PROBE]


def _as_text(path: Path, data: bytes) -> str:
    """Byte → chuỗi, thử UTF-8 trước rồi tới bảng mã Windows tiếng Việt.

    ``cp1258`` có mặt vì tài liệu tiếng Việt cũ thật sự dùng nó, và một tệp đọc sai bảng
    mã không hỏng — nó vào thư viện đầy ký tự vô nghĩa và không bao giờ khớp câu hỏi nào.
    """
    for codec in ("utf-8", "utf-8-sig", "cp1258", "cp1252"):
        try:
            return data.decode(codec)
        except UnicodeDecodeError:
            continue
    # Đến đây thì không còn bảng mã nào để thử; thay ký tự hỏng còn hơn bỏ cả tệp.
    return data.decode("utf-8", errors="replace")


def _markitdown_text(path: Path) -> str:
    """Chạy markitdown trên một tệp. Đồng bộ và tốn CPU — người gọi đẩy sang thread."""
    from markitdown import MarkItDown

    converter = MarkItDown(enable_plugins=False)
    try:
        result = converter.convert(str(path))
    except Exception as err:  # markitdown ném đủ loại lỗi của thư viện bên dưới
        raise ExtractError(str(path), f"markitdown không đọc được: {err}") from err
    return (result.text_content or "").strip()


def _pdf_text_layer(path: Path, data: bytes) -> list[str]:
    """Lớp chữ của PDF, theo trang. Rỗng ở một trang nghĩa là trang ấy không có chữ."""
    from io import BytesIO

    from pypdf import PdfReader

    try:
        reader = PdfReader(BytesIO(data))
        return [(page.extract_text() or "").strip() for page in reader.pages]
    except Exception as err:
        raise ExtractError(str(path), f"không mở được PDF: {err}") from err


async def _extract_pdf(
    path: Path,
    data: bytes,
    *,
    vision: ProviderConfig,
    ocr: OcrConfig,
) -> Extracted:
    native = await asyncio.to_thread(_pdf_text_layer, path, data)
    total = len(native)
    if total == 0:
        raise ExtractError(str(path), "PDF không có trang nào")

    dense = sum(len(page) for page in native)
    average = dense / total
    if average >= ocr.min_chars_per_page:
        # Lớp chữ đủ dày. Đây là đường nhanh và chính xác nhất; đừng đụng tới VLM.
        return Extracted(
            text=split_pages(native),
            format="pdf",
            title=path.stem,
            pages=total,
        )

    if not ocr.enabled:
        raise ExtractError(
            str(path),
            f"lớp chữ chỉ có {average:.0f} ký tự/trang và OCR đang tắt. Bật OCR trong "
            "Cài đặt để đọc tệp quét.",
        )
    if not vision.model.strip():
        raise ExtractError(
            str(path),
            "đây là tệp quét và chưa chọn mô hình vision. Chọn một mô hình đọc được ảnh "
            "trong Cài đặt rồi nạp lại.",
        )

    read, skipped = await read_pdf_pages(data, path=str(path), provider=vision, ocr=ocr)
    # Giữ lớp chữ gốc ở những trang vốn đã dày: OCR một trang chữ in rõ ràng chỉ làm nó
    # tệ đi, và mô hình vision hay bỏ sót bảng biểu mà lớp chữ giữ đúng.
    merged: list[str] = []
    ocr_pages: list[int] = []
    for index in range(total):
        original = native[index] if index < len(native) else ""
        scanned = read[index] if index < len(read) else ""
        if len(original) >= ocr.min_chars_per_page:
            merged.append(original)
        elif scanned:
            merged.append(scanned)
            ocr_pages.append(index + 1)
        else:
            merged.append(original)

    if not any(part.strip() for part in merged):
        raise ExtractError(
            str(path),
            f"mô hình {vision.model} đã chạy nhưng không đọc được chữ nào trong tệp này",
        )

    return Extracted(
        text=split_pages(merged),
        format="pdf",
        title=path.stem,
        pages=total,
        ocr_pages=ocr_pages,
        skipped_pages=skipped,
    )


async def _extract_image(
    path: Path, data: bytes, *, vision: ProviderConfig
) -> Extracted:
    import httpx

    if not vision.model.strip():
        raise UnsupportedFile(
            str(path),
            "đây là ảnh và chưa chọn mô hình vision. Chọn một mô hình đọc được ảnh "
            "trong Cài đặt rồi nạp lại.",
        )
    async with httpx.AsyncClient() as client:
        text = await ocr_image_bytes(client, data, provider=vision, prompt=PROMPT)
    if not text.strip():
        raise ExtractError(str(path), f"mô hình {vision.model} không đọc được chữ nào trong ảnh")
    return Extracted(text=text, format="image", title=path.stem, pages=1, ocr_pages=[1])


async def extract(
    path: Path,
    *,
    vision: ProviderConfig,
    ocr: OcrConfig,
) -> Extracted:
    """Đọc một tệp thành chữ.

    Ném :class:`UnsupportedFile` cho định dạng nằm ngoài tập đọc được — khác
    :class:`ExtractError` vì nó **không** đáng thử lại — và :class:`ExtractError` cho mọi
    thứ khác.
    """
    if not path.is_file():
        raise ExtractError(str(path), "không phải một tệp")

    size = path.stat().st_size
    if size == 0:
        raise ExtractError(str(path), "tệp rỗng")
    if size > MAX_FILE_BYTES:
        raise UnsupportedFile(
            str(path),
            f"tệp {size / 1024 / 1024:.0f} MB vượt trần {MAX_FILE_BYTES // 1024 // 1024} MB",
        )

    kind = format_for(path)
    if not kind:
        shown = path.suffix or "(không đuôi)"
        raise UnsupportedFile(str(path), f"chưa đọc được định dạng `{shown}`")

    data = await asyncio.to_thread(path.read_bytes)

    if kind == "pdf":
        return await _extract_pdf(path, data, vision=vision, ocr=ocr)
    if kind == "image":
        return await _extract_image(path, data, vision=vision)
    if kind in {"office", "html", "data"}:
        text = await asyncio.to_thread(_markitdown_text, path)
        if not text:
            raise ExtractError(str(path), "markitdown đọc ra một tài liệu rỗng")
        return Extracted(text=text, format=kind, title=path.stem)

    # Còn lại là văn bản thuần: markdown, mã nguồn, txt.
    if _looks_binary(data):
        raise UnsupportedFile(str(path), "trông như tệp nhị phân dù mang đuôi văn bản")
    text = _as_text(path, data).strip()
    if not text:
        raise ExtractError(str(path), "tệp chỉ có khoảng trắng")
    return Extracted(text=text, format=kind, title=path.stem)
