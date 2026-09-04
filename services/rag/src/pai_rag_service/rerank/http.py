"""Reranker calling a `/v1/rerank` endpoint, in Cohere's de facto standard shape.
`top_n` is never sent: the seam promises one score per input passage, so a skipped one gets -inf.
"""

from __future__ import annotations

import httpx

from pai_rag_service.config import RerankConfig
from pai_rag_service.errors import RerankError

__all__ = ["HttpReranker"]

#: Reranking is a fast pass over a small batch; past this the user has already left.
TIMEOUT = httpx.Timeout(30.0, connect=5.0)


class HttpReranker:
    """Scores with an external rerank server."""

    def __init__(self, config: RerankConfig) -> None:
        self.config = config
        self.url = config.url.strip().rstrip("/")
        if not self.url.endswith("/rerank"):
            # Configuring the server root is common; append the path instead of returning a 404 the user has to diagnose.
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

        # `-inf`, not 0: a passage the server skipped must sit below every scored one, negatives included.
        scores = [float("-inf")] * len(passages)
        for row in rows:
            if not isinstance(row, dict):
                continue
            index = row.get("index")
            value = row.get("relevance_score", row.get("score"))
            if isinstance(index, int) and 0 <= index < len(scores) and value is not None:
                scores[index] = float(value)
        return scores
