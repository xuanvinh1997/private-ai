"""Cross-encoder chạy bằng ONNX Runtime, không cần torch.

# Vì sao ONNX và không phải torch

``sentence-transformers`` kéo theo torch và, nếu muốn GPU, cả CUDA runtime: khoảng 3–5 GB
trong một bản cài mà phần còn lại chưa tới 300 MB. ONNX Runtime chạy đúng model đó,
nhanh tương đương trên CPU, và ``onnxruntime-gpu`` bật CUDA bằng cách đổi một gói chứ
không đổi mã — xem extra ``gpu`` trong ``pyproject.toml``.

# Nạp lười, và chỉ một lần

Phiên ONNX tốn vài giây để dựng và vài trăm megabyte RAM. Dựng nó lúc khởi động nghĩa là
mọi lần chạy ``pai-rag`` đều trả giá đó kể cả khi không ai tìm kiếm — và bản MCP stdio
thì khởi động ở mỗi lần ứng dụng mở dự án. Nên nó được dựng ở lần chấm điểm đầu tiên và
giữ lại sau đó.

Lần đầu còn phải **tải model về**, cỡ một gigabyte. Đó là một sự việc phải nói ra chứ
không phải một khoảng im lặng: xem ``pai-rag doctor``, thứ tải sẵn để lần tìm đầu tiên
của người dùng không phải chờ.
"""

from __future__ import annotations

import logging
import threading
from typing import Any

import numpy as np

from pai_rag_service.config import RerankConfig
from pai_rag_service.errors import RerankError

__all__ = ["OnnxReranker"]

log = logging.getLogger(__name__)

#: Cửa sổ của XLM-RoBERTa. Cắt ở đây chứ không ở chỗ khác vì model **không đọc** quá 512
#: token; đưa dài hơn thì phần đuôi bị bỏ lặng lẽ.
MAX_LENGTH = 512
#: Bao nhiêu cặp một lần chạy. 16 cặp × 512 token là một ma trận vừa phải trên CPU và
#: không làm GPU đói ở bản `onnxruntime-gpu`.
BATCH = 16


