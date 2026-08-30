"""Web search over whichever host the user picked.

Three backends with three different privacy stories: a SearXNG instance the user runs
(possibly on this very machine), DuckDuckGo's no-JavaScript HTML page, and OpenAI's
Responses API. The service knows how to reach each one and nothing about who is asking.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from html import unescape
from html.parser import HTMLParser
from typing import Any
from urllib.parse import parse_qs, urlsplit

import httpx

SEARXNG = "searxng"
DUCKDUCKGO = "duckduckgo"
OPENAI = "openai"
WEB_SEARCH_BACKENDS = (SEARXNG, DUCKDUCKGO, OPENAI)

DUCKDUCKGO_ENDPOINT = "https://html.duckduckgo.com/html/"
OPENAI_RESPONSES_ENDPOINT = "https://api.openai.com/v1/responses"
# DuckDuckGo serves the no-JavaScript page only to something that looks like a browser.
BROWSER_USER_AGENT = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/125.0 Safari/537.36"
)
MAX_SNIPPET_CHARS = 600

LOOPBACK_HOSTS = frozenset({"localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0"})


def runs_on_device(base_url: str) -> bool:
    """A host is on-device when its endpoint never leaves the loopback interface."""
    host = urlsplit(base_url).hostname
    return host is not None and host.lower() in LOOPBACK_HOSTS


class WebSearchUnavailable(RuntimeError):
    """The configured search host could not answer the query."""


class WebSearchNotConfigured(WebSearchUnavailable):
    """Web search is switched off, or its host has not been filled in yet."""


@dataclass(frozen=True, slots=True)
class WebSearchResult:
    title: str
    url: str
    snippet: str = ""
    engine: str = ""

    def public(self) -> dict[str, str]:
        return {
            "title": self.title,
            "url": self.url,
            "snippet": self.snippet,
            "engine": self.engine,
        }


@dataclass(frozen=True, slots=True)
class WebSearchResponse:
    query: str
    backend: str
    results: list[WebSearchResult] = field(default_factory=list)
    # Only the OpenAI backend answers in prose; the others return links alone.
    summary: str = ""

    def public(self) -> dict[str, Any]:
        return {
            "query": self.query,
            "backend": self.backend,
            "summary": self.summary,
            "results": [item.public() for item in self.results],
        }


@dataclass(frozen=True, slots=True)
class WebSearchConfig:
    """Where a search goes, and whether the query would leave this machine."""

    backend: str = DUCKDUCKGO
    base_url: str = ""
    api_key: str = ""
    model: str = "gpt-5"
    max_results: int = 5

    @property
    def host_label(self) -> str:
        if self.backend == SEARXNG:
            return urlsplit(self.base_url).netloc or self.base_url
        if self.backend == DUCKDUCKGO:
            return "duckduckgo.com"
        return "api.openai.com"

    @property
    def on_device(self) -> bool:
        """True only for a SearXNG instance running on this machine's loopback."""
        return self.backend == SEARXNG and runs_on_device(self.base_url)


def _clean(value: object, limit: int = MAX_SNIPPET_CHARS) -> str:
    text = " ".join(unescape(str(value or "")).split())
    return text[:limit]


class _DuckDuckGoParser(HTMLParser):
    """Reads titles, links and snippets out of the no-JavaScript results page.

    DuckDuckGo publishes no search API, so this markup is the contract and it can change
    without warning. A layout change costs us the results, never an exception.
    """

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.results: list[dict[str, str]] = []
        self._collecting: str = ""
        self._buffer: list[str] = []
        self._href: str = ""

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag != "a":
            return
        attributes = dict(attrs)
        classes = (attributes.get("class") or "").split()
        if "result__a" in classes:
            self._collecting = "title"
            self._buffer = []
            self._href = _redirect_target(attributes.get("href") or "")
        elif "result__snippet" in classes:
            self._collecting = "snippet"
            self._buffer = []

    def handle_data(self, data: str) -> None:
        if self._collecting:
            self._buffer.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag != "a" or not self._collecting:
            return
        text = _clean("".join(self._buffer))
        if self._collecting == "title":
            if self._href:
                self.results.append({"title": text, "url": self._href, "snippet": ""})
        elif self.results and not self.results[-1]["snippet"]:
            self.results[-1]["snippet"] = text
        self._collecting = ""
        self._buffer = []


