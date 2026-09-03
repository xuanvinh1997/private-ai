"""Cắt tài liệu thành đoạn, có nhớ mục và nhớ trang.

Là một :class:`~langchain_text_splitters.TextSplitter` thật, nên nó ghép được vào mọi
thứ khác của LangChain. Nhưng phần đáng nói không nằm ở giao diện mà ở ba luật:

**1. Đoạn không bao giờ vắt qua một tiêu đề hay một ranh giới trang.** Cả hai là điểm
xả. Lý do là trích dẫn: một đoạn nửa nằm ở trang 4 nửa ở trang 5 thì con số trang in ra
cạnh nó sai một nửa số lần, và người dùng mở tệp ra không thấy câu mình vừa đọc.

**2. Mỗi đoạn mang theo tiêu đề mục đang có hiệu lực.** "Phần Bảo mật nói gì" là câu hỏi
mà chỉ nội dung đoạn không trả lời được. Tiêu đề đi vào cả chỉ mục từ khoá lẫn văn bản
đem nhúng — xem :func:`embedding_text`.

**3. Không mất chữ.** Các đoạn phủ kín mọi ký tự không phải khoảng trắng, và đoạn sau bắt
đầu **trước** chỗ đoạn trước kết thúc. Phần chồng lấn tồn tại vì câu trả lời hay nhất
thường nằm vắt qua ranh giới: một câu hỏi và một câu đáp ở hai đoạn khác nhau thì không
đoạn nào trả lời được nó.

Thứ tự ưu tiên khi chọn chỗ cắt: **tiêu đề → đoạn văn → câu → cắt cứng.** Cắt cứng chỉ
xảy ra với một "câu" dài hơn cả một đoạn, tức là một bảng biểu hoặc một khối mã.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

from langchain_core.documents import Document
from langchain_text_splitters import TextSplitter

from pai_rag_service.extract.pages import PAGE_MARKER

__all__ = [
    "DEFAULT_SECTION",
    "Chunk",
    "SectionAwareSplitter",
    "embedding_text",
]

#: Tiêu đề gán cho phần văn bản đứng trước tiêu đề đầu tiên của tài liệu.
DEFAULT_SECTION = "Nội dung"

HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*$")
#: Một tiêu đề dài bất thường làm hỏng cả hàng trong bảng tài liệu lẫn dòng trích dẫn.
MAX_SECTION_TITLE = 240
#: Dấu kết câu **theo sau bởi khoảng trắng**. Điều kiện sau loại đúng hai thứ hay bị cắt
#: nhầm: số thập phân (``3.14``) và tên miền (``example.com``).
SENTENCE_END = re.compile(r"(?<=[.!?…;])\s+")


@dataclass(slots=True)
class Chunk:
    """Một đoạn, đủ để dựng một trích dẫn kiểm chứng được."""

    ordinal: int
    text: str
    section: str
    #: ``0`` nghĩa là định dạng này không có khái niệm trang.
    page: int = 0

    def to_document(self, **metadata: Any) -> Document:
        return Document(
            page_content=self.text,
            metadata={
                "ordinal": self.ordinal,
                "section": self.section,
                "page": self.page,
                **metadata,
            },
        )


def embedding_text_for(section: str, text: str) -> str:
    """Văn bản thật sự đem đi nhúng.

    Tiêu đề mục được ghép vào **trước** nội dung. Nghe như một chi tiết, nhưng nó là một
    trong những chỗ tầng Rust làm sai: bên đó chỉ mục từ khoá cân tiêu đề gấp đôi trong
    khi phần nhúng chỉ thấy thân đoạn, nên nửa ngữ nghĩa mất đúng cái ngữ cảnh mà nửa từ
    khoá coi là quan trọng nhất.

    Nhận hai chuỗi rời chứ không nhận một :class:`Chunk`, vì nó được gọi từ hai phía:
    lúc nạp thì có :class:`Chunk`, lúc nhúng bù thì chỉ có hàng đọc lên từ SQLite.

    Đổi hàm này là đổi ý nghĩa của mọi vector đã lưu. Có một khoá phiên bản canh chuyện
    đó — xem ``EMBED_INPUT_VERSION`` trong :mod:`pai_rag_service.embed`.
    """
    if section and section != DEFAULT_SECTION:
        return f"{section}\n\n{text}"
    return text


def embedding_text(chunk: Chunk) -> str:
    """:func:`embedding_text_for` cho một :class:`Chunk`."""
    return embedding_text_for(chunk.section, chunk.text)


@dataclass(slots=True)
class _Unit:
    """Đơn vị nhỏ nhất thuật toán chịu tách rời nhau."""

    text: str
    section: str
    page: int
    #: Đơn vị này có buộc mở một đoạn mới không (tiêu đề, hoặc sang trang).
    flush: bool


class SectionAwareSplitter(TextSplitter):
    """Cắt văn bản thành đoạn nhớ được mục và trang của mình."""

    def __init__(
        self,
        *,
        chunk_size: int = 1400,
        chunk_overlap: int = 180,
        default_section: str = DEFAULT_SECTION,
    ) -> None:
        # Chồng lấn bằng hoặc lớn hơn đoạn thì đoạn sau chứa trọn đoạn trước và việc cắt
        # không tiến lên được. Siết ở đây thay vì tin người gọi.
        size = max(1, chunk_size)
        super().__init__(chunk_size=size, chunk_overlap=min(max(0, chunk_overlap), size - 1))
        self.default_section = default_section

    # -- giao diện LangChain ----------------------------------------------------------

    def split_text(self, text: str) -> list[str]:
        return [chunk.text for chunk in self.split(text)]

    # -- phần thật --------------------------------------------------------------------

    def split(self, text: str) -> list[Chunk]:
        """Cắt, và trả về đoạn kèm mục và trang của nó."""
        units = self._units(text)
        return self._pack(units)

    def _units(self, text: str) -> list[_Unit]:
        """Bước một: văn bản → đơn vị, theo đúng thứ tự ưu tiên ở đầu tệp."""
        section = self.default_section
        page = 0
        units: list[_Unit] = []
        # Trang vừa đổi, nên đơn vị nội dung kế tiếp phải mở một đoạn mới. Cờ này chứ
        # không phải tự đánh dấu lên marker: marker không phải nội dung và không được
        # chiếm một đơn vị của riêng nó.
        page_turned = False

        for block in self._blocks(text):
            marker = PAGE_MARKER.match(block.strip())
            if marker:
                page = int(marker.group(1))
                page_turned = True
                continue

            heading = HEADING.match(block.strip())
            if heading:
                section = heading.group(2).strip()[:MAX_SECTION_TITLE] or self.default_section
                # Dòng tiêu đề **là** nội dung: nó mang chữ, và một câu hỏi hay khớp
                # đúng vào nó. Nó mở đoạn mới và thuộc về mục do chính nó đặt ra.
                units.append(_Unit(block.strip(), section, page, flush=True))
                page_turned = False
                continue

            for piece in self._fit(block.strip()):
                units.append(_Unit(piece, section, page, flush=page_turned))
                page_turned = False
        return units

    @staticmethod
    def _blocks(text: str) -> list[str]:
        """Khối: một dòng marker, một dòng tiêu đề, hoặc một chuỗi dòng liền nhau giữa
        hai dòng trắng. Dòng trắng là dấu ngăn và không thuộc khối nào."""
        blocks: list[str] = []
        open_lines: list[str] = []

        def close() -> None:
            if open_lines:
                blocks.append("\n".join(open_lines))
                open_lines.clear()

        for line in text.splitlines():
            stripped = line.strip()
            if not stripped:
                close()
                continue
            if PAGE_MARKER.match(stripped) or HEADING.match(stripped):
                close()
                blocks.append(stripped)
                continue
            open_lines.append(line.rstrip())
        close()
        return blocks

    def _fit(self, block: str) -> list[str]:
        """Một khối dài hơn cả đoạn thì xuống mức câu, rồi tới cắt cứng."""
        if len(block) <= self._chunk_size:
            return [block]

        out: list[str] = []
        for sentence in (part.strip() for part in SENTENCE_END.split(block)):
            if not sentence:
                continue
            if len(sentence) <= self._chunk_size:
                out.append(sentence)
                continue
            # Một "câu" dài hơn cả một đoạn là một bảng biểu hoặc một khối mã không có
            # dấu chấm nào. Đến đây không còn ranh giới ngữ nghĩa nào để tôn trọng —
            # nhưng vẫn trượt về ranh giới từ, vì cắt giữa một từ làm hỏng cả việc nhúng
            # lẫn việc đọc, còn mất vài chục ký tự thì không.
            out.extend(self._hard_split(sentence))
        return out

    def _hard_split(self, text: str) -> list[str]:
        out: list[str] = []
        start = 0
        limit = self._chunk_size
        while start < len(text):
            end = min(start + limit, len(text))
            if end < len(text):
                space = text.rfind(" ", start + limit * 3 // 4, end)
                if space > start:
                    end = space
            piece = text[start:end].strip()
            if piece:
                out.append(piece)
            # `end` bằng `start` chỉ xảy ra với một lát toàn khoảng trắng; đẩy lên một để
            # vòng lặp luôn tiến. Một vòng lặp không tiến là một ứng dụng treo.
            start = end if end > start else start + limit
        return out

    def _pack(self, units: list[_Unit]) -> list[Chunk]:
        """Bước hai: gộp đơn vị thành đoạn, rồi lùi lại lấy phần chồng lấn."""
        chunks: list[Chunk] = []
        open_units: list[_Unit] = []
        carry = ""

        # Ngưỡng để một tiêu đề được quyền mở đoạn mới. Không có nó thì một tài liệu toàn
        # tiêu đề ngắn sinh ra mỗi tiêu đề một đoạn; có nó thì các mục ngắn được gom lại.
        min_fill = self._chunk_size // 3

        def flush() -> None:
            nonlocal carry, open_units
            if not open_units:
                return
            body = "\n\n".join(unit.text for unit in open_units)
            text = f"{carry}\n\n{body}".strip() if carry else body
            chunks.append(
                Chunk(
                    ordinal=len(chunks),
                    text=text,
                    # Mục và trang lấy theo **đơn vị đầu tiên**, không theo phần thừa
                    # hưởng: đoạn này thuộc về mục mà nội dung mới của nó nằm trong.
                    section=open_units[0].section,
                    page=open_units[0].page,
                )
            )
            carry = self._overlap_tail(text)
            open_units = []

        filled = 0
        for unit in units:
            too_full = filled + len(unit.text) > self._chunk_size
            new_section = unit.flush and filled >= min_fill
            if open_units and (too_full or new_section):
                flush()
                filled = len(carry)
            open_units.append(unit)
            filled += len(unit.text)
        flush()
        return chunks

    def _overlap_tail(self, text: str) -> str:
        """Đuôi của đoạn vừa đóng, để đoạn sau chồng lên nó.

        Cắt theo **ký tự** rồi trượt tới đầu từ kế tiếp. Bản đầu tiên mang sang nguyên
        những đơn vị cuối vừa vặn trong ngân sách chồng lấn, và nó im lặng không làm gì
        cả: một đoạn văn thường dài hai ba trăm ký tự, ngân sách là 180, nên không đơn vị
        nào vừa và mọi đoạn ra đời không có chồng lấn.
        """
        if self._chunk_overlap <= 0 or len(text) <= self._chunk_overlap:
            return ""
        tail = text[-self._chunk_overlap :]
        space = tail.find(" ")
        # Một đuôi không có khoảng trắng nào là một từ dài hơn cả phần chồng lấn; giữ
        # nguyên vẫn hơn là bỏ hẳn chồng lấn.
        return tail[space + 1 :].strip() if space != -1 else tail.strip()
