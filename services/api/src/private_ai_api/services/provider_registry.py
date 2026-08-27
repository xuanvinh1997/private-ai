from __future__ import annotations

import sqlite3
from collections.abc import AsyncIterator
from contextlib import suppress
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any
from uuid import uuid4

import httpx

from private_ai_api.database import Database
from private_ai_api.schemas import ChatRequest, ModelInfo
from private_ai_api.services.ollama import OllamaClient
from private_ai_api.services.openai_compat import OpenAICompatClient
from private_ai_api.services.provider import PROVIDER_KINDS, NoProviderConfigured

LOCAL_PROVIDER_ID = "local-ollama"
LOCAL_PROVIDER_NAME = "Ollama cục bộ"
ACTIVE_PROVIDER_KEY = "active_provider_id"
SEEDED_KEY = "providers_seeded"


class UnknownProvider(LookupError):
    pass


@dataclass(frozen=True, slots=True)
class ProviderConfig:
    id: str
    name: str
    kind: str
    base_url: str
    api_key: str = ""
    enabled: bool = True
    builtin: bool = False
    created_at: str | None = None
    updated_at: str | None = None

    def public(self, *, active: bool) -> dict[str, Any]:
        """Shape for the API: never leaks the key, only whether one is stored."""
        return {
            "id": self.id,
            "name": self.name,
            "kind": self.kind,
            "base_url": self.base_url,
            "has_api_key": bool(self.api_key),
            "enabled": self.enabled,
            "builtin": self.builtin,
            "active": active,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        }


