"""App preferences: one row per key, with every value validated on the way out."""

from __future__ import annotations

from private_ai.core import preferences as prefs
from private_ai.core.database import Database
from private_ai.core.schemas import (
    PreferencesUpdate,
    RagMode,
    RetrievalStrategyName,
    WebSearchBackend,
)


async def test_defaults_apply_to_an_untouched_database(database: Database) -> None:
    values = prefs.read_app_preferences(database)
    assert values.ocr_enabled is True
    assert values.rag_mode is RagMode.SIMPLE
    assert values.retrieval_strategy is RetrievalStrategyName.AUTO
    assert values.skills_enabled is True
    assert values.agent_max_iterations == prefs.DEFAULT_AGENT_MAX_ITERATIONS
    # Off by default: web search is the only feature that sends anything off the machine.
    assert values.web_search_enabled is False
    assert values.web_search_backend is prefs.DEFAULT_WEB_SEARCH_BACKEND


async def test_an_update_writes_only_the_fields_it_names(database: Database) -> None:
    await prefs.write_app_preferences(database, PreferencesUpdate(ocr_enabled=False))
    after = await prefs.write_app_preferences(
        database,
        PreferencesUpdate(retrieval_strategy=RetrievalStrategyName.HYBRID),
    )
    assert after.ocr_enabled is False
    assert after.retrieval_strategy is RetrievalStrategyName.HYBRID

    stored = {
        str(row["key"])
        for row in database.fetch_all("SELECT key FROM app_state WHERE key LIKE '%enabled%'")
    }
    assert prefs.OCR_ENABLED_KEY in stored
    # Nothing wrote the keys the update did not mention.
    assert prefs.WEB_SEARCH_ENABLED_KEY not in stored


async def test_out_of_range_and_junk_values_fall_back_instead_of_raising(
    database: Database,
) -> None:
    """These rows are user-editable state; a bad one must not brick the settings screen."""
    prefs.write_app_preference(database, prefs.EMBEDDING_BATCH_SIZE_KEY, "9999")
    prefs.write_app_preference(database, prefs.EMBEDDING_CONCURRENCY_KEY, "không phải số")
    prefs.write_app_preference(database, prefs.RETRIEVAL_STRATEGY_KEY, "telepathy")
    prefs.write_app_preference(database, prefs.RAG_MODE_KEY, "quantum")
    prefs.write_app_preference(database, prefs.UI_THEME_KEY, "neon")
    prefs.write_app_preference(database, prefs.WEB_SEARCH_BACKEND_KEY, "altavista")
    prefs.write_app_preference(database, prefs.AGENT_MAX_ITERATIONS_KEY, "0")

    values = await prefs.read_app_preferences_async(database)

    assert values.embedding_batch_size == prefs.DEFAULT_EMBEDDING_BATCH_SIZE
    assert values.embedding_concurrency == prefs.DEFAULT_EMBEDDING_CONCURRENCY
    assert values.retrieval_strategy is prefs.DEFAULT_RETRIEVAL_STRATEGY
    assert values.rag_mode is RagMode.SIMPLE
    assert values.ui_theme == prefs.DEFAULT_UI_THEME
    assert values.web_search_backend is prefs.DEFAULT_WEB_SEARCH_BACKEND
    assert values.agent_max_iterations == prefs.DEFAULT_AGENT_MAX_ITERATIONS


async def test_the_web_search_api_key_never_leaves_the_service_layer(
    database: Database,
) -> None:
    await prefs.write_app_preferences(
        database,
        PreferencesUpdate(
            web_search_enabled=True,
            web_search_backend=WebSearchBackend.OPENAI,
            web_search_api_key="sk-bí-mật",
        ),
    )
    values = await prefs.read_app_preferences_async(database)
    assert values.web_search_api_key == "sk-bí-mật"

    record = values.record()
    assert record.web_search_has_api_key is True
    # The redacted shape the settings view binds to carries no field for the key at all.
    assert "api_key" not in record.model_dump(mode="json")

    # The search service does get it — it is the only caller that needs it.
    config = prefs.read_web_search_config(database)
    assert config.api_key == "sk-bí-mật"
    assert config.backend == WebSearchBackend.OPENAI.value


async def test_booleans_round_trip_as_the_stored_zero_or_one(database: Database) -> None:
    await prefs.write_app_preferences(database, PreferencesUpdate(skills_enabled=False))
    row = database.fetch_one(
        "SELECT value FROM app_state WHERE key = ?",
        (prefs.SKILLS_ENABLED_KEY,),
    )
    assert row == {"value": "0"}
    assert (await prefs.read_app_preferences_async(database)).skills_enabled is False


def test_apply_update_merges_without_touching_the_database(database: Database) -> None:
    base = prefs.read_app_preferences(database)
    merged = prefs.apply_update(base, PreferencesUpdate(ui_font_scale="large"))

    assert merged.ui_font_scale == "large"
    assert base.ui_font_scale == prefs.DEFAULT_UI_FONT_SCALE
    assert prefs.read_app_preferences(database).ui_font_scale == prefs.DEFAULT_UI_FONT_SCALE
