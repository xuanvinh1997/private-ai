"""Retrieval strategies: the fusion maths, the router, and the shared post-processing."""

from __future__ import annotations

import pytest
from conftest import insert_document
from langchain_core.documents import Document

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.core.services import AppServices
from private_ai.rag.stores.sqlite_vectorstore import SqliteVectorStore
from private_ai.rag.strategies.auto import AutoStrategy
from private_ai.rag.strategies.base import (
    MAX_RESULTS,
    RRF_K,
    deduplicate,
    identity,
    reciprocal_rank_fusion,
    stamp,
)
from private_ai.rag.strategies.hybrid import HybridStrategy
from private_ai.rag.strategies.registry import StrategyRegistry
from private_ai.rag.strategies.web import WebStrategy
from private_ai.rag.web_search import WebSearchResponse, WebSearchResult, WebSearchUnavailable


def _document(text: str, chunk_id: str = "", filename: str = "f.txt") -> Document:
    metadata = {"filename": filename}
    if chunk_id:
        metadata["chunk_id"] = chunk_id
    return Document(page_content=text, metadata=metadata)


# --- fusion ---------------------------------------------------------------


def test_reciprocal_rank_fusion_sums_one_over_k_plus_rank() -> None:
    first = _document("a", "a")
    second = _document("b", "b")
    third = _document("c", "c")

    fused = reciprocal_rank_fusion([[first, second], [second, third]])

    # b appears at rank 2 and rank 1; a only at rank 1; c only at rank 2.
    assert [document.metadata["chunk_id"] for document in fused] == ["b", "a", "c"]
    assert fused[0].metadata["score"] == pytest.approx(1 / (RRF_K + 2) + 1 / (RRF_K + 1))
    assert fused[1].metadata["score"] == pytest.approx(1 / (RRF_K + 1))
    assert fused[2].metadata["score"] == pytest.approx(1 / (RRF_K + 2))


def test_fusion_ignores_the_incoming_scores_entirely() -> None:
    """Rank, not score: a cosine similarity and a keyword hit count are incomparable."""
    loud = _document("loud", "loud")
    loud.metadata["score"] = 1_000_000.0
    quiet = _document("quiet", "quiet")
    quiet.metadata["score"] = 0.001

    fused = reciprocal_rank_fusion([[quiet, loud]])

    assert [document.metadata["chunk_id"] for document in fused] == ["quiet", "loud"]
    assert fused[0].metadata["score"] == pytest.approx(1 / (RRF_K + 1))


def test_fusion_is_stable_for_an_identical_corpus() -> None:
    rankings = [[_document("a", "a"), _document("b", "b")], [_document("b", "b")]]
    once = [document.metadata["chunk_id"] for document in reciprocal_rank_fusion(rankings)]
    twice = [document.metadata["chunk_id"] for document in reciprocal_rank_fusion(rankings)]
    assert once == twice == ["b", "a"]


def test_identity_prefers_the_chunk_id_and_falls_back_to_a_content_digest() -> None:
    assert identity(_document("bất kỳ", "chunk-7")) == "chunk-7"
    same = identity(Document(page_content="cùng nội dung", metadata={"document_id": "d"}))
    again = identity(Document(page_content="cùng nội dung", metadata={"document_id": "d"}))
    other = identity(Document(page_content="khác", metadata={"document_id": "d"}))
    assert same == again != other


# --- deduplication --------------------------------------------------------


def test_deduplicate_keeps_the_first_of_each_repeated_passage() -> None:
    """Re-ingesting a document files the same passage under a second chunk id."""
    first = _document("cùng một đoạn", "chunk-old")
    second = _document("cùng một đoạn", "chunk-new")
    third = _document("đoạn khác", "chunk-3")

    kept = deduplicate([first, second, third])

    assert [document.metadata["chunk_id"] for document in kept] == ["chunk-old", "chunk-3"]


def test_deduplicate_treats_the_same_text_in_two_files_as_two_results() -> None:
    left = _document("giống hệt", "1", filename="a.txt")
    right = _document("giống hệt", "2", filename="b.txt")
    assert len(deduplicate([left, right])) == 2


def test_deduplicate_never_returns_more_than_the_prompt_budget_allows() -> None:
    many = [_document(f"đoạn {index}", f"c{index}") for index in range(50)]
    assert len(deduplicate(many)) == MAX_RESULTS
    assert len(deduplicate(many, 3)) == 3
    # A caller asking for more than the cap still gets the cap.
    assert len(deduplicate(many, 999)) == MAX_RESULTS


def test_stamp_fills_in_every_key_a_citation_needs() -> None:
    bare = Document(page_content="nội dung", metadata={})
    stamped = stamp([bare], "vector")[0]
    assert stamped.metadata["strategy"] == "vector"
    assert stamped.metadata["document_id"] == ""
    assert stamped.metadata["filename"] == ""
    assert stamped.metadata["chunk_id"] == "vector:0"
    assert stamped.metadata["score"] == 0.0


