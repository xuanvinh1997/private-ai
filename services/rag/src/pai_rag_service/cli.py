"""Dòng lệnh của service.

``serve`` là thứ ứng dụng gọi; mọi lệnh còn lại tồn tại để **gỡ lỗi mà không cần dựng cả
ứng dụng**. Đó không phải tiện lợi thừa: khi một câu hỏi trả về kết quả sai, câu đầu tiên
phải trả lời là "sai từ khâu nào" — rút chữ, cắt đoạn, nhúng, hay xếp hạng — và chạy được
từng khâu một từ terminal là cách duy nhất trả lời nhanh.

``doctor`` là lệnh nên chạy đầu tiên trên một máy mới: nó chạm vào mọi phụ thuộc ngoài
theo đúng thứ tự phụ thuộc và nói ra cái nào chưa sẵn sàng, thay vì để người dùng phát
hiện ra từng cái một qua các lỗi rời rạc.
"""

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
    """Chạm vào mọi phụ thuộc ngoài, theo đúng thứ tự phụ thuộc.

    Trả về mã thoát khác 0 khi có thứ hỏng, để dùng được trong script.
    """
    from pai_rag_service.embed import embedder_for
    from pai_rag_service.vectors import VectorStore

    ok = True
    config = load()
    print(f"cấu hình      : {'PAI_RAG_CONFIG' if config.projects else 'mặc định trong mã'}")
    print(f"dự án         : {[p.id for p in config.projects] or '(chưa có)'}")

    # 1. Máy chủ nhúng. Mọi thứ khác vô nghĩa nếu cái này không chạy.
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

    # 3. Neo4j — tắt được, nên không sẵn sàng thì cảnh báo chứ không phải lỗi.
    if config.graph.enabled:
        try:
            from neo4j import GraphDatabase

            driver = GraphDatabase.driver(
                config.graph.uri, auth=(config.graph.user, config.graph.password)
            )
            driver.verify_connectivity()
            driver.close()
            print(f"neo4j         : OK  {config.graph.uri}")
        except Exception as err:
            print(f"neo4j         : chưa sẵn sàng ({err}) — chiến lược graph sẽ vắng mặt")
    else:
        print("neo4j         : tắt trong cấu hình")

    # 4. Reranker. Bước này **tải model** ở lần đầu, nên nó là lý do chính `doctor` tồn
    #    tại: trả 2 GB tải về vào một lệnh người dùng chủ động chạy, thay vì vào lần tìm
    #    kiếm đầu tiên của họ.
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

    # `serve` tự lo phần log của nó: stdout là đường JSON-RPC và không được lẫn gì vào.
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
        # Lỗi của tầng này đã được viết để người đọc hành động được — in thẳng, không kèm
        # traceback, vì traceback ở đây chỉ che mất câu cần đọc.
        print(f"lỗi: {err}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
