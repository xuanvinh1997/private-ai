"""Ingest: scan folder -> extract -> chunk -> embed -> write store. Three invariants: a
broken file only breaks itself, an unchanged folder re-extracts nothing (mtime + size),
and embedding may fail. Extractor, embed-input and model identity trigger a rebuild."""

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
from pai_rag_service.errors import (
    EmbedError,
    ExtractError,
    GraphError,
    RagError,
    VectorStoreError,
)
from pai_rag_service.extract import (
    EXTRACT_VERSION,
    SUPPORTED_EXTENSIONS,
    extract,
)
from pai_rag_service.graph import GraphStore
from pai_rag_service.store import ChunkRow, Store
from pai_rag_service.vectors import VectorStore

__all__ = ["MAX_FILES", "Pipeline", "SyncReport", "scan"]

log = logging.getLogger(__name__)

#: How many files one scan will ingest; pointing at a ten-thousand-file Downloads folder must say it hit the cap rather than stall silently.
MAX_FILES = 5_000

#: Directories that never hold user documents, only machine output; scanning them would ingest thousands of third-party source files.
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
    """Result of one sync, enough for the UI to say what happened."""

    scanned: int = 0
    ingested: int = 0
    skipped_unchanged: int = 0
    failed: list[tuple[str, str]] = field(default_factory=list)
    embedded_chunks: int = 0
    #: Files skipped because :data:`MAX_FILES` was reached.
    over_limit: int = 0
    #: Files still in the folder that the user removed from the library.
    excluded: int = 0
    #: Why the semantic half is not ready, when it is not.
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
    """Readable files in the project folder, plus how many were dropped at the cap."""
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
    """Walk the tree, skipping machine-generated directories and hidden files."""
    stack = [root]
    while stack:
        current = stack.pop()
        try:
            entries = list(current.iterdir())
        except (PermissionError, OSError) as err:
            # An unreadable directory must not break the whole scan.
            log.debug("skipping directory %s: %s", current, err)
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
    """Stable document id from the relative path; hashed because it goes into graph labels and Qdrant payload keys, and relative so moving the project folder keeps every id."""
    try:
        rel = path.relative_to(root).as_posix()
    except ValueError:
        rel = path.as_posix()
    return hashlib.sha1(rel.encode("utf-8")).hexdigest()[:16]


