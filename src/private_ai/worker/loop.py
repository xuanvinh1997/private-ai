"""The ingestion process.

Reading a document is CPU-bound Python from end to end -- markitdown and pypdf parse in
pure Python, the splitters chunk with tiktoken, the graph store merges a networkx graph and
rewrites its KV store -- so it holds the GIL for as long as the file takes. Sharing a
process with the desktop app therefore means sharing a stalled event loop: uploads, chat
and even a plain SQLite read queue behind whatever file is being read, and Qt stops
repainting. ``asyncio.to_thread`` does not help, because the GIL is what is contended, not
a thread.

This process owns that work instead. It takes documents off the queue the app writes to,
using the same ``document_claims`` rows that already keep two processes from ingesting the
same document, and the app keeps its loop for the UI.
"""

from __future__ import annotations

import asyncio
import logging
import signal
import sqlite3
import sys
from contextlib import suppress

from private_ai.config import Settings, get_settings
from private_ai.core.bootstrap import build_services, close_services

logger = logging.getLogger("private_ai.worker")


def _pending_count(services) -> int:  # noqa: ANN001 - AppServices, kept loose for testing
    """Documents that still need extraction or an index, by the queue's own definition."""
    row = services.database.fetch_one(
        """
        SELECT COUNT(*) AS pending FROM documents d
        WHERE d.status IN ('queued', 'extracted', 'processing')
           OR (
                d.status = 'ready'
                AND d.extracted_text IS NOT NULL
                AND (
                      d.indexed_at IS NULL
                      OR (d.index_mode = 'simple' AND EXISTS (
                              SELECT 1 FROM document_chunks c
                              WHERE c.document_id = d.id
                                AND COALESCE(c.embedding_vector, c.embedding_json) IS NULL
                      ))
                )
           )
        """
    )
    return int(row["pending"]) if row else 0


def _schema_exists(settings: Settings) -> bool:
    if not settings.database_path.exists():
        return False
    try:
        connection = sqlite3.connect(settings.database_path, timeout=5)
    except sqlite3.Error:
        return False
    try:
        return (
            connection.execute(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'documents'"
            ).fetchone()
            is not None
        )
    except sqlite3.Error:
        return False
    finally:
        connection.close()


async def wait_for_schema(settings: Settings, stop: asyncio.Event) -> bool:
    """Block until the app has created the tables, or until asked to stop.

    Whoever starts first is not ours to control -- the desktop app spawns this process while
    still migrating -- so the worker waits rather than racing the migrations or crashing on
    a missing table.
    """
    while not stop.is_set():
        if await asyncio.to_thread(_schema_exists, settings):
            return True
        with suppress(TimeoutError):
            await asyncio.wait_for(stop.wait(), timeout=settings.worker_poll_seconds)
    return False


async def run_worker(settings: Settings, stop: asyncio.Event) -> None:
    """Drain the ingestion queue until asked to stop."""
    if not await wait_for_schema(settings, stop):
        return
    # The app owns the schema: it is the process the user starts and the one whose
    # migrations delete purged folders. Creating the tables from here as well would race it.
    services = build_services(settings, migrate=False)
    logger.info("Ingestion worker attached to %s", settings.database_path)
    try:
        await services.ingestion.process_pending()
        await services.memory.sync_all()
        while not stop.is_set():
            try:
                await asyncio.wait_for(stop.wait(), timeout=settings.worker_poll_seconds)
                break
            except TimeoutError:
                pass
            # Polling reads one indexed COUNT; the expensive sweep only runs when the queue
            # is not empty, so an idle worker costs a query every couple of seconds.
            if await asyncio.to_thread(_pending_count, services):
                await services.ingestion.process_pending(recover=False)
    except asyncio.CancelledError:
        raise
    except Exception:
        logger.exception("Ingestion worker stopped on an unhandled error")
        raise
    finally:
        await close_services(services)


async def _main(settings: Settings) -> None:
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for number in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(number, stop.set)
        except (NotImplementedError, RuntimeError):
            # Windows has no loop-level signal handling; the launcher's job object and
            # KeyboardInterrupt cover shutdown there.
            signal.signal(number, lambda *_: stop.set())
    await run_worker(settings, stop)


def run() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
        stream=sys.stderr,
    )
    with suppress(KeyboardInterrupt):
        asyncio.run(_main(get_settings()))


if __name__ == "__main__":
    run()
