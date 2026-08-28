from __future__ import annotations

from pathlib import Path
from typing import Any

import httpx
import pytest
from fastapi.testclient import TestClient
from test_workspaces import FakeOllama

from private_ai_api.config import Settings
from private_ai_api.mcp_server import create_mcp_server
from private_ai_api.services.web_search import (
    DUCKDUCKGO,
    OPENAI,
    SEARXNG,
    WebSearchConfig,
    WebSearchNotConfigured,
    WebSearchService,
    WebSearchUnavailable,
)

DUCKDUCKGO_PAGE = """
<html><body>
<div class="result">
  <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fone&amp;rut=x">
    K&#x1EBF;t qu&#x1EA3; m&#x1ED9;t
  </a>
  <a class="result__snippet">Đoạn mô tả <b>một</b>.</a>
</div>
<div class="result">
  <a class="result__a" href="https://example.org/two">Result two</a>
  <a class="result__snippet">Second snippet.</a>
</div>
<div class="result result--ad">
  <a class="result__a" href="//duckduckgo.com/y.js?ad=1">Quảng cáo</a>
</div>
</body></html>
"""


def _service(handler, config: WebSearchConfig) -> WebSearchService:
    return WebSearchService(
        lambda: config,
        timeout=5.0,
        transport=httpx.MockTransport(handler),
    )


@pytest.mark.asyncio
async def test_searxng_backend_reads_the_json_api() -> None:
    seen: dict[str, Any] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["url"] = str(request.url)
        return httpx.Response(
            200,
            json={
                "results": [
                    {
                        "title": "Trang một",
                        "url": "https://example.com/one",
                        "content": "Nội dung   một",
                        "engines": ["google", "brave"],
                    },
                    {"title": "Không có URL", "url": "ftp://nope"},
                ]
            },
        )

    config = WebSearchConfig(backend=SEARXNG, base_url="http://127.0.0.1:8888/")
    found = await _service(handler, config).search("thử nghiệm")

    assert seen["url"].startswith("http://127.0.0.1:8888/search?")
    assert "format=json" in seen["url"]
    # The ftp row is dropped rather than handed to the model as a citable source.
    assert [item.url for item in found.results] == ["https://example.com/one"]
    assert found.results[0].snippet == "Nội dung một"
    assert found.results[0].engine == "google, brave"
    assert config.on_device is True


@pytest.mark.asyncio
async def test_searxng_without_json_output_says_what_to_fix() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, text="<html>results</html>")

    config = WebSearchConfig(backend=SEARXNG, base_url="https://searx.example")
    with pytest.raises(WebSearchUnavailable, match="settings.yml"):
        await _service(handler, config).search("thử nghiệm")
    assert config.on_device is False


@pytest.mark.asyncio
async def test_searxng_needs_an_address_before_it_searches() -> None:
    def handler(request: httpx.Request) -> httpx.Response:  # pragma: no cover - never called
        raise AssertionError("no request should be sent without a configured host")

    service = _service(handler, WebSearchConfig(backend=SEARXNG, base_url=""))
    with pytest.raises(WebSearchNotConfigured):
        await service.search("thử nghiệm")


@pytest.mark.asyncio
async def test_duckduckgo_unwraps_redirect_links_and_drops_ads() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.method == "POST"
        assert b"q=" in request.content
        return httpx.Response(200, text=DUCKDUCKGO_PAGE)

    found = await _service(handler, WebSearchConfig(backend=DUCKDUCKGO)).search("codename")

    assert [item.url for item in found.results] == [
        "https://example.com/one",
        "https://example.org/two",
    ]
    assert found.results[0].title == "Kết quả một"
    assert found.results[0].snippet == "Đoạn mô tả một."
    assert found.summary == ""


@pytest.mark.asyncio
async def test_duckduckgo_rate_limit_reads_as_an_empty_page() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, text="<html><body>anomaly</body></html>")

    service = _service(handler, WebSearchConfig(backend=DUCKDUCKGO))
    with pytest.raises(WebSearchUnavailable, match="rate-limited"):
        await service.search("codename")


@pytest.mark.asyncio
async def test_openai_backend_returns_citations_and_a_summary() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.headers["authorization"] == "Bearer sk-test"
        return httpx.Response(
            200,
            json={
                "output": [
                    {"type": "web_search_call", "status": "completed"},
                    {
                        "type": "message",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Bản phát hành mới nhất là 2.1.",
                                "annotations": [
                                    {
                                        "type": "url_citation",
                                        "url": "https://example.com/release",
                                        "title": "Release notes",
                                    },
                                    {
                                        "type": "url_citation",
                                        "url": "https://example.com/release",
                                        "title": "Duplicate",
                                    },
                                ],
                            }
                        ],
                    },
                ]
            },
        )

    config = WebSearchConfig(backend=OPENAI, api_key="sk-test", model="gpt-5")
    found = await _service(handler, config).search("phiên bản mới nhất")

    assert found.summary == "Bản phát hành mới nhất là 2.1."
    assert [item.url for item in found.results] == ["https://example.com/release"]
    assert found.results[0].title == "Release notes"


@pytest.mark.asyncio
async def test_openai_backend_reports_the_api_error() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(401, json={"error": {"message": "Invalid API key"}})

    config = WebSearchConfig(backend=OPENAI, api_key="sk-bad")
    with pytest.raises(WebSearchUnavailable, match="Invalid API key"):
        await _service(handler, config).search("bất kỳ")


