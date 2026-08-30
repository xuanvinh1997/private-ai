"""User preferences, stored one key at a time in ``app_state``.

A row-per-key table rather than a settings blob, so a new preference never has to
migrate the old ones and two processes writing different keys never collide.
"""

from __future__ import annotations

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Any

from private_ai.core.database import Database
from private_ai.core.schemas import (
    PreferencesRecord,
    PreferencesUpdate,
    RagMode,
    RetrievalStrategyName,
    WebSearchBackend,
)

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.rag.web_search import WebSearchConfig

OCR_ENABLED_KEY = "ocr_enabled"
RAG_MODE_KEY = "rag_mode"
GRAPH_MODEL_KEY = "graph_model"
EMBEDDING_BATCH_SIZE_KEY = "embedding_batch_size"
EMBEDDING_CONCURRENCY_KEY = "embedding_concurrency"
RETRIEVAL_STRATEGY_KEY = "retrieval_strategy"
SKILLS_ENABLED_KEY = "skills_enabled"
AGENT_MAX_ITERATIONS_KEY = "agent_max_iterations"
UI_THEME_KEY = "ui_theme"
UI_FONT_SCALE_KEY = "ui_font_scale"
WEB_SEARCH_ENABLED_KEY = "web_search_enabled"
WEB_SEARCH_BACKEND_KEY = "web_search_backend"
WEB_SEARCH_BASE_URL_KEY = "web_search_base_url"
WEB_SEARCH_API_KEY_KEY = "web_search_api_key"
WEB_SEARCH_MODEL_KEY = "web_search_model"
WEB_SEARCH_MAX_RESULTS_KEY = "web_search_max_results"

DEFAULT_EMBEDDING_BATCH_SIZE = 32
DEFAULT_EMBEDDING_CONCURRENCY = 4
DEFAULT_RETRIEVAL_STRATEGY = RetrievalStrategyName.AUTO
DEFAULT_AGENT_MAX_ITERATIONS = 10
DEFAULT_UI_THEME = "light"
DEFAULT_UI_FONT_SCALE = "normal"
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

PREFERENCE_KEYS = (
    OCR_ENABLED_KEY,
    RAG_MODE_KEY,
    GRAPH_MODEL_KEY,
    EMBEDDING_BATCH_SIZE_KEY,
    EMBEDDING_CONCURRENCY_KEY,
    RETRIEVAL_STRATEGY_KEY,
    SKILLS_ENABLED_KEY,
    AGENT_MAX_ITERATIONS_KEY,
    UI_THEME_KEY,
    UI_FONT_SCALE_KEY,
    *WEB_SEARCH_KEYS,
)

UI_THEMES = ("light", "dark", "system")
UI_FONT_SCALES = ("compact", "normal", "large")


@dataclass(frozen=True)
class AppPreferences:
    ocr_enabled: bool = True
    rag_mode: RagMode = RagMode.SIMPLE
    graph_model: str = ""
    embedding_batch_size: int = DEFAULT_EMBEDDING_BATCH_SIZE
    embedding_concurrency: int = DEFAULT_EMBEDDING_CONCURRENCY
    retrieval_strategy: RetrievalStrategyName = DEFAULT_RETRIEVAL_STRATEGY
    skills_enabled: bool = True
    agent_max_iterations: int = DEFAULT_AGENT_MAX_ITERATIONS
    ui_theme: str = DEFAULT_UI_THEME
    ui_font_scale: str = DEFAULT_UI_FONT_SCALE
    web_search_enabled: bool = False
    web_search_backend: WebSearchBackend = DEFAULT_WEB_SEARCH_BACKEND
    web_search_base_url: str = ""
    web_search_model: str = DEFAULT_WEB_SEARCH_MODEL
    web_search_max_results: int = DEFAULT_WEB_SEARCH_MAX_RESULTS
    # The key itself never leaves the service layer; callers only learn that one exists.
    web_search_api_key: str = ""

    @property
    def web_search_has_api_key(self) -> bool:
        return bool(self.web_search_api_key)

    def record(self) -> PreferencesRecord:
        """The redacted shape the settings view binds to."""
        return PreferencesRecord(
            ocr_enabled=self.ocr_enabled,
            rag_mode=self.rag_mode,
            graph_model=self.graph_model,
            embedding_batch_size=self.embedding_batch_size,
            embedding_concurrency=self.embedding_concurrency,
            retrieval_strategy=self.retrieval_strategy,
            skills_enabled=self.skills_enabled,
            agent_max_iterations=self.agent_max_iterations,
            ui_theme=self.ui_theme,
            ui_font_scale=self.ui_font_scale,
            web_search_enabled=self.web_search_enabled,
            web_search_backend=self.web_search_backend,
            web_search_base_url=self.web_search_base_url,
            web_search_model=self.web_search_model,
            web_search_max_results=self.web_search_max_results,
            web_search_has_api_key=self.web_search_has_api_key,
        )


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


