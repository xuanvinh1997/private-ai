"""LangChain models for whichever provider is currently selected.

Everything that used to be a hand-written httpx client is a ``BaseChatModel`` or an
``Embeddings`` here. The router's only real jobs are picking the model name when the
caller did not name one, pointing the LangChain class at the right host, and keeping
the constructed objects around — a chat model is not free to build, but it must also
stop being used the moment the user edits or switches the provider, which is why the
cache is keyed on the provider's whole signature rather than on its id.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any, cast
from urllib.parse import urlsplit

import httpx
from langchain_core.embeddings import Embeddings
from langchain_core.language_models import BaseChatModel
from langchain_ollama import ChatOllama, OllamaEmbeddings
from langchain_openai import ChatOpenAI, OpenAIEmbeddings

from private_ai.core.schemas import ModelInfo, ModelState
from private_ai.llm import NoProviderConfigured, ProviderUnavailable
from private_ai.llm.admin import ModelAdmin
from private_ai.llm.capabilities import infer_capabilities
from private_ai.llm.leases import OWNER_PREFIX, GpuLeaseCallback, owner_for

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from private_ai.config import Settings
    from private_ai.core.database import Database
    from private_ai.core.gpu_lease import GpuLeaseManager
    from private_ai.llm.registry import ProviderConfig, ProviderRegistry

__all__ = ["ModelRouter", "openai_base_url"]

# openai-python refuses to construct a client with an empty key, but a llama.cpp or vLLM
# server on localhost has no key to give it.
PLACEHOLDER_API_KEY = "not-needed"


def openai_base_url(base_url: str) -> str:
    """Accept either the API root or the bare host, the way the OpenAI SDKs do."""
    value = base_url.strip().rstrip("/")
    if not value:
        raise ValueError("Provider base URL is required")
    tail = value.rsplit("/", 1)[-1]
    if tail.startswith("v") and tail[1:].isdigit():
        return value
    return f"{value}/v1"


def _is_openai_cloud(base_url: str) -> bool:
    host = (urlsplit(base_url).hostname or "").lower()
    return host.endswith("openai.com")


class ModelRouter:
    """Hands out LangChain models bound to the active provider."""

    def __init__(
        self,
        registry: ProviderRegistry,
        *,
        gpu_leases: GpuLeaseManager | None = None,
        settings: Settings,
        database: Database,
    ) -> None:
        self.registry = registry
        self.gpu_leases = gpu_leases
        self.settings = settings
        self.database = database
        self.admin = ModelAdmin(
            registry,
            timeout=settings.request_timeout_seconds,
            gpu_leases=gpu_leases,
            model_overhead_ratio=settings.gpu_model_overhead_ratio,
        )
        self._chat_cache: dict[tuple[Any, ...], BaseChatModel] = {}
        self._embedding_cache: dict[tuple[Any, ...], Embeddings] = {}

    # --- resolution -------------------------------------------------------

    def active_config(self) -> ProviderConfig:
        config = self.registry.active_config()
        if config is None:
            raise NoProviderConfigured("Chưa cấu hình nhà cung cấp AI nào")
        return config

    def default_model(self, task: str) -> str:
        row = self.database.fetch_one(
            "SELECT model_name FROM model_defaults WHERE task = ?",
            (task,),
        )
        return str(row["model_name"]).strip() if row else ""

    # --- models -----------------------------------------------------------

    def chat_model(
        self,
        model: str = "",
        *,
        streaming: bool = True,
        tools: Sequence[Any] | None = None,
        **kwargs: Any,
    ) -> BaseChatModel:
        config = self.active_config()
        name = model.strip() or self.default_model("chat")
        if not name:
            raise ProviderUnavailable("Chưa chọn mô hình trò chuyện")
        base = self._cached_chat(config, name, streaming, kwargs)
        if not tools:
            return base
        # ``bind_tools`` returns a RunnableBinding rather than a model, but it invokes and
        # streams identically and every caller here only ever does those two things.
        return cast(BaseChatModel, base.bind_tools(list(tools)))

    def vision_model(self, model: str = "", **kwargs: Any) -> BaseChatModel:
        name = model.strip() or self.default_model("vision") or self.settings.vision_model
        if not name:
            raise ProviderUnavailable("Chưa chọn mô hình thị giác")
        return self.chat_model(name, streaming=False, **kwargs)

    def embeddings(self, model: str = "") -> Embeddings:
        config = self.active_config()
        name = (
            model.strip() or self.default_model("embedding") or self.settings.embedding_model
        ).strip()
        if not name:
            raise ProviderUnavailable("Chưa chọn mô hình nhúng")
        key = (config.signature, name)
        cached = self._embedding_cache.get(key)
        if cached is not None:
            return cached
        self._evict(self._embedding_cache, config)
        built = self._build_embeddings(config, name)
        self._embedding_cache[key] = built
        return built

    def _cached_chat(
        self,
        config: ProviderConfig,
        name: str,
        streaming: bool,
        kwargs: dict[str, Any],
    ) -> BaseChatModel:
        key = (config.signature, name, streaming, json.dumps(kwargs, sort_keys=True, default=str))
        cached = self._chat_cache.get(key)
        if cached is not None:
            return cached
        self._evict(self._chat_cache, config)
        built = self._build_chat(config, name, streaming, kwargs)
        self._chat_cache[key] = built
        return built

    @staticmethod
    def _evict(cache: dict[tuple[Any, ...], Any], config: ProviderConfig) -> None:
        """Drop anything built against an older shape of this provider."""
        stale = [
            key
            for key in cache
            if key[0][0] == config.id and key[0] != config.signature  # (id, kind, url, key)
        ]
        for key in stale:
            cache.pop(key, None)

    def _build_chat(
        self,
        config: ProviderConfig,
        name: str,
        streaming: bool,
        kwargs: dict[str, Any],
    ) -> BaseChatModel:
        if config.kind == "ollama":
            # ChatOllama has no ``streaming`` flag: ``astream`` streams and ``ainvoke``
            # does not. The flag still keys the cache so the two never share an instance.
            return ChatOllama(
                model=name,
                base_url=config.base_url,
                client_kwargs={"timeout": self.settings.request_timeout_seconds},
                callbacks=self._callbacks(config, name),
                **kwargs,
            )
        return ChatOpenAI(
            model=name,
            base_url=openai_base_url(config.base_url),
            api_key=config.api_key or PLACEHOLDER_API_KEY,
            timeout=self.settings.request_timeout_seconds,
            streaming=streaming,
            **kwargs,
        )

    def _build_embeddings(self, config: ProviderConfig, name: str) -> Embeddings:
        if config.kind == "ollama":
            return OllamaEmbeddings(
                model=name,
                base_url=config.base_url,
                client_kwargs={"timeout": self.settings.request_timeout_seconds},
            )
        return OpenAIEmbeddings(
            model=name,
            base_url=openai_base_url(config.base_url),
            api_key=config.api_key or PLACEHOLDER_API_KEY,
            timeout=self.settings.request_timeout_seconds,
            # Only the real API tokenizes with tiktoken; a local server wants the raw text.
            check_embedding_ctx_length=_is_openai_cloud(config.base_url),
        )

    def _callbacks(self, config: ProviderConfig, name: str) -> list[GpuLeaseCallback]:
        if not self.gpu_leases or config.kind != "ollama":
            return []
        return [GpuLeaseCallback(leases=self.gpu_leases, admin=self.admin, model=name)]

    # --- inventory --------------------------------------------------------

    async def health(self) -> bool:
        config = self.registry.active_config()
        if config is None:
            return False
        if config.kind == "ollama":
            return await self.admin.health()
        try:
            async with httpx.AsyncClient(timeout=5.0) as client:
                response = await client.get(
                    f"{openai_base_url(config.base_url)}/models",
                    headers=self._openai_headers(config),
                )
                response.raise_for_status()
        except (httpx.HTTPError, ValueError):
            return False
        return True

    async def list_models(self) -> list[ModelInfo]:
        config = self.active_config()
        if config.kind != "ollama":
            return await self._list_openai_models(config)
        models = await self.admin.list_models()
        if self.gpu_leases:
            reservations = {
                owner_for(model.name): self.admin.required_bytes(model)
                for model in models
                if model.state == ModelState.LOADED
            }
            await self.gpu_leases.synchronize(OWNER_PREFIX, reservations)
        return models

    async def _list_openai_models(self, config: ProviderConfig) -> list[ModelInfo]:
        try:
            async with httpx.AsyncClient(timeout=self.settings.request_timeout_seconds) as client:
                response = await client.get(
                    f"{openai_base_url(config.base_url)}/models",
                    headers=self._openai_headers(config),
                )
                response.raise_for_status()
                payload = response.json()
        except httpx.HTTPError as exc:
            raise ProviderUnavailable(str(exc)) from exc
        entries = payload.get("data")
        if not isinstance(entries, list):
            raise ProviderUnavailable("Provider returned an invalid model list")
        models: list[ModelInfo] = []
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            name = str(entry.get("id") or "").strip()
            if not name:
                continue
            capabilities = infer_capabilities(f"{name} {entry.get('owned_by', '')}")
            models.append(
                ModelInfo(
                    name=name,
                    model_type="embedding" if capabilities == ["embedding"] else "language",
                    state=ModelState.INSTALLED,
                    capabilities=capabilities,
                    runtime=config.name,
                )
            )
        models.sort(key=lambda model: model.name)
        return models

    @staticmethod
    def _openai_headers(config: ProviderConfig) -> dict[str, str]:
        headers = {"content-type": "application/json"}
        if config.api_key:
            headers["authorization"] = f"Bearer {config.api_key}"
        return headers
