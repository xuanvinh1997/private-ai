"""Cấu hình lúc chạy: service này lấy đâu ra endpoint, model và đường dẫn dự án.

# Ba nguồn, theo thứ tự ưu tiên tăng dần

1. **Mặc định trong mã.** Đủ để ``pai-rag doctor`` chạy được trên một máy vừa cài Ollama
   mà chưa ai cấu hình gì. Không có bước này thì lỗi đầu tiên người dùng gặp là một
   ``KeyError``, thứ không nói được gì.
2. **Tệp JSON do ứng dụng Rust ghi ra**, trỏ tới bằng ``PAI_RAG_CONFIG``. Đây là nguồn
   chính: người dùng chọn model nhúng, model rerank và model vision ở màn hình Cài đặt,
   và ứng dụng ghi lựa chọn đó xuống đây.
3. **Biến môi trường.** Đè lên cả hai. Có để gỡ lỗi và để chạy trong bài kiểm chứng mà
   không phải dựng cả một tệp cấu hình.

# Vì sao đọc lại tệp ở mỗi lần gọi

Người dùng đổi model nhúng trong Cài đặt lúc service đang chạy là chuyện thường. Đọc một
lần lúc khởi động rồi giữ trong bộ nhớ nghĩa là lựa chọn mới chỉ có hiệu lực sau khi tắt
bật lại ứng dụng — và không có gì nói cho họ biết điều đó. :func:`load` vì thế soi
``mtime`` và nạp lại khi tệp đổi; chi phí là một lần ``stat`` cho mỗi lời gọi tool.
"""

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

#: Ollama trên máy, cổng mặc định. Đây là cấu hình mà phần lớn người dùng sẽ có.
DEFAULT_OLLAMA = "http://127.0.0.1:11434"
#: Đúng model mà GOAL.md đã chọn, và đúng lý do: nó đa ngữ thật, mạnh với tiếng Việt.
#: ``nomic-embed-text`` — mặc định của tầng Rust — thiên về tiếng Anh, trong khi cả thư
#: viện tài liệu ở đây là tiếng Việt.
DEFAULT_EMBED_MODEL = "qwen3-embedding:4b"
DEFAULT_VISION_MODEL = "qwen2.5vl:7b"
DEFAULT_CHAT_MODEL = "qwen3:8b"
#: Reranker mặc định: ``bge-reranker-v2-m3``, bản ONNX.
#:
#: # Vì sao không phải ``bge-reranker-base``, dù nó là artifact chính chủ của BAAI
#:
#: Vì đo rồi: trên bộ câu hỏi tiếng Việt dùng để kiểm chứng, ``bge-reranker-base`` **làm
#: kết quả tệ đi** — nó đảo đúng thành sai ở những câu cần suy luận ngữ nghĩa thay vì
#: khớp từ (top-1 5/7, MRR 0.810), trong khi chỉ dùng RRF không rerank đạt 7/7. Một bước
#: "làm tốt hơn" mà làm tệ đi thì tệ hơn cả việc không có nó. ``v2-m3`` đạt 7/7 và sửa
#: đúng hai câu ``base`` làm hỏng.
#:
#: # Cái giá phải nói ra
#:
#: Repo chính thức của v2-m3 **không có** ONNX; đây là bản export của cộng đồng. Đó là
#: một rủi ro nguồn cung thật — repo có thể biến mất, và không ai ký nó. Đổi lại là một
#: reranker thực sự đọc được tiếng Việt. Nếu bạn cần chắc chắn hơn, hãy nhân bản repo này
#: về tài khoản của mình rồi trỏ ``rerank.model`` vào bản nhân bản đó.
#:
#: Trọng số nằm ở tệp ``model.onnx_data`` tách rời, 2,27 GB — xem
#: ``OnnxReranker._fetch_external_data`` và lệnh ``pai-rag doctor`` để tải sẵn.
DEFAULT_RERANK_MODEL = "viplao5/bge-reranker-v2-m3-onnx"
#: Tệp ONNX bên trong repo mặc định. Repo chính chủ của BAAI đặt ở ``onnx/model.onnx``;
#: bản export này đặt ở gốc. Hai chỗ khác nhau nên nó phải là một tuỳ chọn, không phải
#: một hằng số ngầm.
DEFAULT_RERANK_ONNX_FILE = "model.onnx"