def _redirect_target(href: str) -> str:
    """DuckDuckGo wraps outbound links in /l/?uddg=…; the real URL is the payload."""
    if not href:
        return ""
    absolute = f"https:{href}" if href.startswith("//") else href
    parts = urlsplit(absolute)
    if parts.path.startswith("/l/"):
        target = parse_qs(parts.query).get("uddg", [""])[0]
        return target if target.startswith(("http://", "https://")) else ""
    host = (parts.hostname or "").lower()
    # Sponsored rows (/y.js) and pagination point back at DuckDuckGo itself. Neither is a
    # source the model should be able to cite, so only outbound links survive.
    if host == "duckduckgo.com" or host.endswith(".duckduckgo.com"):
        return ""
    return absolute if absolute.startswith(("http://", "https://")) else ""


class WebSearchService:
    """Runs one query against whichever search host the user picked.

    The configuration is resolved per call rather than captured once, so a settings change
    takes effect on the next message instead of at the next restart.
    """

    def __init__(
        self,
        resolve_config: Callable[[], WebSearchConfig],
        *,
        timeout: float = 20.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self.resolve_config = resolve_config
        self.timeout = timeout
        self.transport = transport

    def config(self) -> WebSearchConfig:
        return self.resolve_config()

    async def search(self, query: str, limit: int = 0) -> WebSearchResponse:
        config = self.config()
        return await self.run(query, config, limit=limit)

    async def run(
        self,
        query: str,
        config: WebSearchConfig,
        *,
        limit: int = 0,
    ) -> WebSearchResponse:
        text = query.strip()
        if not text:
            raise WebSearchUnavailable("Câu truy vấn tìm kiếm không được để trống")
        count = max(1, min(limit or config.max_results, 10))
        if config.backend == SEARXNG:
            results = await self._searxng(text, config, count)
            summary = ""
        elif config.backend == DUCKDUCKGO:
            results = await self._duckduckgo(text, count)
            summary = ""
        elif config.backend == OPENAI:
            results, summary = await self._openai(text, config, count)
        else:
            raise WebSearchNotConfigured(
                f"Backend tìm kiếm web không được hỗ trợ: {config.backend}"
            )
        return WebSearchResponse(
            query=text,
            backend=config.backend,
            results=results[:count],
            summary=summary,
        )

    async def probe(self, config: WebSearchConfig) -> dict[str, Any]:
        """Run one throwaway query so the user finds a bad host in settings, not mid-chat."""
        try:
            response = await self.run("private ai", config, limit=3)
        except WebSearchUnavailable as exc:
            return {
                "reachable": False,
                "result_count": 0,
                "host": config.host_label,
                "on_device": config.on_device,
                "detail": str(exc),
            }
        return {
            "reachable": True,
            "result_count": len(response.results),
            "host": config.host_label,
            "on_device": config.on_device,
            "detail": None,
        }

    def _client(self) -> httpx.AsyncClient:
        return httpx.AsyncClient(timeout=self.timeout, transport=self.transport)

    async def _searxng(
        self,
        query: str,
        config: WebSearchConfig,
        count: int,
    ) -> list[WebSearchResult]:
        base_url = config.base_url.strip().rstrip("/")
        if not base_url:
            raise WebSearchNotConfigured("Hãy đặt địa chỉ SearXNG trước khi tìm kiếm trên web")
        try:
            async with self._client() as client:
                response = await client.get(
                    f"{base_url}/search",
                    params={
                        "q": query,
                        "format": "json",
                        "safesearch": 1,
                    },
                    headers={"accept": "application/json"},
                )
                response.raise_for_status()
                payload = response.json()
        except httpx.HTTPError as exc:
            raise WebSearchUnavailable(f"Không kết nối được SearXNG: {exc}") from exc
        except ValueError as exc:
            # A stock SearXNG only emits HTML; JSON has to be enabled in settings.yml.
            raise WebSearchUnavailable(
                "SearXNG không trả về JSON. Hãy thêm 'json' vào search.formats trong settings.yml."
            ) from exc
        rows = payload.get("results") if isinstance(payload, dict) else None
        if not isinstance(rows, list):
            raise WebSearchUnavailable("SearXNG trả về dữ liệu không đúng định dạng")
        return [
            item for item in (_searxng_result(row) for row in rows[: count * 2]) if item is not None
        ]

    async def _duckduckgo(self, query: str, count: int) -> list[WebSearchResult]:
        try:
            async with self._client() as client:
                response = await client.post(
                    DUCKDUCKGO_ENDPOINT,
                    data={"q": query, "kl": "wt-wt"},
                    headers={
                        "user-agent": BROWSER_USER_AGENT,
                        "accept": "text/html",
                        "content-type": "application/x-www-form-urlencoded",
                    },
                    follow_redirects=True,
                )
                response.raise_for_status()
                body = response.text
        except httpx.HTTPError as exc:
            raise WebSearchUnavailable(f"Không kết nối được DuckDuckGo: {exc}") from exc
        parser = _DuckDuckGoParser()
        parser.feed(body)
        parser.close()
        if not parser.results:
            # DuckDuckGo answers 200 with an interstitial when it rate-limits a client.
            raise WebSearchUnavailable(
                "DuckDuckGo không trả về kết quả nào, thường là do máy này bị giới hạn "
                "tần suất truy cập. Hãy đợi một lát, hoặc chuyển sang SearXNG."
            )
        return [
            WebSearchResult(
                title=row["title"],
                url=row["url"],
                snippet=row["snippet"],
                engine="duckduckgo",
            )
            for row in parser.results[: count * 2]
            if row["title"] and row["url"]
        ]

    async def _openai(
        self,
        query: str,
        config: WebSearchConfig,
        count: int,
    ) -> tuple[list[WebSearchResult], str]:
        if not config.api_key.strip():
            raise WebSearchNotConfigured(
                "Hãy thêm khóa API OpenAI trước khi dùng tìm kiếm web của OpenAI"
            )
        payload = {
            "model": config.model or "gpt-5",
            "input": query,
            "tools": [{"type": "web_search"}],
            "tool_choice": "required",
        }
        try:
            async with self._client() as client:
                response = await client.post(
                    OPENAI_RESPONSES_ENDPOINT,
                    json=payload,
                    headers={
                        "authorization": f"Bearer {config.api_key.strip()}",
                        "content-type": "application/json",
                    },
                )
                if response.status_code >= 400:
                    raise WebSearchUnavailable(_openai_error(response))
                body = response.json()
        except httpx.HTTPError as exc:
            raise WebSearchUnavailable(
                f"Không kết nối được tìm kiếm web của OpenAI: {exc}"
            ) from exc
        except ValueError as exc:
            raise WebSearchUnavailable("OpenAI trả về phản hồi không đọc được") from exc
        return _openai_results(body, count)


def _searxng_result(row: object) -> WebSearchResult | None:
    if not isinstance(row, dict):
        return None
    url = str(row.get("url") or "").strip()
    if not url.startswith(("http://", "https://")):
        return None
    engines = row.get("engines")
    engine = ", ".join(str(item) for item in engines) if isinstance(engines, list) else ""
    return WebSearchResult(
        title=_clean(row.get("title"), 200) or url,
        url=url,
        snippet=_clean(row.get("content")),
        engine=engine or _clean(row.get("engine"), 60) or "searxng",
    )


def _openai_error(response: httpx.Response) -> str:
    try:
        detail = response.json().get("error", {}).get("message")
    except ValueError:
        detail = None
    return (
        f"Tìm kiếm web của OpenAI thất bại ({response.status_code}): "
        f"{detail or response.text[:200]}"
    )


def _openai_results(body: object, count: int) -> tuple[list[WebSearchResult], str]:
    """Pull the answer text and its url_citation annotations out of a Responses payload."""
    output = body.get("output") if isinstance(body, dict) else None
    if not isinstance(output, list):
        return [], ""
    summary_parts: list[str] = []
    results: dict[str, WebSearchResult] = {}
    for item in output:
        if not isinstance(item, dict) or item.get("type") != "message":
            continue
        for block in item.get("content") or []:
            if not isinstance(block, dict) or block.get("type") != "output_text":
                continue
            summary_parts.append(str(block.get("text") or ""))
            for annotation in block.get("annotations") or []:
                if not isinstance(annotation, dict):
                    continue
                if annotation.get("type") != "url_citation":
                    continue
                url = str(annotation.get("url") or "").strip()
                if not url.startswith(("http://", "https://")) or url in results:
                    continue
                results[url] = WebSearchResult(
                    title=_clean(annotation.get("title"), 200) or url,
                    url=url,
                    snippet="",
                    engine="openai",
                )
    summary = "\n".join(part for part in summary_parts if part.strip()).strip()
    return list(results.values())[: count * 2], summary[:4_000]
