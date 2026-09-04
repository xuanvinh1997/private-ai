"""Runtime configuration: where this service gets endpoints, models and project paths.
Three sources in rising priority: code defaults, the JSON file the Rust app writes
(`PAI_RAG_CONFIG`), then environment variables. Reloaded on `mtime` change per call."""

from __future__ import annotations

import json
import os
import threading
from pathlib import Path
from typing import Any, Literal

from pydantic import BaseModel, Field

from pai_rag_service.errors import ConfigError

__all__ = [
    "ChunkConfig",
    "GraphConfig",
    "OcrConfig",
    "ProjectConfig",
    "ProviderConfig",
    "RagConfig",
    "RerankConfig",
    "VectorConfig",
    "load",
    "reset_cache",
]

ProviderKind = Literal["ollama", "openai"]

#: Local Ollama on its default port. The setup most users will have.
DEFAULT_OLLAMA = "http://127.0.0.1:11434"
#: Genuinely multilingual and strong on Vietnamese; `nomic-embed-text` leans English.
DEFAULT_EMBED_MODEL = "qwen3-embedding:4b"
DEFAULT_VISION_MODEL = "qwen2.5vl:7b"
DEFAULT_CHAT_MODEL = "qwen3:8b"
#: Default reranker, ONNX build of `bge-reranker-v2-m3`: measured on the Vietnamese eval set `bge-reranker-base` made results *worse* (5/7) while v2-m3 scores 7/7. Its ONNX is a community export - a real supply risk - and the 2.27 GB weights sit in a separate `model.onnx_data`.
DEFAULT_RERANK_MODEL = "viplao5/bge-reranker-v2-m3-onnx"
#: The ONNX file inside the default repo; BAAI puts it under `onnx/`, this export at the root, so it must be an option.
DEFAULT_RERANK_ONNX_FILE = "model.onnx"


class ProviderConfig(BaseModel):
    """An endpoint speaking either the Ollama or the OpenAI protocol."""

    kind: ProviderKind = "ollama"
    base_url: str = DEFAULT_OLLAMA
    api_key: str = ""
    model: str = ""
    #: Dimension when config knows it; `None` is valid, since many servers only reveal it by returning a vector.
    dim: int | None = None

    def root(self) -> str:
        """Server root with any `/v1` suffix stripped, or embedding requests would hit `/v1/v1/embeddings` and 404 inexplicably."""
        value = self.base_url.strip().rstrip("/")
        tail = value.rsplit("/", 1)[-1]
        if tail.startswith("v") and len(tail) > 1 and tail[1:].isdigit():
            return value[: -len(tail)].rstrip("/")
        return value


class RerankConfig(BaseModel):
    """Reranking. Can be turned off, and retrieval still works - just worse."""

    enabled: bool = True
    #: `onnx` runs in this process; `http` calls out to a `/rerank` endpoint.
    backend: Literal["onnx", "http"] = "onnx"
    model: str = DEFAULT_RERANK_MODEL
    #: Path of the ONNX file inside the HuggingFace repo; differs per repo - see :data:`DEFAULT_RERANK_ONNX_FILE`.
    onnx_file: str = DEFAULT_RERANK_ONNX_FILE
    #: Where downloaded models are kept. `None` uses huggingface-hub's default cache.
    cache_dir: str | None = None
    #: How many candidates to fetch before rescoring, and how many to keep after.
    candidates: int = 30
    top_n: int = 8
    #: Only used when `backend == "http"`.
    url: str = ""
    api_key: str = ""


class VectorConfig(BaseModel):
    """Qdrant. Runs out of process - see `services/rag/deploy/`."""

    url: str = "http://127.0.0.1:6333"
    api_key: str = ""
    #: One collection per project; sharing one would return another project's chunks, which looks exactly like an ordinary wrong answer.
    collection_prefix: str = "pai_docs"


class GraphConfig(BaseModel):
    """SurrealDB, embedded in this process, and optional: without it `auto` falls back to `hybrid`. A directory on disk rather than a server, one store per project."""

    enabled: bool = True
    #: Connection string. Empty means embedded; `ws://127.0.0.1:<port>` when the app owns a `surreal` process, since `surrealkv` locks the directory exclusively.
    url: str = ""
    namespace: str = "pai"
    #: Empty means the project id. Two libraries sharing one database answer for each other.
    database: str = ""


