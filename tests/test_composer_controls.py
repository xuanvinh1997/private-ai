"""The two retrieval controls in the composer: what they offer and what they persist.

The picker used to be a combo box that wrote the *saved default* on every change, so one
click in a conversation silently re-aimed retrieval for every conversation after it. It
also offered "Web" beside a globe toggle that meant something else entirely. Both of those
are behaviour, not looks, which is why they are asserted here.
"""

from __future__ import annotations

import asyncio
import dataclasses
from typing import Any

import pytest

from private_ai.core.preferences import RETRIEVAL_STRATEGY_KEY, WEB_SEARCH_ENABLED_KEY
from private_ai.core.schemas import RetrievalStrategyName
from private_ai.core.services import AppServices
from private_ai.ui.widgets.strategy_picker import STRATEGY_CHOICES, StrategyPicker

# --- the picker on its own ------------------------------------------------


def test_every_strategy_but_web_is_offered() -> None:
    """Web is the globe toggle's job; offering it here gave one word two meanings."""
    offered = [value for value, _label, _hint in STRATEGY_CHOICES]

    assert offered[0] == RetrievalStrategyName.AUTO.value
    assert set(offered) == {name.value for name in RetrievalStrategyName} - {
        RetrievalStrategyName.WEB.value
    }


def test_it_shows_the_saved_default_until_the_user_picks(qapp) -> None:
    picker = StrategyPicker()
    chosen: list[str] = []
    picker.selectionChanged.connect(chosen.append)

    picker.set_default(RetrievalStrategyName.HYBRID.value)
    assert picker.current() == RetrievalStrategyName.HYBRID.value

    picker._choose(RetrievalStrategyName.KEYWORD.value)
    assert chosen == [RetrievalStrategyName.KEYWORD.value]
    assert picker.current() == RetrievalStrategyName.KEYWORD.value

    # The override outlives a later default: the session's choice is the user's, and a
    # preference arriving from Cài đặt must not quietly take it back.
    picker.set_default(RetrievalStrategyName.GRAPH.value)
    assert picker.current() == RetrievalStrategyName.KEYWORD.value


def test_a_strategy_it_no_longer_offers_falls_back_to_auto(qapp) -> None:
    picker = StrategyPicker()

    picker.set_default(RetrievalStrategyName.WEB.value)

    assert picker.current() == RetrievalStrategyName.AUTO.value


# --- the composer ---------------------------------------------------------


@pytest.fixture
def composer(services: AppServices, workspace_id: str, qapp, monkeypatch):
    """A real ChatView, with every preference write recorded instead of performed."""
    from private_ai.ui import theme
    from private_ai.ui.context import AppContext
    from private_ai.ui.views import chat_view as module

    written: list[tuple[str, str]] = []

    async def record(_database: Any, key: str, value: str) -> None:
        written.append((key, value))

    monkeypatch.setattr(module, "write_app_preference_async", record)
    theme.apply_theme(qapp, "light", "normal")
    context = AppContext(services=services)
    context.workspace_id = workspace_id
    view = module.ChatView(context)
    view.resize(900, 600)
    view.show()
    qapp.processEvents()
    yield view, context, written
    view.close()


async def test_choosing_a_strategy_leaves_the_saved_default_alone(composer, qapp) -> None:
    view, _context, written = composer

    view._strategy._choose(RetrievalStrategyName.KEYWORD.value)
    await asyncio.sleep(0)
    qapp.processEvents()

    assert view._strategy.current() == RetrievalStrategyName.KEYWORD.value
    assert not [key for key, _value in written if key == RETRIEVAL_STRATEGY_KEY]


async def test_the_web_toggle_is_still_a_saved_setting(composer, qapp) -> None:
    """The pair is deliberate: the override is per session, the web switch is not."""
    view, _context, written = composer

    view._web.setChecked(True)
    await asyncio.sleep(0)
    qapp.processEvents()

    assert (WEB_SEARCH_ENABLED_KEY, "1") in written


async def test_a_default_saved_as_web_moves_onto_the_web_toggle(composer, qapp) -> None:
    view, context, written = composer
    written.clear()

    view._apply_preferences(
        dataclasses.replace(context.preferences, retrieval_strategy=RetrievalStrategyName.WEB)
    )
    await asyncio.sleep(0)
    qapp.processEvents()

    assert view._strategy.current() == RetrievalStrategyName.AUTO.value
    assert view._web.isChecked()
    assert (RETRIEVAL_STRATEGY_KEY, RetrievalStrategyName.AUTO.value) in written
    assert (WEB_SEARCH_ENABLED_KEY, "1") in written