class OnnxReranker:
    """Chấm cặp (câu hỏi, đoạn) bằng một cross-encoder ONNX."""

    def __init__(self, config: RerankConfig) -> None:
        self.config = config
        self._lock = threading.Lock()
        self._session: Any = None
        self._tokenizer: Any = None
        self._inputs: set[str] = set()
        self._provider: str = ""

    @property
    def id(self) -> str:
        return f"onnx:{self.config.model}"

    @property
    def provider(self) -> str:
        """Execution provider thật sự đang chạy. Rỗng khi chưa nạp.

        Là thứ **thật sự** dùng, không phải thứ đã xin — xem
        :meth:`_canh_bao_neu_tut_ve_cpu`.
        """
        return self._provider

    # -- nạp ---------------------------------------------------------------------------

    def _ensure(self) -> None:
        """Dựng phiên ONNX và tokenizer, đúng một lần."""
        if self._session is not None:
            return
        with self._lock:
            if self._session is not None:
                return

            import onnxruntime
            from huggingface_hub import hf_hub_download
            from tokenizers import Tokenizer

            # Nạp DLL của CUDA từ các gói `nvidia-*-cu12` trong site-packages.
            #
            # Không có bước này thì `onnxruntime_providers_cuda.dll` không tìm thấy
            # `cublasLt64_12.dll` và ONNX Runtime **lùi về CPU trong im lặng** — GPU vẫn
            # rảnh, tìm kiếm vẫn chạy, chỉ là chậm gấp mấy chục lần. Thư viện CUDA của
            # gói pip không nằm trên PATH của hệ thống, nên phải chỉ đường tường minh.
            #
            # `hasattr` vì hàm này chỉ có từ onnxruntime 1.21; bản CPU cũ hơn vẫn chạy
            # được, chỉ là không có gì để nạp.
            if hasattr(onnxruntime, "preload_dlls"):
                try:
                    onnxruntime.preload_dlls()
                except Exception as err:
                    # Không chặn: thiếu CUDA thì phiên vẫn dựng được trên CPU.
                    log.debug("không nạp sẵn được DLL của CUDA: %s", err)

            repo = self.config.model
            try:
                path = hf_hub_download(
                    repo_id=repo,
                    filename=self.config.onnx_file,
                    cache_dir=self.config.cache_dir,
                )
            except Exception as err:
                raise RerankError(
                    f"không tải được `{self.config.onnx_file}` từ `{repo}`: {err}. "
                    "Kiểm tra tên model trong Cài đặt, hoặc tắt xếp hạng lại."
                ) from err

            # Model lớn tách trọng số ra `model.onnx_data` nằm cạnh. `hf_hub_download`
            # chỉ lấy đúng tệp được hỏi, nên phải hỏi thêm — không có nó thì phiên ONNX
            # dựng lên rồi hỏng ở lần chạy đầu với một lỗi không nói được vì sao.
            self._fetch_external_data(repo)

            try:
                self._tokenizer = Tokenizer.from_pretrained(repo)
            except Exception as err:
                raise RerankError(f"không nạp được tokenizer của `{repo}`: {err}") from err
            self._tokenizer.enable_truncation(max_length=MAX_LENGTH)
            self._tokenizer.enable_padding()

            options = onnxruntime.SessionOptions()
            options.graph_optimization_level = (
                onnxruntime.GraphOptimizationLevel.ORT_ENABLE_ALL
            )
            # `onnxruntime-gpu` có CUDA trong danh sách; bản CPU thì không, và khi ấy
            # danh sách này rút gọn về đúng CPU. Một dòng cho cả hai bản cài.
            available = onnxruntime.get_available_providers()
            wanted = ("CUDAExecutionProvider", "CPUExecutionProvider")
            providers = [name for name in wanted if name in available]
            try:
                self._session = onnxruntime.InferenceSession(
                    path, sess_options=options, providers=providers
                )
            except Exception as err:
                raise RerankError(
                    f"ONNX Runtime không dựng được phiên cho `{repo}`: {err}"
                ) from err
            self._inputs = {item.name for item in self._session.get_inputs()}
            self._provider = self._session.get_providers()[0]
            log.info(
                "reranker sẵn sàng: %s (%s), đầu vào %s",
                repo,
                self._provider,
                sorted(self._inputs),
            )
            self._canh_bao_neu_tut_ve_cpu(available, providers)

    def _canh_bao_neu_tut_ve_cpu(self, available: list[str], asked: list[str]) -> None:
        """Nói to khi phiên tụt về CPU dù CUDA có trong danh sách.

        # Vì sao đây là một cảnh báo chứ không phải một dòng debug

        ONNX Runtime **lùi về CPU trong im lặng** khi provider CUDA không nạp được — DLL
        thiếu, sai đời CUDA, driver cũ. Phiên vẫn dựng, kết quả vẫn đúng, và không có gì
        báo lỗi.

        Cái mất là tốc độ, và nó lớn tới mức đổi hẳn tính dùng được: đo trên máy thật với
        `bge-reranker-v2-m3` (XLM-RoBERTa large, 568M tham số), CPU chấm khoảng **0,4 giây
        mỗi đoạn** — 30 ứng viên là hơn mười giây cho mỗi câu hỏi. Người dùng chỉ thấy tìm
        kiếm chậm và không có cách nào đoán ra vì sao.
        """
        if self._provider != "CPUExecutionProvider":
            return
        if "CUDAExecutionProvider" not in available:
            # Bản `onnxruntime` CPU. Đây là lựa chọn, không phải sự cố — nhưng vẫn nói ra
            # cái giá, vì nó quyết định `candidates` nên đặt bao nhiêu.
            log.info(
                "reranker chạy trên CPU (~0,4s mỗi đoạn). Cài extra `gpu` và CUDA khớp "
                "đời với onnxruntime để nhanh hơn nhiều lần."
            )
            return
        log.warning(
            "reranker TỤT VỀ CPU dù đã xin %s. ONNX Runtime lùi về CPU trong im lặng khi "
            "thư viện CUDA không nạp được — thường là sai đời CUDA hoặc driver cũ; dòng "
            "chẩn đoán của nó nằm ngay phía trên. Hậu quả: mỗi đoạn mất ~0,4s, nên %d ứng "
            "viên là hơn mười giây cho mỗi câu hỏi.",
            asked,
            self.config.candidates,
        )

    def _fetch_external_data(self, repo: str) -> None:
        """Lấy tệp trọng số ngoài, nếu repo có."""
        from huggingface_hub import hf_hub_download
        from huggingface_hub.errors import EntryNotFoundError

        sidecar = f"{self.config.onnx_file}_data"
        try:
            hf_hub_download(repo_id=repo, filename=sidecar, cache_dir=self.config.cache_dir)
        except EntryNotFoundError:
            # Model nhỏ gói trọn trong một tệp. Đây là trường hợp thường gặp, không phải lỗi.
            return
        except Exception as err:
            log.debug("không lấy được `%s` của `%s`: %s", sidecar, repo, err)

    # -- chấm điểm ---------------------------------------------------------------------

    def score(self, query: str, passages: list[str]) -> list[float]:
        if not passages:
            return []
        self._ensure()

        out: list[float] = []
        for start in range(0, len(passages), BATCH):
            batch = passages[start : start + BATCH]
            encoded = self._tokenizer.encode_batch([(query, passage) for passage in batch])
            feed: dict[str, np.ndarray] = {
                "input_ids": np.array([item.ids for item in encoded], dtype=np.int64),
                "attention_mask": np.array(
                    [item.attention_mask for item in encoded], dtype=np.int64
                ),
            }
            # XLM-RoBERTa không dùng `token_type_ids`, nhưng vài bản export vẫn khai nó ở
            # đầu vào. Chỉ đưa những gì phiên thật sự hỏi: thừa một khoá là một lỗi
            # "invalid input name", thiếu một khoá cũng vậy.
            if "token_type_ids" in self._inputs:
                feed["token_type_ids"] = np.array(
                    [item.type_ids for item in encoded], dtype=np.int64
                )
            feed = {name: value for name, value in feed.items() if name in self._inputs}

            logits = self._session.run(None, feed)[0]
            out.extend(self._as_scores(logits))
        return out

    @staticmethod
    def _as_scores(logits: np.ndarray) -> list[float]:
        """Logits → điểm.

        Cross-encoder rerank trả về **một** logit mỗi cặp. Vài bản export trả hình dạng
        ``(batch, 1)``, vài bản trả ``(batch,)``; một số model phân loại trả ``(batch, 2)``
        và khi ấy điểm liên quan là lớp dương.

        Không bọc sigmoid: xếp hạng chỉ cần thứ tự, mà sigmoid là hàm đơn điệu tăng nên
        nó không đổi thứ tự — chỉ nén khoảng cách lại và làm điểm hiển thị khó đọc hơn.
        """
        array = np.asarray(logits, dtype=np.float32)
        if array.ndim == 1:
            return [float(value) for value in array]
        if array.shape[-1] == 1:
            return [float(value) for value in array.reshape(-1)]
        return [float(row[-1]) for row in array.reshape(-1, array.shape[-1])]
