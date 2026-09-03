"""Nạp tài liệu: quét thư mục → rút chữ → cắt đoạn → nhúng → ghi kho.

# Ba bất biến

**1. Một tệp hỏng chỉ làm hỏng chính nó.** Tệp thứ bảy là một PDF cụt thì mười ba tệp
còn lại vẫn vào thư viện, và tệp hỏng được ghi vào bảng ``failures`` kèm lý do đọc được.

**2. Quét lại một thư mục không đổi thì không rút chữ lại tệp nào.** Dấu vân tay là
``mtime`` + kích thước. Đây là thứ biến "mở lại dự án" từ một buổi chờ thành một giây.

**3. Nhúng là bước được phép hỏng.** Ollama tắt thì tài liệu **vẫn** được rút chữ, cắt
đoạn và đưa vào FTS5 — tìm bằng từ khoá chạy ngay. :meth:`Pipeline.embed_pending` dọn nốt
khi Ollama quay lại. Nếu bước nhúng bắt buộc thì một máy chủ chưa bật biến cả lần nạp
thành công cốc, và người dùng không có gì trong tay cả.

# Danh tính, và khi nào phải làm lại từ đầu

Ba thứ quyết định ý nghĩa của những gì đã lưu: bản của bộ rút chữ, cách dựng văn bản đem
nhúng, và model nhúng. Đổi bất cứ cái nào là những gì đang nằm trong kho không còn so
sánh được với những gì sắp ghi vào. :meth:`Pipeline.open` so cả ba với thứ đã ghi và tự
dọn — quên dấu vân tay, dựng lại collection — chứ không để người dùng phải biết rằng họ
vừa cần làm việc đó.
"""

from __future__ import annotations

import hashlib
import logging
import time
from collections.abc import Iterator
from dataclasses import dataclass, field
from pathlib import Path

from pai_rag_service import store as store_meta
from pai_rag_service.chunking import SectionAwareSplitter, embedding_text_for
from pai_rag_service.config import ProjectConfig, RagConfig
from pai_rag_service.embed import EMBED_INPUT_VERSION, embedder_for
from pai_rag_service.errors import EmbedError, ExtractError, RagError, VectorStoreError
from pai_rag_service.extract import (
    EXTRACT_VERSION,
    SUPPORTED_EXTENSIONS,
    extract,
)
from pai_rag_service.store import ChunkRow, Store
from pai_rag_service.vectors import VectorStore

__all__ = ["MAX_FILES", "Pipeline", "SyncReport", "scan"]

log = logging.getLogger(__name__)

#: Bao nhiêu tệp một lần quét chịu nạp.
#:
#: Người dùng chỉ vào thư mục Downloads mười nghìn tệp là chuyện có thật, và mười nghìn
#: lần rút chữ cộng mười nghìn lần gọi bộ nhúng không phải một lần chờ lâu — nó là một
#: ứng dụng đứng hình cả buổi. Chạm trần thì **nói ra**, không lặng lẽ dừng.
MAX_FILES = 5_000

#: Thư mục không bao giờ chứa tài liệu của người dùng, chỉ chứa thứ máy sinh ra. Quét vào
#: đây là nạp hàng nghìn tệp mã nguồn của thư viện bên thứ ba vào thư viện tài liệu.
SKIP_DIRS = frozenset(
    {
        ".git", ".hg", ".svn", ".venv", "venv", "env", "node_modules", "__pycache__",
        ".mypy_cache", ".pytest_cache", ".ruff_cache", "target", "dist", "build",
        ".next", ".nuxt", ".cache", ".idea", ".vscode", ".tox", "site-packages",
        ".gradle", ".terraform", "vendor", ".DS_Store", "$RECYCLE.BIN",
    }
)


