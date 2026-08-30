"""The configured AI hosts: seeding, selection, and what the UI is allowed to see."""

from __future__ import annotations

import pytest

from private_ai.config import Settings
from private_ai.core.database import Database
from private_ai.llm import UnknownProvider
from private_ai.llm.registry import (
    LOCAL_PROVIDER_ID,
    ProviderRegistry,
    runs_on_device,
)


@pytest.fixture
def registry(database: Database, settings: Settings) -> ProviderRegistry:
    return ProviderRegistry(database, settings=settings)


def test_the_local_ollama_row_is_seeded_on_a_fresh_database(
    registry: ProviderRegistry,
    settings: Settings,
) -> None:
    configs = registry.list_configs()
    assert [config.id for config in configs] == [LOCAL_PROVIDER_ID]
    assert configs[0].base_url == settings.ollama_url
    assert configs[0].builtin is True
    assert registry.active_id() == LOCAL_PROVIDER_ID


def test_a_deleted_local_provider_is_never_resurrected(
    database: Database,
    settings: Settings,
    registry: ProviderRegistry,
) -> None:
    """The seed marker is what makes deletion stick across restarts."""
    registry.delete(LOCAL_PROVIDER_ID)
    assert registry.list_configs() == []

    # A second registry is what the next process start looks like.
    restarted = ProviderRegistry(database, settings=settings)
    assert restarted.list_configs() == []
    assert restarted.active_id() == ""
    assert restarted.active_config() is None


def test_deleting_the_active_provider_repoints_the_selection(
    registry: ProviderRegistry,
) -> None:
    remote = registry.create(
        name="vLLM",
        kind="openai",
        base_url="https://gpu.example.com/v1",
        api_key="sk-abc",
    )
    registry.activate(remote.id)
    assert registry.active_id() == remote.id

    registry.delete(remote.id)
    assert registry.active_id() == LOCAL_PROVIDER_ID


def test_the_api_key_is_stored_but_never_published(registry: ProviderRegistry) -> None:
    created = registry.create(
        name="OpenAI",
        kind="openai",
        base_url="https://api.openai.com/v1",
        api_key="sk-bí-mật",
    )
    assert registry.get(created.id).api_key == "sk-bí-mật"

    published = created.public(active=False)
    assert published["has_api_key"] is True
    assert "api_key" not in published
    assert all("api_key" not in item for item in registry.public_list())
    assert "sk-bí-mật" not in str(registry.public_list())


def test_updating_without_a_key_keeps_the_stored_one(registry: ProviderRegistry) -> None:
    created = registry.create(
        name="OpenAI",
        kind="openai",
        base_url="https://api.openai.com/v1",
        api_key="sk-first",
    )
    renamed = registry.update(created.id, name="OpenAI Cloud")
    assert renamed.name == "OpenAI Cloud"
    assert registry.get(created.id).api_key == "sk-first"

    replaced = registry.update(created.id, api_key="sk-second")
    assert replaced.api_key == "sk-second"


def test_an_ollama_host_never_carries_a_key(registry: ProviderRegistry) -> None:
    updated = registry.update(LOCAL_PROVIDER_ID, api_key="sk-pointless")
    assert updated.api_key == ""


def test_validation_rejects_an_unknown_kind_or_a_bare_host(
    registry: ProviderRegistry,
) -> None:
    with pytest.raises(ValueError, match="kind"):
        registry.create(name="X", kind="anthropic", base_url="https://x/v1")
    with pytest.raises(ValueError, match="http"):
        registry.create(name="X", kind="openai", base_url="gpu.example.com")


def test_a_disabled_provider_cannot_be_activated_and_is_skipped(
    registry: ProviderRegistry,
) -> None:
    remote = registry.create(name="Off", kind="openai", base_url="https://x/v1", enabled=False)
    with pytest.raises(ValueError, match="disabled"):
        registry.activate(remote.id)
    assert registry.active_id() == LOCAL_PROVIDER_ID


def test_an_unknown_id_raises(registry: ProviderRegistry) -> None:
    with pytest.raises(UnknownProvider):
        registry.get("ghost")


def test_on_device_is_decided_by_the_base_url_alone() -> None:
    """The 'on device' badge must not survive pointing the local row at another machine."""
    assert runs_on_device("http://127.0.0.1:11434")
    assert runs_on_device("http://localhost:11434")
    assert not runs_on_device("http://192.168.1.50:11434")
    assert not runs_on_device("https://api.openai.com/v1")


def test_moving_the_local_row_to_another_host_drops_the_on_device_badge(
    registry: ProviderRegistry,
) -> None:
    moved = registry.update(LOCAL_PROVIDER_ID, base_url="http://192.168.1.50:11434")
    assert moved.on_device is False
    # The registry follows the new host, so health and GPU leases go with it.
    assert registry.ollama_url == "http://192.168.1.50:11434"
