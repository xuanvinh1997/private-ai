"""Reranker gọi ra một endpoint ``/v1/rerank``.

# Chuẩn nào

Hình dạng của Cohere, thứ đã thành chuẩn thực tế: gửi ``{model, query, documents,
top_n}``, nhận ``{results: [{index, relevance_score}]}``. Text Embeddings Inference của
HuggingFace, Infinity, vLLM, Jina và Voyage đều nói giao thức này.

**Ollama, LM Studio và OpenAI thì không** — cả ba đều chưa có endpoint rerank nào tính
đến lúc viết. Đó là lý do bản ONNX chạy trong tiến trình mới là mặc định: với một máy chỉ
có Ollama, nó là đường duy nhất chạy được. Nhánh này dành cho khi bạn dựng thêm TEI hoặc
Infinity, và nó sẵn sàng cho ngày Ollama thêm endpoint ấy.

# Vì sao trả về đủ điểm cho mọi đoạn

Endpoint nhận ``top_n`` và nhiều bản cài đặt chỉ trả về chừng ấy kết quả. Nhưng seam
:class:`~pai_rag_service.rerank.Reranker` hứa trả **một điểm cho mỗi đoạn đưa vào** —
người gọi ghép điểm với đoạn theo chỉ số. Nên ở đây không gửi ``top_n``, và đoạn nào máy
chủ bỏ qua thì nhận điểm âm vô cùng: nó tụt xuống cuối thay vì lệch cả phép ghép.
"""

from __future__ import annotations

import httpx

from pai_rag_service.config import RerankConfig
from pai_rag_service.errors import RerankError

__all__ = ["HttpReranker"]

#: Xếp hạng lại là một phép chạy nhanh trên một lô nhỏ. Chờ quá ngần này thì đằng nào
#: người dùng cũng đã bỏ đi, và trả về thứ tự cũ còn hơn treo giao diện.
TIMEOUT = httpx.Timeout(30.0, connect=5.0)


class HttpReranker:
    """Chấm điểm bằng một máy chủ rerank ngoài."""

    def __init__(self, config: RerankConfig) -> None:
        self.config = config
        self.url = config.url.strip().rstrip("/")
        if not self.url.endswith("/rerank"):
            # Cấu hình khai gốc máy chủ là chuyện thường; nối đuôi hộ thay vì trả về một
            # lỗi 404 mà người dùng phải tự đoán ra nguyên nhân.
            self.url = f"{self.url}/v1/rerank"

    @property
    def id(self) -> str:
        return f"http:{self.config.model or self.url}"

    def score(self, query: str, passages: list[str]) -> list[float]:
        if not passages:
            return []
        headers = {"Content-Type": "application/json"}
        if self.config.api_key:
            headers["Authorization"] = f"Bearer {self.config.api_key}"
        body: dict[str, object] = {"query": query, "documents": passages}
        if self.config.model:
            body["model"] = self.config.model

        try:
            response = httpx.post(self.url, headers=headers, json=body, timeout=TIMEOUT)
        except httpx.HTTPError as err:
            raise RerankError(f"không gọi được máy chủ rerank ở {self.url}: {err}") from err
        if response.status_code >= 400:
            head = response.text.strip().splitlines()
            detail = head[0][:200] if head else ""
            raise RerankError(f"máy chủ rerank trả {response.status_code}: {detail}")

        payload = response.json()
        rows = payload.get("results") if isinstance(payload, dict) else None
        if not isinstance(rows, list):
            raise RerankError("phản hồi rerank thiếu trường `results`")

        # `-inf` chứ không phải 0: một đoạn máy chủ bỏ qua phải nằm dưới **mọi** đoạn được
        # chấm, kể cả đoạn bị chấm điểm âm.
        scores = [float("-inf")] * len(passages)
        for row in rows:
            if not isinstance(row, dict):
                continue
            index = row.get("index")
            value = row.get("relevance_score", row.get("score"))
            if isinstance(index, int) and 0 <= index < len(scores) and value is not None:
                scores[index] = float(value)
        return scores
