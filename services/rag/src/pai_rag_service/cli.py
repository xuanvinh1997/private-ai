"""Service command line. `serve` is what the app calls; the rest exist to debug one
stage at a time without the app, and `doctor` touches every external dependency in
dependency order so a new machine reports what is missing all at once."""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import sys

from pai_rag_service.config import load
from pai_rag_service.errors import RagError
from pai_rag_service.service import Service

__all__ = ["main"]


def _out(value: object) -> None:
    print(json.dumps(value, ensure_ascii=False, indent=2, default=str))


async def _sync(args: argparse.Namespace) -> int:
    service = Service()
    report = await service.pipeline(args.project).sync()
    _out(report.as_dict())
    return 0 if not report.failed else 1


async def _search(args: argparse.Namespace) -> int:
    service = Service()
    retriever = service.retriever(args.project)
    if args.strategy == "keyword":
        hits = retriever.keyword(args.query, args.limit)
    elif args.strategy == "vector":
        hits = await retriever.vector(args.query, args.limit)
    else:
        hits = await retriever.hybrid(args.query, args.limit)
    if args.json:
        _out([hit.as_dict() for hit in hits])
    else:
        for hit in hits:
            print(f"\n{'=' * 70}\n{hit.score:7.2f} [{hit.matched_by}]  {hit.render()}")
    return 0


async def _docs(args: argparse.Namespace) -> int:
    rows = Service().pipeline(args.project).store.documents()
    _out(
        [
            {
                "id": row.id,
                "title": row.title,
                "format": row.format,
                "chunks": row.chunks,
                "pages": row.pages,
                "ocrPages": row.ocr_pages,
                "path": row.path,
            }
            for row in rows
        ]
    )
    return 0


async def _stats(args: argparse.Namespace) -> int:
    _out(Service().pipeline(args.project).stats())
    return 0


async def _doctor(args: argparse.Namespace) -> int:
    """Touch every external dependency in dependency order; non-zero exit when something is broken."""
    from pai_rag_service.embed import embedder_for
    from pai_rag_service.vectors import VectorStore

    ok = True
    config = load()
    print(f"cấu hình      : {'PAI_RAG_CONFIG' if config.projects else 'mặc định trong mã'}")
    print(f"dự án         : {[p.id for p in config.projects] or '(chưa có)'}")

    # 1. Embedding server. Nothing else matters if this is down.
    try:
        vector = await embedder_for(config.embedding).aembed_query("kiểm tra")
        print(f"nhúng         : OK  {config.embedding.model} → {len(vector)} chiều")
    except Exception as err:
        ok = False
        print(f"nhúng         : HỎNG  {err}")

    # 2. Qdrant.
    try:
        store = VectorStore(config.vectors, "pai_doctor_probe")
        print(f"qdrant        : {'OK' if store.health() else 'HỎNG'}  {config.vectors.url}")
        ok = ok and store.health()
    except Exception as err:
        ok = False
        print(f"qdrant        : HỎNG  {err}")

    # 3. Graph - optional, so unavailable is a warning rather than a failure. Probed through a real
    # project when there is one: an embedded store is a directory per project, and the only useful
    # question is whether *that* directory opens.
    if config.graph.enabled:
        try:
            from pai_rag_service.graph import GraphStore

            probe = config.projects[0] if config.projects else None
            url = config.graph_url(probe) if probe else (config.graph.url or "surrealkv://<dự án>")
            if probe is None:
                print(f"graph         : chưa có dự án để mở  {url}")
            else:
                graph = GraphStore(config.graph, url, config.graph_database(probe))
                entities, relations = graph.count()
                graph.close()
                print(f"graph         : OK  {url} → {entities} thực thể, {relations} quan hệ")
        except Exception as err:
            print(f"graph         : chưa sẵn sàng ({err}) — chiến lược graph sẽ vắng mặt")
    else:
        print("graph         : tắt trong cấu hình")

    # 4. Reranker. This downloads the model on first run, which is the main reason `doctor` exists: pay the 2 GB in a command the user chose to run.
    if config.rerank.enabled:
        try:
            from pai_rag_service.rerank import build

            reranker = build(config.rerank)
            scores = reranker.score("thử", ["một đoạn văn bản để kiểm tra"])
            print(f"rerank        : OK  {reranker.id} → {scores[0]:.2f}")
        except Exception as err:
            print(f"rerank        : chưa sẵn sàng ({err}) — truy hồi vẫn chạy, chỉ kém hơn")
    else:
        print("rerank        : tắt trong cấu hình")

    return 0 if ok else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="pai-rag", description="Tầng RAG của Private AI: nạp, truy hồi, và nói MCP."
    )
    parser.add_argument(
        "-v", "--verbose", action="store_true", help="log chi tiết ra stderr"
    )
    sub = parser.add_subparsers(dest="command", required=True)

    serve = sub.add_parser("serve", help="chạy MCP server trên stdio (ứng dụng gọi lệnh này)")
    serve.set_defaults(handler=None)

    def with_project(p: argparse.ArgumentParser) -> argparse.ArgumentParser:
        p.add_argument("--project", default="", help="mã dự án; bỏ trống là dự án đang mở")
        return p

    with_project(sub.add_parser("sync", help="quét thư mục dự án và nạp tệp mới")).set_defaults(
        handler=_sync
    )
    with_project(sub.add_parser("docs", help="liệt kê tài liệu")).set_defaults(handler=_docs)
    with_project(sub.add_parser("stats", help="sức khoẻ thư viện")).set_defaults(handler=_stats)

    search = with_project(sub.add_parser("search", help="tìm thử một câu hỏi"))
    search.add_argument("query")
    search.add_argument("-n", "--limit", type=int, default=5)
    search.add_argument(
        "-s", "--strategy", default="hybrid", choices=["hybrid", "vector", "keyword"]
    )
    search.add_argument("--json", action="store_true")
    search.set_defaults(handler=_search)

    sub.add_parser("doctor", help="kiểm tra mọi phụ thuộc ngoài").set_defaults(handler=_doctor)

    args = parser.parse_args(argv)

    # `serve` handles its own logging: stdout is the JSON-RPC channel and must stay clean.
    if args.command == "serve":
        from pai_rag_service.mcp_server import run

        run()
        return 0

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.WARNING,
        format="%(levelname)s %(name)s: %(message)s",
        stream=sys.stderr,
    )
    try:
        return asyncio.run(args.handler(args))
    except RagError as err:
        # Errors from this layer are already actionable; print them plainly, since a traceback would bury the sentence that matters.
        print(f"lỗi: {err}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