# --- the router -----------------------------------------------------------


@pytest.fixture
def auto(services: AppServices) -> AutoStrategy:
    return AutoStrategy(services)


@pytest.mark.parametrize(
    ("query", "expected"),
    [
        # 1. A whole-document summary: a summary verb plus a document-scope word.
        ("Tóm tắt toàn bộ tài liệu này giúp tôi", RetrievalStrategyName.SUMMARY),
        # 2. A question about how two things stand to each other.
        ("Mối liên hệ giữa Nguyễn Văn A và Trần Thị B là gì?", RetrievalStrategyName.GRAPH),
        # 3. Something that has to match letter for letter.
        ('Tìm câu "an toàn thực phẩm" nằm ở đâu', RetrievalStrategyName.KEYWORD),
        ("Nghị định 15/2024 quy định gì?", RetrievalStrategyName.KEYWORD),
        # 4. Anything else: we do not know whether wording or meaning decides it.
        ("làm sao để cải thiện hiệu suất", RetrievalStrategyName.HYBRID),
    ],
)
def test_auto_routes_the_four_query_shapes(
    auto: AutoStrategy,
    query: str,
    expected: RetrievalStrategyName,
) -> None:
    name, reason = auto.classify(query)
    assert name == expected.value
    assert reason


def test_two_proper_nouns_are_enough_to_reach_the_graph(auto: AutoStrategy) -> None:
    assert auto.classify("Vai trò của Minh trong dự án Apollo")[0] == "graph"
    # One is not: a single name is a lookup, not a relationship.
    assert auto.classify("kể về dự án Apollo")[0] != "graph"


def test_explain_names_the_choice_and_the_reason(auto: AutoStrategy) -> None:
    explanation = auto.explain("Nghị định 15/2024 quy định gì?")
    assert explanation.startswith("auto → keyword:")
    assert explanation.endswith(".")


def test_routing_is_deterministic(auto: AutoStrategy) -> None:
    """The same question routing two ways would make a bad answer impossible to explain."""
    query = "Quan hệ giữa Alpha và Beta"
    assert {auto.classify(query) for _ in range(5)} == {auto.classify(query)}


async def test_auto_marks_what_routed_a_result(
    services: AppServices,
    workspace_id: str,
    auto: AutoStrategy,
) -> None:
    document_id = insert_document(services.database, workspace_id, "huong-dan.txt")
    await services.vectors.scoped(workspace_id).aadd_texts(
        ["cách cải thiện hiệu suất của hệ thống"],
        [{"document_id": document_id}],
    )

    found = await auto.retrieve("làm sao để cải thiện hiệu suất", workspace_id=workspace_id)

    assert found
    for document in found:
        # The concrete strategy is kept so a citation still says how it was found.
        assert document.metadata["strategy"] == "hybrid"
        assert document.metadata["routed_by"] == "auto"
        assert document.metadata["routing_reason"]


async def test_an_empty_query_retrieves_nothing(auto: AutoStrategy, workspace_id: str) -> None:
    assert await auto.retrieve("   ", workspace_id=workspace_id) == []


# --- hybrid ---------------------------------------------------------------


async def test_hybrid_fuses_both_arms_and_labels_their_scores(
    services: AppServices,
    workspace_id: str,
) -> None:
    document_id = insert_document(services.database, workspace_id, "xe.txt")
    await services.vectors.scoped(workspace_id).aadd_texts(
        ["chiếc xe hơi màu đỏ", "công thức nấu súp"],
        [{"document_id": document_id}, {"document_id": document_id}],
    )

    found = await HybridStrategy(services).retrieve("chiếc xe hơi", workspace_id=workspace_id)

    assert found
    assert found[0].page_content == "chiếc xe hơi màu đỏ"
    assert found[0].metadata["strategy"] == "hybrid"
    assert "vector_score" in found[0].metadata or "keyword_score" in found[0].metadata