@pytest.mark.asyncio
async def test_openai_backend_refuses_to_run_without_a_key() -> None:
    def handler(request: httpx.Request) -> httpx.Response:  # pragma: no cover - never called
        raise AssertionError("no request should be sent without a key")

    service = _service(handler, WebSearchConfig(backend=OPENAI, api_key=""))
    with pytest.raises(WebSearchNotConfigured):
        await service.search("bất kỳ")


@pytest.mark.asyncio
async def test_probe_reports_the_host_instead_of_raising() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(500, text="boom")

    config = WebSearchConfig(backend=SEARXNG, base_url="https://searx.example")
    result = await _service(handler, config).probe(config)

    assert result["reachable"] is False
    assert result["host"] == "searx.example"
    assert result["on_device"] is False
    assert "not reachable" in str(result["detail"])


class FakeWebSearch:
    """Stands in for the search service so no test ever reaches the public internet."""

    def __init__(self, error: Exception | None = None) -> None:
        self.error = error
        self.queries: list[str] = []

    async def search(self, query: str, limit: int = 0):
        self.queries.append(query)
        if self.error:
            raise self.error
        from private_ai_api.services.web_search import WebSearchResponse, WebSearchResult

        return WebSearchResponse(
            query=query,
            backend=DUCKDUCKGO,
            results=[
                WebSearchResult(
                    title="Bản tin Starfruit",
                    url="https://example.com/starfruit",
                    snippet="Starfruit-Delta ra mắt tháng trước.",
                    engine="duckduckgo",
                )
            ],
        )


def test_chat_only_searches_the_web_when_the_message_asks_for_it(client: TestClient) -> None:
    fake_ollama = FakeOllama()
    client.app.state.services.ai = fake_ollama
    search = FakeWebSearch()
    client.app.state.services.web_search = search
    conversation = client.post(
        "/api/v1/workspaces/personal/conversations",
        json={"model": "test-model"},
    ).json()

    quiet = client.post(
        f"/api/v1/conversations/{conversation['id']}/chat",
        json={"model": "test-model", "content": "Không cần tra cứu"},
    )
    assert quiet.status_code == 200
    assert search.queries == []

    searched = client.post(
        f"/api/v1/conversations/{conversation['id']}/chat",
        json={
            "model": "test-model",
            "content": "Starfruit mới nhất là gì?",
            "web_search": True,
        },
    )
    assert searched.status_code == 200
    assert search.queries == ["Starfruit mới nhất là gì?"]
    assert fake_ollama.last_request is not None
    web_prompt = next(
        message.content
        for message in fake_ollama.last_request.messages
        if message.role == "system" and "https://example.com/starfruit" in message.content
    )
    assert "không đáng tin cậy" in web_prompt
    assert "Starfruit-Delta ra mắt tháng trước." in web_prompt


def test_a_failed_search_becomes_a_notice_and_still_answers(client: TestClient) -> None:
    client.app.state.services.ai = FakeOllama()
    client.app.state.services.web_search = FakeWebSearch(
        WebSearchUnavailable("DuckDuckGo returned no results, rate-limited"),
    )
    conversation = client.post(
        "/api/v1/workspaces/personal/conversations",
        json={"model": "test-model"},
    ).json()

    with client.stream(
        "POST",
        f"/api/v1/conversations/{conversation['id']}/chat/stream",
        json={"model": "test-model", "content": "Tin mới nhất?", "web_search": True},
    ) as response:
        body = "\n".join(response.iter_lines())

    assert response.status_code == 200
    assert '"type":"notice"' in body
    assert "rate-limited" in body
    # The answer still lands, so a flaky search host never costs the user their message.
    assert '"type":"done"' in body
    detail = client.get(f"/api/v1/conversations/{conversation['id']}").json()
    assert [message["role"] for message in detail["messages"]] == ["user", "assistant"]


def test_web_search_settings_round_trip_without_leaking_the_key(client: TestClient) -> None:
    updated = client.patch(
        "/api/v1/preferences",
        json={
            "web_search_enabled": True,
            "web_search_backend": "searxng",
            "web_search_base_url": "http://127.0.0.1:8888",
            "web_search_api_key": "sk-secret",
            "web_search_max_results": 8,
        },
    )

    assert updated.status_code == 200
    body = updated.json()
    assert body["web_search_enabled"] is True
    assert body["web_search_backend"] == "searxng"
    assert body["web_search_base_url"] == "http://127.0.0.1:8888"
    assert body["web_search_max_results"] == 8
    assert body["web_search_has_api_key"] is True
    assert "web_search_api_key" not in body
    assert client.get("/api/v1/preferences").json() == body


@pytest.mark.asyncio
async def test_mcp_exposes_web_search(tmp_path: Path) -> None:
    from conftest import FakeIndex

    server = create_mcp_server(
        Settings(
            data_dir=tmp_path,
            frontend_dist=tmp_path / "missing-web",
            embedding_enabled=False,
        ),
        FakeIndex(),  # type: ignore[arg-type]
    )
    tool_names = {tool.name for tool in await server.list_tools()}
    assert "web.search" in tool_names
