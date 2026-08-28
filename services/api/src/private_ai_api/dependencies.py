from __future__ import annotations

from dataclasses import dataclass

from fastapi import Request

from private_ai_api.config import Settings
from private_ai_api.database import Database
from private_ai_api.services.asr import AsrService
from private_ai_api.services.document_processor import DocumentProcessor
from private_ai_api.services.gpu_lease import GpuLeaseManager
from private_ai_api.services.lightrag_store import LightRagStore
from private_ai_api.services.memory_service import MemoryService
from private_ai_api.services.ollama import OllamaClient
from private_ai_api.services.provider_registry import ProviderRegistry, ProviderRouter
from private_ai_api.services.tool_calling import McpToolBridge
from private_ai_api.services.web_search import WebSearchService


@dataclass(slots=True)
class AppServices:
    settings: Settings
    database: Database
    ollama: OllamaClient
    providers: ProviderRegistry
    ai: ProviderRouter
    gpu_leases: GpuLeaseManager
    document_processor: DocumentProcessor
    lightrag: LightRagStore
    memory_service: MemoryService
    asr: AsrService
    web_search: WebSearchService
    # Filled in after the services exist, because the tool server is built on top of them.
    tools: McpToolBridge | None = None


def get_services(request: Request) -> AppServices:
    return request.app.state.services
