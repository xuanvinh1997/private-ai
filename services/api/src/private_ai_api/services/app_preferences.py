from __future__ import annotations

from dataclasses import dataclass

from private_ai_api.database import Database
from private_ai_api.schemas import RagMode, WebSearchBackend
from private_ai_api.services.web_search import WebSearchConfig

OCR_ENABLED_KEY = "ocr_enabled"
RAG_MODE_KEY = "rag_mode"
GRAPH_MODEL_KEY = "graph_model"
EMBEDDING_BATCH_SIZE_KEY = "embedding_batch_size"
EMBEDDING_CONCURRENCY_KEY = "embedding_concurrency"
WEB_SEARCH_ENABLED_KEY = "web_search_enabled"
WEB_SEARCH_BACKEND_KEY = "web_search_backend"
WEB_SEARCH_BASE_URL_KEY = "web_search_base_url"
WEB_SEARCH_API_KEY_KEY = "web_search_api_key"
WEB_SEARCH_MODEL_KEY = "web_search_model"
WEB_SEARCH_MAX_RESULTS_KEY = "web_search_max_results"

DEFAULT_EMBEDDING_BATCH_SIZE = 32
DEFAULT_EMBEDDING_CONCURRENCY = 4
# DuckDuckGo needs no key and no server, so the toggle works before anything is configured.
DEFAULT_WEB_SEARCH_BACKEND = WebSearchBackend.DUCKDUCKGO
DEFAULT_WEB_SEARCH_MODEL = "gpt-5"
DEFAULT_WEB_SEARCH_MAX_RESULTS = 5

WEB_SEARCH_KEYS = (
    WEB_SEARCH_ENABLED_KEY,
    WEB_SEARCH_BACKEND_KEY,
    WEB_SEARCH_BASE_URL_KEY,
    WEB_SEARCH_API_KEY_KEY,
    WEB_SEARCH_MODEL_KEY,
    WEB_SEARCH_MAX_RESULTS_KEY,
)


@dataclass(frozen=True)
class AppPreferences:
    ocr_enabled: bool = True
    rag_mode: RagMode = RagMode.SIMPLE
    graph_model: str = ""
    embedding_batch_size: int = DEFAULT_EMBEDDING_BATCH_SIZE
    embedding_concurrency: int = DEFAULT_EMBEDDING_CONCURRENCY
    web_search_enabled: bool = False
    web_search_backend: WebSearchBackend = DEFAULT_WEB_SEARCH_BACKEND
    web_search_base_url: str = ""
    web_search_model: str = DEFAULT_WEB_SEARCH_MODEL
    web_search_max_results: int = DEFAULT_WEB_SEARCH_MAX_RESULTS
    # The key itself never leaves the service layer; the API only reports that one exists.
    web_search_api_key: str = ""

    @property
    def web_search_has_api_key(self) -> bool:
        return bool(self.web_search_api_key)


def _bounded_int(value: object, default: int, minimum: int, maximum: int) -> int:
    try:
        parsed = int(str(value))
    except (TypeError, ValueError):
        return default
    return parsed if minimum <= parsed <= maximum else default


def _web_search_backend(value: object) -> WebSearchBackend:
    try:
        return WebSearchBackend(str(value or DEFAULT_WEB_SEARCH_BACKEND))
    except ValueError:
        return DEFAULT_WEB_SEARCH_BACKEND


def read_app_preferences(database: Database) -> AppPreferences:
    keys = (
        OCR_ENABLED_KEY,
        RAG_MODE_KEY,
        GRAPH_MODEL_KEY,
        EMBEDDING_BATCH_SIZE_KEY,
        EMBEDDING_CONCURRENCY_KEY,
        *WEB_SEARCH_KEYS,
    )
    placeholders = ", ".join("?" for _ in keys)
    rows = database.fetch_all(
        f"SELECT key, value FROM app_state WHERE key IN ({placeholders})",  # noqa: S608
        keys,
    )
    values = {str(row["key"]): row["value"] for row in rows}
    try:
        rag_mode = RagMode(str(values.get(RAG_MODE_KEY, RagMode.SIMPLE)))
    except ValueError:
        rag_mode = RagMode.SIMPLE
    return AppPreferences(
        web_search_enabled=str(values.get(WEB_SEARCH_ENABLED_KEY, "0")) == "1",
        web_search_backend=_web_search_backend(values.get(WEB_SEARCH_BACKEND_KEY)),
        web_search_base_url=str(values.get(WEB_SEARCH_BASE_URL_KEY, "") or ""),
        web_search_api_key=str(values.get(WEB_SEARCH_API_KEY_KEY, "") or ""),
        web_search_model=str(values.get(WEB_SEARCH_MODEL_KEY, "") or DEFAULT_WEB_SEARCH_MODEL),
        web_search_max_results=_bounded_int(
            values.get(WEB_SEARCH_MAX_RESULTS_KEY),
            DEFAULT_WEB_SEARCH_MAX_RESULTS,
            1,
            10,
        ),
        ocr_enabled=str(values.get(OCR_ENABLED_KEY, "1")) == "1",
        rag_mode=rag_mode,
        graph_model=str(values.get(GRAPH_MODEL_KEY, "") or "").strip(),
        embedding_batch_size=_bounded_int(
            values.get(EMBEDDING_BATCH_SIZE_KEY),
            DEFAULT_EMBEDDING_BATCH_SIZE,
            1,
            256,
        ),
        embedding_concurrency=_bounded_int(
            values.get(EMBEDDING_CONCURRENCY_KEY),
            DEFAULT_EMBEDDING_CONCURRENCY,
            1,
            32,
        ),
    )


def read_web_search_config(database: Database) -> WebSearchConfig:
    """The shape the search service wants, without handing it the whole preference set."""
    preferences = read_app_preferences(database)
    return WebSearchConfig(
        backend=preferences.web_search_backend.value,
        base_url=preferences.web_search_base_url,
        api_key=preferences.web_search_api_key,
        model=preferences.web_search_model,
        max_results=preferences.web_search_max_results,
    )


def write_app_preference(database: Database, key: str, value: str) -> None:
    database.execute(
        """
        INSERT INTO app_state(key, value) VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        """,
        (key, value),
    )
