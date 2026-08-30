"""The configured AI hosts.

This owns rows, not clients: it says *where* to send a request and with what key, and
``llm.router`` turns that into a LangChain model. Splitting the two is what lets the
router cache a model per provider signature while the registry stays a thin SQLite view.
"""

from __future__ import annotations

import sqlite3
from contextlib import suppress
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any
from urllib.parse import urlsplit
from uuid import uuid4

from private_ai.core.schemas import ProviderKind
from private_ai.llm import UnknownProvider

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.config import Settings
    from private_ai.core.database import Database

__all__ = [
    "ACTIVE_PROVIDER_KEY",
    "LOCAL_PROVIDER_ID",
    "PROVIDER_KINDS",
    "ProviderConfig",
    "ProviderRegistry",
    "runs_on_device",
]

PROVIDER_KINDS = tuple(kind.value for kind in ProviderKind)
LOOPBACK_HOSTS = frozenset({"localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0"})  # noqa: S104

LOCAL_PROVIDER_ID = "local-ollama"
LOCAL_PROVIDER_NAME = "Ollama cục bộ"
ACTIVE_PROVIDER_KEY = "active_provider_id"
SEEDED_KEY = "providers_seeded"


def runs_on_device(base_url: str) -> bool:
    """A provider is on-device when its endpoint never leaves the loopback interface."""
    host = urlsplit(base_url).hostname
    return host is not None and host.lower() in LOOPBACK_HOSTS


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

    @property
    def signature(self) -> tuple[str, str, str, str]:
        """Everything a constructed client depends on, so a cache can key on it."""
        return (self.id, self.kind, self.base_url, self.api_key)

    @property
    def on_device(self) -> bool:
        return runs_on_device(self.base_url)

    def public(self, *, active: bool) -> dict[str, Any]:
        """Shape for the UI and MCP: never leaks the key, only whether one is stored."""
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
    """Owns the configured AI hosts and which one is selected.

    The local Ollama install is seeded once on a fresh database so the app works out of the
    box, but it is an ordinary row from then on: the user may rename it, move it to another
    host, or delete it outright once they have somewhere else to send requests.
    """

    def __init__(self, database: Database, *, settings: Settings) -> None:
        self.database = database
        self.settings = settings
        self.ollama_url = settings.ollama_url
        # A database that has not been migrated yet must not stop the process from starting;
        # the seed is retried on the next construction.
        with suppress(sqlite3.Error):
            self._seed_local()

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

    def public_list(self) -> list[dict[str, Any]]:
        active = self.active_id()
        return [config.public(active=config.id == active) for config in self.list_configs()]

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
        if updated.id == LOCAL_PROVIDER_ID:
            self.ollama_url = updated.base_url
        return self.get(provider_id)

    @staticmethod
    def _next_key(current: ProviderConfig, api_key: str | None) -> str:
        # An Ollama host authenticates by nothing, so it never carries a key.
        if current.kind == "ollama":
            return ""
        return current.api_key if api_key is None else api_key.strip()

    def delete(self, provider_id: str) -> None:
        self.get(provider_id)
        # Read the selection first: once the row is gone the lookup reports the fallback.
        was_active = self.active_id() == provider_id
        self.database.execute("DELETE FROM ai_providers WHERE id = ?", (provider_id,))
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