class ProviderConfig(BaseModel):
    """Một endpoint nói giao thức Ollama hoặc giao thức OpenAI."""

    kind: ProviderKind = "ollama"
    base_url: str = DEFAULT_OLLAMA
    api_key: str = ""
    model: str = ""
    #: Số chiều, khi cấu hình biết. ``None`` là hợp lệ: nhiều máy chủ chỉ nói ra số
    #: chiều bằng cách trả về một vector, và bắt biết trước thì phải gọi thử một lần chỉ
    #: để hỏi.
    dim: int | None = None

    def root(self) -> str:
        """Gốc máy chủ, đã cắt đuôi ``/v1`` nếu có.

        Kho cấu hình phía Rust lưu URL theo dạng tầng hội thoại mong đợi, và phần lớn
        mục có đuôi ``/v1``. Để nguyên thì mọi request nhúng bay tới
        ``/v1/v1/embeddings`` và trả về 404 mà không ai đoán ra vì sao.
        """
        value = self.base_url.strip().rstrip("/")
        tail = value.rsplit("/", 1)[-1]
        if tail.startswith("v") and len(tail) > 1 and tail[1:].isdigit():
            return value[: -len(tail)].rstrip("/")
        return value


class RerankConfig(BaseModel):
    """Xếp hạng lại. Tắt được, và tắt thì truy hồi vẫn chạy — chỉ kém đi."""

    enabled: bool = True
    #: ``onnx`` chạy trong tiến trình này; ``http`` gọi ra một endpoint ``/rerank``.
    backend: Literal["onnx", "http"] = "onnx"
    model: str = DEFAULT_RERANK_MODEL
    #: Đường dẫn tệp ONNX bên trong repo HuggingFace. Khác nhau giữa các repo —
    #: xem :data:`DEFAULT_RERANK_ONNX_FILE`.
    onnx_file: str = DEFAULT_RERANK_ONNX_FILE
    #: Chỗ giữ model đã tải. ``None`` là dùng cache mặc định của huggingface-hub.
    cache_dir: str | None = None
    #: Lấy về bao nhiêu ứng viên trước khi xếp lại, và giữ lại bao nhiêu sau đó.
    candidates: int = 30
    top_n: int = 8
    #: Chỉ dùng khi ``backend == "http"``.
    url: str = ""
    api_key: str = ""


class VectorConfig(BaseModel):
    """Qdrant. Chạy ngoài tiến trình — xem ``services/rag/deploy/``."""

    url: str = "http://127.0.0.1:6333"
    api_key: str = ""
    #: Mỗi dự án một collection. Dùng chung một collection thì một câu hỏi trong dự án
    #: này trả về đoạn của dự án khác — trông y hệt một câu trả lời sai bình thường, nên
    #: không ai lần ra nguyên nhân.
    collection_prefix: str = "pai_docs"


class GraphConfig(BaseModel):
    """Neo4j. Tắt được: chiến lược graph vắng mặt thì ``auto`` lùi về ``hybrid``."""

    enabled: bool = True
    uri: str = "bolt://127.0.0.1:7687"
    user: str = "neo4j"
    password: str = ""
    database: str = "neo4j"


class ChunkConfig(BaseModel):
    """Đơn vị là **ký tự**, không phải byte: một trần tính bằng byte khiến đoạn tiếng
    Việt ngắn hơn đoạn tiếng Anh khoảng một phần ba, mà cửa sổ ngữ cảnh thì đếm token."""

    size: int = 1400
    overlap: int = 180


class OcrConfig(BaseModel):
    enabled: bool = True
    #: Số ký tự trung bình mỗi trang, dưới ngưỡng này thì lớp chữ của PDF không đáng tin
    #: và cascade OCR tiếp quản. Một trang chữ in chạy tới vài nghìn ký tự; một trang
    #: quét cho ra gần như không gì. 200 nằm xa dưới mọi trang chữ thật và xa trên một
    #: trang trống, nên một bài báo nhiều hình vẫn được tính là dày chữ.
    min_chars_per_page: int = 200
    #: Trần số trang đưa qua VLM cho một tệp. Một cuốn sách 800 trang quét sẽ chạy hàng
    #: giờ và chiếm hết GPU; trần này cắt ở chỗ vẫn còn dùng được.
    max_pages: int = 120
    #: Độ phân giải dựng ảnh trang. 2.0 ≈ 144 DPI — đủ để VLM đọc chữ nhỏ, chưa tới mức
    #: một trang thành vài megabyte base64.
    scale: float = 2.0