@dataclass(slots=True)
class SyncReport:
    """Kết quả một lần đồng bộ, đủ để giao diện nói ra chuyện gì đã xảy ra."""

    scanned: int = 0
    ingested: int = 0
    skipped_unchanged: int = 0
    failed: list[tuple[str, str]] = field(default_factory=list)
    embedded_chunks: int = 0
    #: Tệp bị bỏ qua vì chạm :data:`MAX_FILES`.
    over_limit: int = 0
    #: Tệp còn trong thư mục nhưng người dùng đã bỏ khỏi thư viện.
    excluded: int = 0
    #: Lý do phần ngữ nghĩa chưa sẵn sàng, khi nó chưa sẵn sàng.
    embed_error: str | None = None
    rebuilt: bool = False

    def as_dict(self) -> dict[str, object]:
        return {
            "scanned": self.scanned,
            "ingested": self.ingested,
            "skipped_unchanged": self.skipped_unchanged,
            "failed": [{"path": path, "reason": reason} for path, reason in self.failed],
            "embedded_chunks": self.embedded_chunks,
            "over_limit": self.over_limit,
            "excluded": self.excluded,
            "embed_error": self.embed_error,
            "rebuilt": self.rebuilt,
        }


def scan(root: Path, limit: int = MAX_FILES) -> tuple[list[Path], int]:
    """Tệp đọc được trong thư mục dự án, và số tệp bị bỏ vì chạm trần."""
    found: list[Path] = []
    over = 0
    for path in _walk(root):
        if path.suffix.lower() not in SUPPORTED_EXTENSIONS:
            continue
        if len(found) >= limit:
            over += 1
            continue
        found.append(path)
    return found, over


def _walk(root: Path) -> Iterator[Path]:
    """Đi cây thư mục, bỏ qua thư mục máy sinh và tệp ẩn."""
    stack = [root]
    while stack:
        current = stack.pop()
        try:
            entries = list(current.iterdir())
        except (PermissionError, OSError) as err:
            # Một thư mục không đọc được không được làm hỏng cả lần quét.
            log.debug("bỏ qua thư mục %s: %s", current, err)
            continue
        for entry in entries:
            name = entry.name
            if name.startswith(".") or name in SKIP_DIRS:
                continue
            try:
                if entry.is_dir():
                    stack.append(entry)
                elif entry.is_file():
                    yield entry
            except OSError:
                continue


def document_id(root: Path, path: Path) -> str:
    """Mã ổn định của một tài liệu, suy từ đường dẫn tương đối.

    Băm chứ không dùng thẳng đường dẫn: mã này đi vào tên nhãn Neo4j và khoá payload
    Qdrant, mà đường dẫn Windows có dấu hai chấm, dấu gạch ngược và khoảng trắng. Băm
    theo đường dẫn **tương đối** để di chuyển cả thư mục dự án không đổi mã của mọi
    tài liệu trong đó.
    """
    try:
        rel = path.relative_to(root).as_posix()
    except ValueError:
        rel = path.as_posix()
    return hashlib.sha1(rel.encode("utf-8")).hexdigest()[:16]