class ChunkConfig(BaseModel):
    """Sizes are in *characters*, not bytes: a byte cap would make Vietnamese chunks about a third shorter."""

    size: int = 1400
    overlap: int = 180


class OcrConfig(BaseModel):
    enabled: bool = True
    #: Average characters per page below which a PDF's text layer is untrustworthy and the OCR cascade takes over.
    min_chars_per_page: int = 200
    #: Cap on pages sent to the VLM per file; an 800-page scan would run for hours and occupy the GPU.
    max_pages: int = 120
    #: Page render scale. 2.0 is about 144 DPI: enough for small print, short of megabytes of base64 per page.
    scale: float = 2.0


class ProjectConfig(BaseModel):
    """A document project: the folder the user picked, plus a stable id for collection names and graph labels; the service runs on the user's machine, so there is only one path."""

    id: str
    name: str = ""
    root: str

    def local_root(self) -> Path:
        return Path(self.root)


class RagConfig(BaseModel):
    version: int = 1
    #: Where the service keeps its metadata database.
    data_dir: str = ""
    projects: list[ProjectConfig] = Field(default_factory=list)
    active_project: str = ""

    embedding: ProviderConfig = Field(
        default_factory=lambda: ProviderConfig(model=DEFAULT_EMBED_MODEL)
    )
    vision: ProviderConfig = Field(
        default_factory=lambda: ProviderConfig(model=DEFAULT_VISION_MODEL)
    )
    chat: ProviderConfig = Field(default_factory=lambda: ProviderConfig(model=DEFAULT_CHAT_MODEL))

    rerank: RerankConfig = Field(default_factory=RerankConfig)
    vectors: VectorConfig = Field(default_factory=VectorConfig)
    graph: GraphConfig = Field(default_factory=GraphConfig)
    chunk: ChunkConfig = Field(default_factory=ChunkConfig)
    ocr: OcrConfig = Field(default_factory=OcrConfig)

    def project(self, key: str = "") -> ProjectConfig:
        """The project by id, or the active one; raises with the list of real ids, because the model calling this needs to know what it may type next time."""
        wanted = (key or self.active_project).strip()
        if not wanted:
            raise ConfigError(
                "chưa có dự án nào đang mở. Mở một dự án tài liệu trong ứng dụng, "
                "hoặc truyền `project` vào lời gọi."
            )
        for item in self.projects:
            if item.id == wanted:
                return item
        known = ", ".join(item.id for item in self.projects) or "(chưa có dự án nào)"
        raise ConfigError(f"không có dự án `{wanted}`. Đang có: {known}")

    def store_path(self, project: ProjectConfig) -> Path:
        """The SQLite file holding a project's documents, chunks and keyword index."""
        base = Path(self.data_dir) if self.data_dir else default_data_dir()
        return base / project.id / "rag.sqlite"

    def graph_path(self, project: ProjectConfig) -> Path:
        """The SurrealDB directory for a project's entity graph when embedded; beside `rag.sqlite`, so deleting a project stays a directory delete."""
        return self.store_path(project).parent / "graph"

    def graph_url(self, project: ProjectConfig) -> str:
        """How to reach a project's graph. The app sets `graph.url` when it runs its own `surreal`; without it the service opens the directory in-process. Same SDK, same store class - only this string differs."""
        return self.graph.url or f"surrealkv://{self.graph_path(project)}"

    def graph_database(self, project: ProjectConfig) -> str:
        return self.graph.database or project.id

    def collection(self, project: ProjectConfig) -> str:
        return f"{self.vectors.collection_prefix}_{project.id}"


def default_data_dir() -> Path:
    """Where data goes when nobody says, per OS convention."""
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        return Path(base) / "private-ai" / "rag"
    xdg = os.environ.get("XDG_DATA_HOME")
    if xdg:
        return Path(xdg) / "private-ai" / "rag"
    return Path.home() / ".local" / "share" / "private-ai" / "rag"


