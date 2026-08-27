from __future__ import annotations

from dataclasses import dataclass

from fastapi import Request

from private_ai_api.config import Settings
from private_ai_api.database import Database
from private_ai_api.services.asr import AsrService
from private_ai_api.services.document_processor import DocumentProcessor
from private_ai_api.services.gpu_lease import GpuLeaseManager
from private_ai_api.services.graph_store import GraphStore
from private_ai_api.services.memory_service import MemoryService
from private_ai_api.services.ollama import OllamaClient


@dataclass(slots=True)
class AppServices:
    settings: Settings
    database: Database
    ollama: OllamaClient
    gpu_leases: GpuLeaseManager
    document_processor: DocumentProcessor
    graph_store: GraphStore
    memory_service: MemoryService
    asr: AsrService


def get_services(request: Request) -> AppServices:
    return request.app.state.services