class Pipeline:
    """Ingests and keeps one project's library in sync."""

    def __init__(self, config: RagConfig, project: ProjectConfig) -> None:
        self.config = config
        self.project = project
        self.root = project.local_root()
        self.store = Store(config.store_path(project))
        self.splitter = SectionAwareSplitter(
            chunk_size=config.chunk.size, chunk_overlap=config.chunk.overlap
        )
        self.vectors = VectorStore(config.vectors, config.collection(project))
        # Built even when the store is unreachable: `GraphStore` connects on first use, so a graph
        # that is down costs nothing until something asks it a question.
        self.graph = (
            GraphStore(config.graph, config.graph_url(project), config.graph_database(project))
            if config.graph.enabled
            else None
        )
        self._embedder = None

    def close(self) -> None:
        self.store.close()
        if self.graph is not None:
            self.graph.close()

    def __enter__(self) -> Pipeline:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- identity --------------------------------------------------------------------------

    @property
    def embedder(self):
        if self._embedder is None:
            self._embedder = embedder_for(self.config.embedding)
        return self._embedder

    def reconcile(self) -> bool:
        """Compare running identity with stored identity and clean up on drift; `True` when something must be redone."""
        seen = self.store.identity()
        model = self.config.embedding.model
        stale_extract = seen["extract"] != str(EXTRACT_VERSION)
        stale_input = seen["embed_input"] != str(EMBED_INPUT_VERSION)
        stale_model = seen["embedder"] not in (None, model)

        if stale_extract or stale_input:
            # How files are read or how embed input is built has changed: everything goes again, untouched files included.
            count = self.store.forget_fingerprints()
            log.info(
                "extractor or embed input changed - re-reading %d documents", count
            )
        self.store.set_identity(
            embedder=model,
            dim=self.config.embedding.dim,
            embed_input=EMBED_INPUT_VERSION,
            extract=EXTRACT_VERSION,
        )
        return stale_extract or stale_input or stale_model

    # -- ingest ----------------------------------------------------------------------------

    async def sync(self) -> SyncReport:
        """Catch up with the project folder. Safe to call any number of times."""
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
        # Files the user removed from the library are still on disk, so every scan sees them; without this filter `remove` would be a button that does nothing.
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
                # Invariant 1: record it and move on. The fingerprint goes into `failures` too, because the file will fail identically until the user edits it - and editing changes `mtime`.
                self.store.put_failure(str(path), fingerprint[0], fingerprint[1], err.reason)
                report.failed.append((str(path), err.reason))
            except RagError as err:
                # Infrastructure errors do *not* go into `failures`: fingerprinting a healthy file would make the next scan see it "unchanged" and skip it forever.
                report.failed.append((str(path), str(err)))

        # Embed last, once for the whole batch: large batches are far cheaper than per-document calls, and this also sweeps up chunks owed from earlier runs.
        try:
            report.embedded_chunks = await self.embed_pending()
        except (EmbedError, VectorStoreError) as err:
            # Invariant 3: a failed embedding does not fail the ingest.
            report.embed_error = str(err)
            log.warning("embedding not done yet: %s", err)

        # Record this scan; the UI reads these three numbers to describe the last scan before the session's own first scan runs.
        self.store.set_meta(store_meta.META_SCAN_FILES, str(report.scanned))
        self.store.set_meta(store_meta.META_SCAN_SKIPPED, str(report.over_limit))
        self.store.set_meta(store_meta.META_SCAN_AT, str(int(time.time() * 1000)))
        return report

    async def ingest(self, path: Path) -> str:
        """Extract, chunk and write one file into the store. Returns the document id."""
        got = await extract(path, vision=self.config.vision, ocr=self.config.ocr)
        chunks = self.splitter.split(got.text)
        if not chunks:
            raise ExtractError(str(path), "đọc được tệp nhưng không cắt ra đoạn nào")

        stat = path.stat()
        doc_id = document_id(self.root, path)
        # Clear this document's old vectors, but never let that block the ingest: orphan vectors are harmless, while letting the error escape would break invariant 3.
        try:
            self.vectors.remove_document(doc_id)
        except VectorStoreError as err:
            log.debug("could not clear old vectors for %s: %s", path.name, err)
        self._forget_graph(doc_id)
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
            log.info("%s: read %d pages via OCR", path.name, len(got.ocr_pages))
        return doc_id

    async def embed_pending(self) -> int:
        """Embed every chunk without a vector; the path a library ingested while Ollama was down takes to catch up. Returns how many were embedded."""
        rows = self._all_chunks()
        if not rows:
            return 0

        model = self.config.embedding.model
        # The dimension is only knowable by embedding one chunk - many servers publish it nowhere - so probe first, then create the collection.
        probe = await self.embedder.aembed_documents(
            [embedding_text_for(rows[0].section, rows[0].body)]
        )
        if not probe or not probe[0]:
            raise EmbedError(f"model `{model}` trả về vector rỗng")
        dim = len(probe[0])
        rebuilt = self.vectors.ensure(dim=dim, model=model, input_version=EMBED_INPUT_VERSION)

        # A freshly rebuilt collection holds no points; asking Qdrant just to get an empty set is a wasted round trip.
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
        """Every chunk in the store with all the fields needed for both embed text and payload, read in one pass from SQLite."""
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

    # -- removal ---------------------------------------------------------------------------

    def remove(self, doc_id: str) -> bool:
        """Drop a document from the library without deleting the user's file; exclusion is marked first, because the path is only readable from the row about to vanish."""
        row = self.store.document(doc_id)
        if row is not None:
            self.store.exclude(row.path, int(time.time() * 1000))
        removed = self.store.remove_document(doc_id)
        try:
            self.vectors.remove_document(doc_id)
        except VectorStoreError as err:
            # Orphan vectors are harmless - unmatched ids drop out of results - so a dead Qdrant must not block a button.
            log.debug("could not clear vectors for %s: %s", doc_id, err)
        self._forget_graph(doc_id)
        return bool(removed)

    def _forget_graph(self, doc_id: str) -> None:
        """Clear a document's entities and edges. Same rule as vectors: the graph is an extra
        strategy, so a store that is down must never block an ingest or a delete button."""
        if self.graph is None:
            return
        try:
            self.graph.remove_document(doc_id)
        except GraphError as err:
            log.debug("could not clear the graph for %s: %s", doc_id, err)

    def stats(self) -> dict[str, object]:
        docs, chunks = self.store.counts()
        # `count()` raises when it cannot ask, so `reachable` here states the truth instead of always being `True`.
        try:
            vectors = self.vectors.count()
            reachable = True
        except VectorStoreError as err:
            vectors, reachable = 0, False
            log.debug("Qdrant unreachable: %s", err)
        entities, relations, graph_reachable = 0, 0, False
        if self.graph is not None:
            try:
                entities, relations = self.graph.count()
                graph_reachable = True
            except GraphError as err:
                log.debug("graph unreachable: %s", err)

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
            "entities": entities,
            "relations": relations,
            "graph_reachable": graph_reachable,
            "embedder": self.config.embedding.model,
            "failures": [
                {"path": path, "reason": reason} for path, reason in self.store.failures()
            ],
            "excluded": len(self.store.excluded()),
            "files_seen": number(store_meta.META_SCAN_FILES),
            "files_skipped": number(store_meta.META_SCAN_SKIPPED),
            # `None`, not "now": never scanned is a real state, and inventing a timestamp makes the UI lie.
            "scanned_at": number(store_meta.META_SCAN_AT) or None,
        }


def _batched(items: list, size: int) -> Iterator[list]:
    for start in range(0, len(items), size):
        yield items[start : start + size]