def _apply_env(data: dict[str, Any]) -> dict[str, Any]:
    """Overlay environment variables onto the file config; only infrastructure endpoints and secrets, not the whole tree."""

    def put(section: str, key: str, value: str | None) -> None:
        if not value:
            return
        data.setdefault(section, {})[key] = value

    put("vectors", "url", os.environ.get("PAI_RAG_QDRANT_URL"))
    put("vectors", "api_key", os.environ.get("PAI_RAG_QDRANT_API_KEY"))
    put("graph", "url", os.environ.get("PAI_RAG_GRAPH_URL"))
    put("graph", "namespace", os.environ.get("PAI_RAG_GRAPH_NAMESPACE"))
    put("graph", "database", os.environ.get("PAI_RAG_GRAPH_DATABASE"))
    put("embedding", "base_url", os.environ.get("PAI_RAG_EMBED_URL"))
    put("embedding", "model", os.environ.get("PAI_RAG_EMBED_MODEL"))
    put("vision", "base_url", os.environ.get("PAI_RAG_VISION_URL"))
    put("vision", "model", os.environ.get("PAI_RAG_VISION_MODEL"))
    put("chat", "base_url", os.environ.get("PAI_RAG_CHAT_URL"))
    put("chat", "model", os.environ.get("PAI_RAG_CHAT_MODEL"))
    put("rerank", "model", os.environ.get("PAI_RAG_RERANK_MODEL"))

    off = {"0", "false", "no"}
    if os.environ.get("PAI_RAG_GRAPH_ENABLED", "").lower() in off:
        data.setdefault("graph", {})["enabled"] = False
    if os.environ.get("PAI_RAG_RERANK_ENABLED", "").lower() in off:
        data.setdefault("rerank", {})["enabled"] = False
    if os.environ.get("PAI_RAG_OCR_ENABLED", "").lower() in off:
        data.setdefault("ocr", {})["enabled"] = False

    data_dir = os.environ.get("PAI_RAG_DATA_DIR")
    if data_dir:
        data["data_dir"] = data_dir
    project = os.environ.get("PAI_RAG_PROJECT")
    if project:
        data["active_project"] = project
    root = os.environ.get("PAI_RAG_PROJECT_ROOT")
    if root and project:
        # A project declared purely through the environment: the path for the CLI and the tests, with no config file written first.
        projects = [item for item in data.get("projects", []) if item.get("id") != project]
        projects.append({"id": project, "name": Path(root).name, "root": root})
        data["projects"] = projects
    return data


_lock = threading.Lock()
_cached: tuple[Path | None, float, RagConfig] | None = None


def reset_cache() -> None:
    """Forget the cached config. The tests call this between cases."""
    global _cached
    with _lock:
        _cached = None


def load(path: str | Path | None = None) -> RagConfig:
    """The current config, reloaded when the file on disk has changed."""
    global _cached
    target = Path(path) if path else None
    if target is None:
        env_path = os.environ.get("PAI_RAG_CONFIG")
        target = Path(env_path) if env_path else None

    stamp = 0.0
    if target is not None and target.is_file():
        stamp = target.stat().st_mtime

    with _lock:
        if _cached is not None:
            seen_path, seen_stamp, config = _cached
            if seen_path == target and seen_stamp == stamp:
                return config

        data: dict[str, Any] = {}
        if target is not None:
            if not target.is_file():
                raise ConfigError(
                    f"không thấy tệp cấu hình `{target}`. Biến PAI_RAG_CONFIG đang trỏ "
                    "vào một chỗ không có tệp nào."
                )
            try:
                data = json.loads(target.read_text(encoding="utf-8"))
            except json.JSONDecodeError as err:
                raise ConfigError(
                    f"tệp cấu hình `{target}` không phải JSON hợp lệ: {err}"
                ) from err
            if not isinstance(data, dict):
                raise ConfigError(f"tệp cấu hình `{target}` phải là một object JSON")

        config = RagConfig.model_validate(_apply_env(data))
        _cached = (target, stamp, config)
        return config