async def test_hybrid_survives_one_arm_failing(
    services: AppServices,
    workspace_id: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """No embedding model should cost us the dense arm, not the whole answer."""
    document_id = insert_document(services.database, workspace_id, "xe.txt")
    await services.vectors.scoped(workspace_id).aadd_texts(
        ["chiếc xe hơi màu đỏ"],
        [{"document_id": document_id}],
    )

    async def broken(*args: object, **kwargs: object) -> list[object]:
        raise RuntimeError("nhà cung cấp nhúng ngoại tuyến")

    monkeypatch.setattr(SqliteVectorStore, "asimilarity_search_with_score", broken)

    found = await HybridStrategy(services).retrieve("chiếc xe hơi", workspace_id=workspace_id)
    assert [document.page_content for document in found] == ["chiếc xe hơi màu đỏ"]


async def test_hybrid_raises_only_when_both_arms_fail(
    services: AppServices,
    workspace_id: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    async def broken(*args: object, **kwargs: object) -> list[object]:
        raise RuntimeError("cả hai nhánh hỏng")

    monkeypatch.setattr(SqliteVectorStore, "asimilarity_search_with_score", broken)
    monkeypatch.setattr(SqliteVectorStore, "akeyword_search", broken)

    with pytest.raises(RuntimeError):
        await HybridStrategy(services).retrieve("bất kỳ", workspace_id=workspace_id)


# --- web ------------------------------------------------------------------


async def test_web_degrades_to_an_empty_list_with_a_notice(
    services: AppServices,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A search host that is off is an ordinary state of the world, not a failed turn."""

    async def unavailable(*args: object, **kwargs: object) -> WebSearchResponse:
        raise WebSearchUnavailable("Không kết nối được DuckDuckGo")

    monkeypatch.setattr(services.web_search, "search", unavailable)
    strategy = WebStrategy(services)

    assert await strategy.retrieve("tin mới nhất", workspace_id="") == []
    outcome = await strategy.search("tin mới nhất")
    assert outcome.documents == []
    assert "DuckDuckGo" in outcome.notice
    # The framing survives even when nothing came back, because it is what the agent reads.
    assert "không đáng tin cậy" in outcome.framing


async def test_an_empty_web_query_is_refused_rather_than_sent(
    services: AppServices,
) -> None:
    outcome = await WebStrategy(services).search("   ")
    assert outcome.documents == []
    assert outcome.notice


async def test_web_results_become_citable_documents(
    services: AppServices,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    async def canned(query: str, limit: int = 0) -> WebSearchResponse:
        return WebSearchResponse(
            query=query,
            backend="duckduckgo",
            results=[
                WebSearchResult(
                    title="Tiêu đề",
                    url="https://example.com/a",
                    snippet="đoạn trích",
                    engine="ddg",
                ),
                WebSearchResult(title="Khác", url="https://example.com/b", snippet="khác"),
            ],
        )

    monkeypatch.setattr(services.web_search, "search", canned)

    found = await WebStrategy(services).retrieve("tin mới nhất", limit=5)

    assert [document.metadata["url"] for document in found] == [
        "https://example.com/a",
        "https://example.com/b",
    ]
    assert found[0].metadata["strategy"] == "web"
    assert found[0].metadata["untrusted"] is True
    assert "https://example.com/a" in found[0].page_content
    # Earlier results score higher, so the fused ordering has something to work with.
    assert found[0].metadata["score"] > found[1].metadata["score"]


# --- registry -------------------------------------------------------------


def test_the_registry_knows_all_seven_strategies(services: AppServices) -> None:
    registry = StrategyRegistry(services)
    assert set(registry.names()) == {name.value for name in RetrievalStrategyName}
    assert len(registry.names()) == 7
    assert {strategy.name for strategy in registry.all()} == set(registry.names())
    # Every description is what a model reads to choose; none may be blank.
    assert all(strategy.description.strip() for strategy in registry.all())


def test_the_registry_hands_back_the_same_instance(services: AppServices) -> None:
    registry = StrategyRegistry(services)
    assert registry.get("vector") is registry.get(RetrievalStrategyName.VECTOR)


def test_an_unknown_strategy_name_lists_the_valid_ones(services: AppServices) -> None:
    registry = StrategyRegistry(services)
    with pytest.raises(KeyError) as raised:
        registry.get("telepathy")
    assert "hybrid" in str(raised.value)


async def test_the_registry_defaults_to_auto(
    services: AppServices,
    workspace_id: str,
) -> None:
    registry = StrategyRegistry(services)
    document_id = insert_document(services.database, workspace_id, "a.txt")
    await services.vectors.scoped(workspace_id).aadd_texts(
        ["cách cải thiện hiệu suất"],
        [{"document_id": document_id}],
    )

    found = await registry.retrieve(
        "làm sao để cải thiện hiệu suất",
        workspace_id=workspace_id,
        strategy="",
    )
    assert found
    assert found[0].metadata["routed_by"] == "auto"


async def test_a_retriever_wrapping_a_strategy_is_async_only(
    services: AppServices,
    workspace_id: str,
) -> None:
    """Every store here is async; a blocking retriever would deadlock the shared loop."""
    retriever = StrategyRegistry(services).get("keyword").as_retriever(workspace_id=workspace_id)
    with pytest.raises(NotImplementedError, match="async-only"):
        retriever.invoke("bất kỳ")
    assert await retriever.ainvoke("bất kỳ") == []