def _retrieval_strategy(value: object) -> RetrievalStrategyName:
    try:
        return RetrievalStrategyName(str(value or DEFAULT_RETRIEVAL_STRATEGY))
    except ValueError:
        return DEFAULT_RETRIEVAL_STRATEGY


def _one_of(value: object, allowed: tuple[str, ...], default: str) -> str:
    candidate = str(value or "").strip()
    return candidate if candidate in allowed else default


def _preferences(values: dict[str, Any]) -> AppPreferences:
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
        retrieval_strategy=_retrieval_strategy(values.get(RETRIEVAL_STRATEGY_KEY)),
        skills_enabled=str(values.get(SKILLS_ENABLED_KEY, "1")) == "1",
        agent_max_iterations=_bounded_int(
            values.get(AGENT_MAX_ITERATIONS_KEY),
            DEFAULT_AGENT_MAX_ITERATIONS,
            1,
            64,
        ),
        ui_theme=_one_of(values.get(UI_THEME_KEY), UI_THEMES, DEFAULT_UI_THEME),
        ui_font_scale=_one_of(values.get(UI_FONT_SCALE_KEY), UI_FONT_SCALES, DEFAULT_UI_FONT_SCALE),
    )


_QUERY = "SELECT key, value FROM app_state WHERE key IN ({})".format(  # noqa: S608
    ", ".join("?" for _ in PREFERENCE_KEYS)
)


def read_app_preferences(database: Database) -> AppPreferences:
    rows = database.fetch_all(_QUERY, PREFERENCE_KEYS)
    return _preferences({str(row["key"]): row["value"] for row in rows})


async def read_app_preferences_async(database: Database) -> AppPreferences:
    rows = await database.fetch_all_async(_QUERY, PREFERENCE_KEYS)
    return _preferences({str(row["key"]): row["value"] for row in rows})


def read_web_search_config(database: Database) -> WebSearchConfig:
    """The shape the search service wants, without handing it the whole preference set."""
    # Imported here rather than at module scope: the search service imports preferences
    # back for its own defaults, and the worker loads this module without it.
    from private_ai.rag.web_search import WebSearchConfig

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


async def write_app_preference_async(database: Database, key: str, value: str) -> None:
    await database.execute_async(
        """
        INSERT INTO app_state(key, value) VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        """,
        (key, value),
    )


def _serialize(value: Any) -> str:
    if isinstance(value, bool):
        return "1" if value else "0"
    return str(getattr(value, "value", value))


_UPDATE_KEYS = {
    "ocr_enabled": OCR_ENABLED_KEY,
    "rag_mode": RAG_MODE_KEY,
    "graph_model": GRAPH_MODEL_KEY,
    "embedding_batch_size": EMBEDDING_BATCH_SIZE_KEY,
    "embedding_concurrency": EMBEDDING_CONCURRENCY_KEY,
    "retrieval_strategy": RETRIEVAL_STRATEGY_KEY,
    "skills_enabled": SKILLS_ENABLED_KEY,
    "agent_max_iterations": AGENT_MAX_ITERATIONS_KEY,
    "ui_theme": UI_THEME_KEY,
    "ui_font_scale": UI_FONT_SCALE_KEY,
    "web_search_enabled": WEB_SEARCH_ENABLED_KEY,
    "web_search_backend": WEB_SEARCH_BACKEND_KEY,
    "web_search_base_url": WEB_SEARCH_BASE_URL_KEY,
    "web_search_api_key": WEB_SEARCH_API_KEY_KEY,
    "web_search_model": WEB_SEARCH_MODEL_KEY,
    "web_search_max_results": WEB_SEARCH_MAX_RESULTS_KEY,
}


async def write_app_preferences(
    database: Database,
    update: PreferencesUpdate,
) -> AppPreferences:
    """Apply only the fields the caller actually set, then read the result back."""
    for field, key in _UPDATE_KEYS.items():
        value = getattr(update, field, None)
        if value is None:
            continue
        await write_app_preference_async(database, key, _serialize(value))
    return await read_app_preferences_async(database)


def apply_update(preferences: AppPreferences, update: PreferencesUpdate) -> AppPreferences:
    """Merge an update into a preference set without touching the database."""
    changes = {
        field: value
        for field in _UPDATE_KEYS
        if (value := getattr(update, field, None)) is not None
    }
    return replace(preferences, **changes)
