"""Cross-encoder on ONNX Runtime, no torch: the same model, but 300 MB instead of 3-5 GB,
and GPU is a package swap rather than a code change. The session is built on the first
score and kept, since it costs seconds and hundreds of megabytes."""

from __future__ import annotations

import logging
import threading
from typing import Any

import numpy as np

from pai_rag_service.config import RerankConfig
from pai_rag_service.errors import RerankError

__all__ = ["OnnxReranker"]

log = logging.getLogger(__name__)

#: XLM-RoBERTa's window; the model does not read past 512 tokens, and the tail is dropped silently.
MAX_LENGTH = 512
#: Pairs per run. 16 x 512 is a reasonable matrix on CPU and does not starve the GPU build.
BATCH = 16


class OnnxReranker:
    """Scores (query, passage) pairs with an ONNX cross-encoder."""

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
        """The execution provider actually in use, empty until loaded - see :meth:`_canh_bao_neu_tut_ve_cpu`."""
        return self._provider

    # -- loading -------------------------------------------------------------------------

    def _ensure(self) -> None:
        """Build the ONNX session and tokenizer, exactly once."""
        if self._session is not None:
            return
        with self._lock:
            if self._session is not None:
                return

            import onnxruntime
            from huggingface_hub import hf_hub_download
            from tokenizers import Tokenizer

            # Load the CUDA DLLs from `nvidia-*-cu12`; without this ONNX Runtime falls back to CPU silently. `hasattr` because the call only exists from onnxruntime 1.21.
            if hasattr(onnxruntime, "preload_dlls"):
                try:
                    onnxruntime.preload_dlls()
                except Exception as err:
                    # Not fatal: without CUDA the session still builds on CPU.
                    log.debug("could not preload the CUDA DLLs: %s", err)

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

            # Large models keep weights in a neighbouring `model.onnx_data`; `hf_hub_download` fetches only what is asked, and without it the session fails on first run.
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
            # `onnxruntime-gpu` lists CUDA; the CPU build does not, and the list collapses to CPU. One line for both installs.
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
                "reranker ready: %s (%s), inputs %s",
                repo,
                self._provider,
                sorted(self._inputs),
            )
            self._canh_bao_neu_tut_ve_cpu(available, providers)

    def _canh_bao_neu_tut_ve_cpu(self, available: list[str], asked: list[str]) -> None:
        """Say it loudly when the session lands on CPU although CUDA was listed: ONNX Runtime falls back silently, and the cost is about 0.4 s per passage."""
        if self._provider != "CPUExecutionProvider":
            return
        if "CUDAExecutionProvider" not in available:
            # The CPU `onnxruntime` build. A choice, not a fault - but name the cost, since it decides how many `candidates` to ask for.
            log.info(
                "reranker running on CPU (~0.4s per passage). Install the `gpu` extra with a CUDA "
                "generation matching onnxruntime to make it many times faster."
            )
            return
        log.warning(
            "reranker FELL BACK TO CPU although %s was requested. ONNX Runtime falls back "
            "silently when the CUDA libraries cannot load - usually a CUDA generation "
            "mismatch or an old driver; its diagnostic line is just above. Cost: ~0.4s per "
            "passage, so %d candidates is over ten seconds per query.",
            asked,
            self.config.candidates,
        )

    def _fetch_external_data(self, repo: str) -> None:
        """Fetch the external weights file, if the repo has one."""
        from huggingface_hub import hf_hub_download
        from huggingface_hub.errors import EntryNotFoundError

        sidecar = f"{self.config.onnx_file}_data"
        try:
            hf_hub_download(repo_id=repo, filename=sidecar, cache_dir=self.config.cache_dir)
        except EntryNotFoundError:
            # Small models fit in a single file. Common, not an error.
            return
        except Exception as err:
            log.debug("could not fetch `%s` of `%s`: %s", sidecar, repo, err)

    # -- scoring -------------------------------------------------------------------------

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
            # XLM-RoBERTa does not use `token_type_ids`, but some exports declare it; feed exactly what the session asks for, since an extra or missing key is an error.
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
        """Logits -> scores; exports return `(batch, 1)`, `(batch,)` or `(batch, 2)`, and no sigmoid is applied because ranking only needs order."""
        array = np.asarray(logits, dtype=np.float32)
        if array.ndim == 1:
            return [float(value) for value in array]
        if array.shape[-1] == 1:
            return [float(value) for value in array.reshape(-1)]
        return [float(row[-1]) for row in array.reshape(-1, array.shape[-1])]