class ProviderRegistry:
    """Owns the configured AI hosts and hands out a client for the selected one.

    The local Ollama install is seeded once on a fresh database so the app works out of the
    box, but it is an ordinary row from then on: the user may rename it, move it to another
    host, or delete it outright once they have somewhere else to send requests.
    """

    def __init__(
        self,
        database: Database,
        *,
        ollama: OllamaClient,
        ollama_url: str,
        timeout: float = 60.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self.database = database
        self.ollama = ollama
        self.ollama_url = ollama_url
        self.timeout = timeout
        self.transport = transport
        self._clients: dict[str, OpenAICompatClient] = {}
        self._signatures: dict[str, tuple[str, str, str]] = {}
        with suppress(sqlite3.Error):
            self._seed_local()
            local = self.database.fetch_one(
                "SELECT base_url FROM ai_providers WHERE id = ?",
                (LOCAL_PROVIDER_ID,),
            )
            # The user may have moved the local install, and the shared client has to follow
            # before anything reaches the old host.
            if local:
                self._retarget_local(str(local["base_url"]))

    def _seed_local(self) -> None:
        """Write the local install once, and never resurrect it after a deliberate delete."""
        marked = self.database.fetch_one(
            "SELECT 1 AS present FROM app_state WHERE key = ?",
            (SEEDED_KEY,),
        )
        if marked:
            return
        now = datetime.now(UTC).isoformat()
        self.database.execute(
            "INSERT OR REPLACE INTO app_state(key, value) VALUES (?, ?)",
            (SEEDED_KEY, now),
        )
        self.database.execute(
            """
            INSERT OR IGNORE INTO ai_providers(
                id, name, kind, base_url, api_key, enabled, created_at, updated_at
            ) VALUES (?, ?, 'ollama', ?, '', 1, ?, ?)
            """,
            (LOCAL_PROVIDER_ID, LOCAL_PROVIDER_NAME, self.ollama_url, now, now),
        )

    def list_configs(self) -> list[ProviderConfig]:
        rows = self.database.fetch_all("SELECT * FROM ai_providers ORDER BY created_at ASC")
        return [self._config(row) for row in rows]

    def get(self, provider_id: str) -> ProviderConfig:
        row = self.database.fetch_one("SELECT * FROM ai_providers WHERE id = ?", (provider_id,))
        if not row:
            raise UnknownProvider(provider_id)
        return self._config(row)

    def active_id(self) -> str:
        """The stored pick, or the first one left; empty once every provider is gone."""
        config = self.active_config()
        return config.id if config else ""

    def active_config(self) -> ProviderConfig | None:
        configured = self.list_configs()
        if not configured:
            return None
        row = self.database.fetch_one(
            "SELECT value FROM app_state WHERE key = ?",
            (ACTIVE_PROVIDER_KEY,),
        )
        selected = str(row["value"]).strip() if row else ""
        chosen = next(
            (item for item in configured if item.id == selected and item.enabled),
            None,
        )
        return chosen or next((item for item in configured if item.enabled), configured[0])

    def client_for(self, config: ProviderConfig) -> OllamaClient | OpenAICompatClient:
        # Only the local install shares the lease-aware client that tracks GPU memory.
        if config.kind == "ollama" and config.id == LOCAL_PROVIDER_ID:
            return self.ollama
        if config.kind == "ollama":
            return OllamaClient(config.base_url, self.timeout, transport=self.transport)
        cached = self._clients.get(config.id)
        signature = (config.base_url, config.api_key, config.name)
        if cached is not None and self._signatures.get(config.id) == signature:
            return cached
        client = OpenAICompatClient(
            config.base_url,
            config.api_key,
            self.timeout,
            label=config.name,
            transport=self.transport,
        )
        self._clients[config.id] = client
        self._signatures[config.id] = signature
        return client

    def active_client(self) -> OllamaClient | OpenAICompatClient:
        config = self.active_config()
        if config is None:
            raise NoProviderConfigured("No AI provider is configured")
        return self.client_for(config)

    def create(
        self,
        *,
        name: str,
        kind: str,
        base_url: str,
        api_key: str = "",
        enabled: bool = True,
    ) -> ProviderConfig:
        self._validate(kind, base_url)
        now = datetime.now(UTC).isoformat()
        provider_id = str(uuid4())
        self.database.execute(
            """
            INSERT INTO ai_providers(
                id, name, kind, base_url, api_key, enabled, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                provider_id,
                name.strip(),
                kind,
                base_url.strip(),
                api_key.strip(),
                int(enabled),
                now,
                now,
            ),
        )
        return self.get(provider_id)

    def update(
        self,
        provider_id: str,
        *,
        name: str | None = None,
        base_url: str | None = None,
        api_key: str | None = None,
        enabled: bool | None = None,
    ) -> ProviderConfig:
        current = self.get(provider_id)
        updated = ProviderConfig(
            id=current.id,
            name=(name or current.name).strip(),
            kind=current.kind,
            base_url=(base_url or current.base_url).strip(),
            api_key=self._next_key(current, api_key),
            enabled=current.enabled if enabled is None else enabled,
            builtin=current.builtin,
        )
        self._validate(updated.kind, updated.base_url)
        now = datetime.now(UTC).isoformat()
        self.database.execute(
            """
            INSERT INTO ai_providers(
                id, name, kind, base_url, api_key, enabled, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                base_url = excluded.base_url,
                api_key = excluded.api_key,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            """,
            (
                updated.id,
                updated.name,
                updated.kind,
                updated.base_url,
                updated.api_key,
                int(updated.enabled),
                current.created_at or now,
                now,
            ),
        )
        self._forget(provider_id)
        if updated.id == LOCAL_PROVIDER_ID:
            self._retarget_local(updated.base_url)
        return self.get(provider_id)

    @staticmethod
    def _next_key(current: ProviderConfig, api_key: str | None) -> str:
        # An Ollama host authenticates by nothing, so it never carries a key.
        if current.kind == "ollama":
            return ""
        return current.api_key if api_key is None else api_key.strip()

    def _retarget_local(self, base_url: str) -> None:
        """Move the shared local client, so GPU leases and health follow the new host."""
        self.ollama_url = base_url
        self.ollama.base_url = base_url.rstrip("/")

    def delete(self, provider_id: str) -> None:
        self.get(provider_id)
        # Read the selection first: once the row is gone the lookup reports the fallback.
        was_active = self.active_id() == provider_id
        self.database.execute("DELETE FROM ai_providers WHERE id = ?", (provider_id,))
        self._forget(provider_id)
        if not was_active:
            return
        # Point at whatever is left, or clear the pick entirely when nothing is.
        remaining = self.active_config()
        self.database.execute(
            """
            INSERT INTO app_state(key, value) VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            """,
            (ACTIVE_PROVIDER_KEY, remaining.id if remaining else ""),
        )

    def activate(self, provider_id: str) -> ProviderConfig:
        config = self.get(provider_id)
        if not config.enabled:
            raise ValueError("A disabled provider cannot be activated")
        self.database.execute(
            """
            INSERT INTO app_state(key, value) VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            """,
            (ACTIVE_PROVIDER_KEY, config.id),
        )
        return config

    @staticmethod
    def _validate(kind: str, base_url: str) -> None:
        if kind not in PROVIDER_KINDS:
            raise ValueError(f"Unsupported provider kind: {kind}")
        if not base_url.strip().startswith(("http://", "https://")):
            raise ValueError("Provider base URL must start with http:// or https://")

    def _forget(self, provider_id: str) -> None:
        self._clients.pop(provider_id, None)
        self._signatures.pop(provider_id, None)

    @staticmethod
    def _config(row: dict[str, Any]) -> ProviderConfig:
        return ProviderConfig(
            id=str(row["id"]),
            name=str(row["name"]),
            kind=str(row["kind"]),
            base_url=str(row["base_url"]),
            api_key=str(row["api_key"] or ""),
            enabled=bool(row["enabled"]),
            builtin=str(row["id"]) == LOCAL_PROVIDER_ID,
            created_at=str(row["created_at"]),
            updated_at=str(row["updated_at"]),
        )


class ProviderRouter:
    """Forwards every model call to whichever provider is currently selected."""

    def __init__(self, registry: ProviderRegistry) -> None:
        self.registry = registry

    @property
    def client(self) -> OllamaClient | OpenAICompatClient:
        return self.registry.active_client()

    async def health(self) -> bool:
        return await self.client.health()

    async def list_models(self) -> list[ModelInfo]:
        return await self.client.list_models()

    async def chat(self, request: ChatRequest) -> dict[str, Any]:
        return await self.client.chat(request)

    async def chat_stream(self, request: ChatRequest) -> AsyncIterator[dict[str, Any]]:
        async for event in self.client.chat_stream(request):
            yield event

    async def embed(self, model: str, inputs: list[str]) -> list[list[float]]:
        return await self.client.embed(model, inputs)

    async def extract_graph(self, model: str, content: str) -> dict[str, list[dict[str, str]]]:
        return await self.client.extract_graph(model, content)

    async def pull(self, name: str) -> AsyncIterator[dict[str, Any]]:
        async for event in self.client.pull(name):
            yield event

    async def unload(self, name: str) -> None:
        await self.client.unload(name)

    async def delete(self, name: str) -> None:
        await self.client.delete(name)