class Pipeline:
    """Nạp và giữ đồng bộ thư viện của một dự án."""

    def __init__(self, config: RagConfig, project: ProjectConfig) -> None:
        self.config = config
        self.project = project
        self.root = project.local_root()
        self.store = Store(config.store_path(project))
        self.splitter = SectionAwareSplitter(
            chunk_size=config.chunk.size, chunk_overlap=config.chunk.overlap
        )
        self.vectors = VectorStore(config.vectors, config.collection(project))
        self._embedder = None

    def close(self) -> None:
        self.store.close()

    def __enter__(self) -> Pipeline:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- danh tính ---------------------------------------------------------------------

    @property
    def embedder(self):
        if self._embedder is None:
            self._embedder = embedder_for(self.config.embedding)
        return self._embedder

    def reconcile(self) -> bool:
        """So danh tính đang chạy với danh tính đã ghi, và dọn khi lệch.

        Trả về ``True`` khi có gì đó phải làm lại. Xem docstring của module.
        """
        seen = self.store.identity()
        model = self.config.embedding.model
        stale_extract = seen["extract"] != str(EXTRACT_VERSION)
        stale_input = seen["embed_input"] != str(EMBED_INPUT_VERSION)
        stale_model = seen["embedder"] not in (None, model)

        if stale_extract or stale_input:
            # Cách đọc tệp hoặc cách dựng văn bản nhúng đã đổi: mọi thứ phải đi lại từ
            # đầu, kể cả những tệp không ai sửa.
            count = self.store.forget_fingerprints()
            log.info(
                "bộ rút chữ hoặc đầu vào nhúng đã đổi — sẽ đọc lại %d tài liệu", count
            )
        self.store.set_identity(
            embedder=model,
            dim=self.config.embedding.dim,
            embed_input=EMBED_INPUT_VERSION,
            extract=EXTRACT_VERSION,
        )
        return stale_extract or stale_input or stale_model

    # -- nạp ---------------------------------------------------------------------------

    async def sync(self) -> SyncReport:
        """Bắt kịp thư mục dự án. Gọi lại bao nhiêu lần cũng được."""
        report = SyncReport()
        report.rebuilt = self.reconcile()

        if not self.root.is_dir():
            raise RagError(
                f"thư mục dự án `{self.root}` không tồn tại. Kiểm tra lại đường dẫn "
                "trong ứng dụng."
            )

        files, report.over_limit = scan(self.root)
        report.scanned = len(files)
        known = self.store.known_files()
        # Tệp người dùng đã bỏ khỏi thư viện thì vẫn nằm trong thư mục, nên lần quét nào
        # cũng thấy nó. Không lọc ở đây thì `remove` là một nút bấm không có tác dụng.
        excluded = self.store.excluded()

        for path in files:
            if str(path) in excluded:
                report.excluded += 1
                continue
            try:
                stat = path.stat()
            except OSError as err:
                report.failed.append((str(path), f"không đọc được thuộc tính: {err}"))
                continue
            fingerprint = (int(stat.st_mtime), int(stat.st_size))
            if known.get(str(path)) == fingerprint:
                report.skipped_unchanged += 1
                continue
            try:
                await self.ingest(path)
                report.ingested += 1
            except ExtractError as err:
                # Bất biến 1: ghi lại rồi đi tiếp.
                #
                # Ghi vào `failures` **kèm dấu vân tay** vì đây là tính chất của chính
                # tệp: nó sẽ đọc hỏng y như vậy ở mọi lần thử, cho tới khi người dùng sửa
                # tệp — mà sửa thì `mtime` đổi và nó tự được thử lại.
                self.store.put_failure(str(path), fingerprint[0], fingerprint[1], err.reason)
                report.failed.append((str(path), err.reason))
            except RagError as err:
                # Lỗi hạ tầng — Qdrant chết, máy chủ nhúng không trả lời — thì **không**
                # ghi vào `failures`. Ghi vào đó là đóng dấu vân tay lên một tệp hoàn toàn
                # lành, và lần quét sau sẽ thấy nó "không đổi" rồi bỏ qua vĩnh viễn: một
                # sự cố mười giây biến thành một tài liệu không bao giờ vào thư viện.
                # Báo trong report để người dùng thấy, rồi thử lại ở lần sync kế tiếp.
                report.failed.append((str(path), str(err)))

        # Nhúng sau cùng, một lượt cho cả mẻ: gọi bộ nhúng theo lô lớn rẻ hơn nhiều so với
        # gọi từng tài liệu, và nó cũng gom mọi đoạn còn nợ từ những lần trước.
        try:
            report.embedded_chunks = await self.embed_pending()
        except (EmbedError, VectorStoreError) as err:
            # Bất biến 3: nhúng hỏng không làm hỏng lần nạp.
            report.embed_error = str(err)
            log.warning("chưa nhúng được: %s", err)

        # Ghi lại lượt quét này. Giao diện đọc ba con số ấy để nói "quét 240 tệp lúc
        # 14:05, bỏ qua 3" ngay khi vừa mở dự án — trước cả lần quét đầu của phiên mới.
        self.store.set_meta(store_meta.META_SCAN_FILES, str(report.scanned))
        self.store.set_meta(store_meta.META_SCAN_SKIPPED, str(report.over_limit))
        self.store.set_meta(store_meta.META_SCAN_AT, str(int(time.time() * 1000)))
        return report

    async def ingest(self, path: Path) -> str:
        """Rút chữ, cắt đoạn và ghi một tệp vào kho. Trả về mã tài liệu."""
        got = await extract(path, vision=self.config.vision, ocr=self.config.ocr)
        chunks = self.splitter.split(got.text)
        if not chunks:
            raise ExtractError(str(path), "đọc được tệp nhưng không cắt ra đoạn nào")

        stat = path.stat()
        doc_id = document_id(self.root, path)
        # Dọn vector cũ của tài liệu này, nhưng **không** để việc đó chặn lần nạp.
        #
        # Một tệp bị sửa ngắn đi sẽ để lại vector mồ côi trong Qdrant. Chúng vô hại với
        # tính đúng đắn của kết quả: phép tìm ánh xạ mã điểm Qdrant về hàng `chunks` bằng
        # `chunks_by_id`, và một mã không còn hàng nào thì rơi ra khỏi kết quả. Chúng chỉ
        # tốn chỗ, và lần nhúng kế tiếp ghi đè lên đúng những mã được dùng lại.
        #
        # Đổi lại, để `VectorStoreError` thoát ra ở đây là đánh đổi cả bất biến 3: Qdrant
        # chết thì **không tệp nào** vào được thư viện, kể cả phần rút chữ và FTS5 vốn
        # không cần Qdrant một chút nào.
        try:
            self.vectors.remove_document(doc_id)
        except VectorStoreError as err:
            log.debug("chưa dọn được vector cũ của %s: %s", path.name, err)
        self.store.put_document(
            doc_id=doc_id,
            path=str(path),
            title=got.title,
            fmt=got.format,
            size=int(stat.st_size),
            mtime=int(stat.st_mtime),
            pages=got.pages,
            ocr_pages=got.ocr_pages,
            chunks=chunks,
        )
        if got.ocr_pages:
            log.info("%s: đọc %d trang bằng OCR", path.name, len(got.ocr_pages))
        return doc_id

    async def embed_pending(self) -> int:
        """Nhúng mọi đoạn chưa có vector. Trả về số đoạn vừa nhúng.

        Gọi được nhiều lần và gọi lúc nào cũng được: đây là đường mà một thư viện đã nạp
        lúc Ollama tắt đi theo để bắt kịp khi Ollama bật lại.
        """
        rows = self._all_chunks()
        if not rows:
            return 0

        model = self.config.embedding.model
        # Số chiều chỉ biết được bằng cách nhúng thử một đoạn — nhiều máy chủ không khai
        # nó ở đâu cả. Nhúng đúng một đoạn, đọc độ dài, rồi mới dựng collection.
        probe = await self.embedder.aembed_documents(
            [embedding_text_for(rows[0].section, rows[0].body)]
        )
        if not probe or not probe[0]:
            raise EmbedError(f"model `{model}` trả về vector rỗng")
        dim = len(probe[0])
        rebuilt = self.vectors.ensure(dim=dim, model=model, input_version=EMBED_INPUT_VERSION)

        # Collection vừa dựng lại thì không điểm nào còn; hỏi Qdrant chỉ để nhận lại một
        # tập rỗng là một vòng gọi thừa.
        already: set[int] = (
            set() if rebuilt else self.vectors.existing_ids([row.id for row in rows])
        )
        pending = [row for row in rows if row.id not in already]
        if not pending:
            return 0

        total = 0
        for batch in _batched(pending, 64):
            texts = [embedding_text_for(row.section, row.body) for row in batch]
            vectors = await self.embedder.aembed_documents(texts)
            if len(vectors) != len(batch):
                raise EmbedError(f"xin {len(batch)} vector nhưng nhận {len(vectors)}")
            self.vectors.upsert(
                chunk_ids=[row.id for row in batch],
                vectors=vectors,
                payloads=[
                    {
                        "document_id": row.document_id,
                        "ordinal": row.ordinal,
                        "page": row.page,
                    }
                    for row in batch
                ],
                model=model,
                input_version=EMBED_INPUT_VERSION,
            )
            total += len(batch)
        return total

    def _all_chunks(self) -> list[ChunkRow]:
        """Mọi đoạn trong kho, kèm đủ thứ để dựng cả văn bản nhúng lẫn payload.

        Đọc một lần từ SQLite thay vì hỏi lại từng đoạn: mỗi hàng đã mang sẵn
        ``document_id``, ``ordinal`` và ``page``, nên không có lý do gì để quay lại hỏi.
        """
        out: list[ChunkRow] = []
        for doc in self.store.documents():
            offset = 0
            while True:
                page = self.store.chunks_of(doc.id, offset, 1000)
                if not page:
                    break
                out.extend(page)
                offset += len(page)
        return out

    # -- xoá ---------------------------------------------------------------------------

    def remove(self, doc_id: str) -> bool:
        """Bỏ một tài liệu khỏi thư viện. **Không** xoá tệp của người dùng.

        Đánh dấu loại trừ trước khi xoá: đường dẫn chỉ đọc được từ hàng `documents`, mà
        hàng đó sắp biến mất. Thiếu bước này thì lần quét kế tiếp nạp lại đúng tài liệu
        vừa bị bỏ.
        """
        row = self.store.document(doc_id)
        if row is not None:
            self.store.exclude(row.path, int(time.time() * 1000))
        removed = self.store.remove_document(doc_id)
        try:
            self.vectors.remove_document(doc_id)
        except VectorStoreError as err:
            # Vector mồ côi vô hại: phép tìm ánh xạ mã điểm về hàng `chunks`, và mã không
            # còn hàng nào thì rơi ra khỏi kết quả. Đừng để Qdrant chết chặn một nút bấm.
            log.debug("chưa dọn được vector của %s: %s", doc_id, err)
        return bool(removed)

    def stats(self) -> dict[str, object]:
        docs, chunks = self.store.counts()
        # `count()` ném khi không hỏi được, nên `reachable` ở đây nói đúng sự thật thay vì
        # luôn luôn `True` — xem `VectorStore.count`.
        try:
            vectors = self.vectors.count()
            reachable = True
        except VectorStoreError as err:
            vectors, reachable = 0, False
            log.debug("Qdrant không với tới được: %s", err)
        def number(key: str) -> int:
            raw = self.store.meta(key)
            return int(raw) if raw and raw.isdigit() else 0

        return {
            "project": self.project.id,
            "root": str(self.root),
            "documents": docs,
            "chunks": chunks,
            "vectors": vectors,
            "qdrant_reachable": reachable,
            "embedder": self.config.embedding.model,
            "failures": [
                {"path": path, "reason": reason} for path, reason in self.store.failures()
            ],
            "excluded": len(self.store.excluded()),
            "files_seen": number(store_meta.META_SCAN_FILES),
            "files_skipped": number(store_meta.META_SCAN_SKIPPED),
            # `None` chứ không phải "bây giờ": chưa quét lần nào là một trạng thái thật,
            # và bịa ra một mốc thời gian ở đây làm giao diện nói dối.
            "scanned_at": number(store_meta.META_SCAN_AT) or None,
        }


def _batched(items: list, size: int) -> Iterator[list]:
    for start in range(0, len(items), size):
        yield items[start : start + size]