class ProjectConfig(BaseModel):
    """Một dự án tài liệu: thư mục người dùng chọn, và một mã ổn định để đặt tên
    collection cùng nhãn graph.

    Service chạy **trên máy người dùng**, không trong container, nên ở đây chỉ có một
    đường dẫn: thứ ứng dụng thấy và thứ service đọc là cùng một chỗ. Đó là lý do chính
    khiến bản stdio đơn giản hơn hẳn bản chạy trong container — không có phép ánh xạ
    đường dẫn nào phải giữ đúng ở hai đầu.
    """

    id: str
    name: str = ""
    root: str

    def local_root(self) -> Path:
        return Path(self.root)


class RagConfig(BaseModel):
    version: int = 1
    #: Chỗ service đặt cơ sở dữ liệu siêu dữ liệu của nó.
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
        """Dự án theo mã, hoặc dự án đang mở khi không nói mã.

        Ném :class:`ConfigError` kèm **danh sách mã có thật** thay vì chỉ nói không tìm
        thấy: mô hình gọi tool này và cần biết nó gõ được gì ở lần sau.
        """
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
        """Tệp SQLite giữ tài liệu, đoạn và chỉ mục từ khoá của một dự án."""
        base = Path(self.data_dir) if self.data_dir else default_data_dir()
        return base / project.id / "rag.sqlite"

    def collection(self, project: ProjectConfig) -> str:
        return f"{self.vectors.collection_prefix}_{project.id}"


def default_data_dir() -> Path:
    """Chỗ đặt dữ liệu khi không ai nói, theo quy ước của từng hệ điều hành."""
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        return Path(base) / "private-ai" / "rag"
    xdg = os.environ.get("XDG_DATA_HOME")
    if xdg:
        return Path(xdg) / "private-ai" / "rag"
    return Path.home() / ".local" / "share" / "private-ai" / "rag"


def _apply_env(data: dict[str, Any]) -> dict[str, Any]:
    """Đè biến môi trường lên cấu hình đã đọc từ tệp.

    Chỉ những khoá thật sự cần chỉnh từ ngoài: điểm cuối hạ tầng và bí mật. Phơi cả cây
    cấu hình ra thành biến môi trường là dựng một giao diện thứ hai phải bảo trì song
    song với tệp JSON.
    """

    def put(section: str, key: str, value: str | None) -> None:
        if not value:
            return
        data.setdefault(section, {})[key] = value

    put("vectors", "url", os.environ.get("PAI_RAG_QDRANT_URL"))
    put("vectors", "api_key", os.environ.get("PAI_RAG_QDRANT_API_KEY"))
    put("graph", "uri", os.environ.get("PAI_RAG_NEO4J_URI"))
    put("graph", "user", os.environ.get("PAI_RAG_NEO4J_USER"))
    put("graph", "password", os.environ.get("PAI_RAG_NEO4J_PASSWORD"))
    put("graph", "database", os.environ.get("PAI_RAG_NEO4J_DATABASE"))
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
        # Một dự án khai bằng biến môi trường. Đường dùng cho CLI và cho bài kiểm chứng:
        # chạy được mà không cần ứng dụng Rust ghi tệp cấu hình ra trước.
        projects = [item for item in data.get("projects", []) if item.get("id") != project]
        projects.append({"id": project, "name": Path(root).name, "root": root})
        data["projects"] = projects
    return data


_lock = threading.Lock()
_cached: tuple[Path | None, float, RagConfig] | None = None


def reset_cache() -> None:
    """Quên cấu hình đã nhớ. Bài kiểm chứng gọi cái này giữa hai trường hợp."""
    global _cached
    with _lock:
        _cached = None


def load(path: str | Path | None = None) -> RagConfig:
    """Cấu hình hiện hành, nạp lại khi tệp trên đĩa đã đổi."""
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
