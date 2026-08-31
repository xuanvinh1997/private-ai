"""Add or edit an AI provider, with a connection check before saving.

The probe lives here rather than in the service layer because it is a settings-screen
affordance: it answers "will this host answer me at all", which is exactly the question
the user is asking while typing a URL, and it must work on a draft that has never been
saved.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import httpx
from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QLabel,
    QLineEdit,
    QPushButton,
)

from private_ai.core.schemas import ProviderProbeResult
from private_ai.llm.router import openai_base_url
from private_ai.ui.dialogs import _shell

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.llm.registry import ProviderConfig
    from private_ai.ui.context import AppContext

KIND_LABELS: dict[str, str] = {"ollama": "Ollama", "openai": "OpenAI API"}

PROBE_TIMEOUT_SECONDS = 8.0


async def probe_provider(kind: str, base_url: str, api_key: str = "") -> ProviderProbeResult:
    """One cheap model-list call against a saved provider or an unsaved draft."""
    url = base_url.strip().rstrip("/")
    if not url.startswith(("http://", "https://")):
        return ProviderProbeResult(
            reachable=False, detail="Địa chỉ phải bắt đầu bằng http:// hoặc https://"
        )
    headers: dict[str, str] = {}
    if kind == "ollama":
        endpoint = f"{url}/api/tags"
    else:
        endpoint = f"{openai_base_url(url)}/models"
        if api_key.strip():
            headers["Authorization"] = f"Bearer {api_key.strip()}"
    try:
        async with httpx.AsyncClient(timeout=PROBE_TIMEOUT_SECONDS) as client:
            response = await client.get(endpoint, headers=headers)
            response.raise_for_status()
            payload = response.json()
    except httpx.HTTPError as exc:
        return ProviderProbeResult(reachable=False, detail=str(exc))
    except ValueError:
        return ProviderProbeResult(reachable=False, detail="Máy chủ trả về dữ liệu không hợp lệ")

    rows = payload.get("models") if kind == "ollama" else payload.get("data")
    names = []
    for row in rows or []:
        if isinstance(row, dict):
            name = str(row.get("name") or row.get("id") or "").strip()
            if name:
                names.append(name)
    return ProviderProbeResult(reachable=True, model_count=len(names), models=names)


class ProviderDialog(QDialog):
    saved = Signal()

    def __init__(
        self,
        ctx: AppContext,
        provider: ProviderConfig | None = None,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._provider = provider
        self._busy = False

        editing = provider is not None
        self.setModal(True)
        self.setWindowTitle(provider.name if provider else "Thêm nhà cung cấp AI")
        self.setMinimumWidth(480)

        layout = _shell.dialog_layout(self)
        _shell.title_block(
            layout,
            self.windowTitle(),
            "Đổi địa chỉ Ollama mà ứng dụng gọi tới, ví dụ khi Ollama chạy trong WSL2 hoặc "
            "trên một máy khác trong mạng nội bộ."
            if (provider is not None and provider.builtin)
            else "Kết nối tới một máy chủ nói giao thức OpenAI API, ví dụ vLLM, LM Studio, "
            "LiteLLM hoặc OpenAI. Khóa API chỉ được lưu trên máy này.",
        )

        self._name = QLineEdit(provider.name if provider else "")
        self._name.setPlaceholderText("Máy chủ nội bộ")
        self._name.setMaxLength(120)
        _shell.field(layout, "Tên hiển thị", self._name)

        self._kind = QComboBox()
        for value in ("openai", "ollama"):
            self._kind.addItem(KIND_LABELS[value], value)
        self._kind_label = _shell.field(layout, "Giao thức", self._kind)
        if editing:
            # The kind decides the client class and cannot be swapped under a saved row.
            self._kind.setCurrentIndex(self._kind.findData(provider.kind))
            self._kind_label.hide()
            self._kind.hide()
        self._kind.currentIndexChanged.connect(self._sync_key_row)

        self._base_url = QLineEdit(provider.base_url if provider else "")
        self._base_url.setPlaceholderText("https://api.openai.com/v1")
        self._base_url.setMaxLength(500)
        _shell.field(layout, "Địa chỉ máy chủ", self._base_url)

        self._key = QLineEdit()
        self._key.setEchoMode(QLineEdit.EchoMode.Password)
        self._key.setMaxLength(500)
        self._key.setPlaceholderText(
            "Giữ nguyên khóa đã lưu" if (provider and provider.api_key) else "sk-…"
        )
        self._key_label = _shell.field(layout, "Khóa API", self._key)

        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setProperty("class", "muted")
        self._status.hide()
        layout.addWidget(self._status)

        self._error = QLabel("")
        self._error.setWordWrap(True)
        self._error.setProperty("class", "danger")
        self._error.hide()
        layout.addWidget(self._error)

        row = _shell.action_row(layout)
        self._probe = QPushButton("Kiểm tra")
        self._probe.clicked.connect(self._on_probe)
        row.addWidget(self._probe)
        row.addStretch(1)
        cancel = QPushButton("Hủy")
        cancel.clicked.connect(self.reject)
        row.addWidget(cancel)
        self._save = QPushButton("Lưu" if editing else "Thêm")
        self._save.setProperty("class", "primary")
        self._save.setDefault(True)
        self._save.clicked.connect(self._on_save)
        row.addWidget(self._save)

        self._sync_key_row()
        self._name.setFocus(Qt.FocusReason.OtherFocusReason)

    # --- helpers ----------------------------------------------------------

    def _current_kind(self) -> str:
        if self._provider is not None:
            return self._provider.kind
        return str(self._kind.currentData() or "openai")

    def _sync_key_row(self) -> None:
        visible = self._current_kind() == "openai"
        self._key_label.setVisible(visible)
        self._key.setVisible(visible)

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        self._save.setEnabled(not busy)
        self._probe.setEnabled(not busy)

    def _fail(self, message: str) -> None:
        self._status.hide()
        self._error.setText(message)
        self._error.show()
        self._set_busy(False)

    def _inform(self, message: str) -> None:
        self._error.hide()
        self._status.setText(message)
        self._status.show()

    # --- actions ----------------------------------------------------------

    def _on_probe(self) -> None:
        base_url = self._base_url.text().strip()
        if not base_url:
            self._fail("Cần nhập địa chỉ máy chủ")
            return
        self._set_busy(True)
        self._inform("Đang kiểm tra kết nối…")
        self._ctx.run(
            probe_provider(self._current_kind(), base_url, self._key.text()),
            on_result=self._show_probe,
            on_error=lambda exc: self._fail(str(exc) or "Không kiểm tra được kết nối"),
        )

    def _show_probe(self, result: ProviderProbeResult) -> None:
        self._set_busy(False)
        if not result.reachable:
            self._fail(result.detail or "Không kết nối được tới máy chủ này")
            return
        sample = ", ".join(result.models[:3])
        suffix = f" ({sample}…)" if sample else ""
        self._inform(f"Kết nối thành công · {result.model_count} mô hình{suffix}")

    def _on_save(self) -> None:
        if self._busy:
            return
        name = self._name.text().strip()
        base_url = self._base_url.text().strip()
        if not name or not base_url:
            self._fail("Cần nhập tên và địa chỉ máy chủ")
            return
        self._error.hide()
        self._set_busy(True)
        # A probe before every save: a provider that cannot be reached is a provider the
        # user will otherwise only discover is broken in the middle of a conversation.
        self._ctx.run(
            probe_provider(self._current_kind(), base_url, self._key.text()),
            on_result=lambda result: self._save_after_probe(result, name, base_url),
            on_error=lambda exc: self._fail(str(exc) or "Không kiểm tra được kết nối"),
        )

    def _save_after_probe(self, result: ProviderProbeResult, name: str, base_url: str) -> None:
        if not result.reachable:
            self._fail(
                f"{result.detail or 'Không kết nối được tới máy chủ này'} — "
                "sửa địa chỉ hoặc khóa rồi thử lại."
            )
            return
        registry = self._ctx.services.providers
        key = self._key.text().strip()
        try:
            if self._provider is None:
                registry.create(
                    name=name,
                    kind=self._current_kind(),
                    base_url=base_url,
                    api_key=key,
                )
            else:
                registry.update(
                    self._provider.id,
                    name=name,
                    base_url=base_url,
                    # An untouched key field means "keep the stored key", never "clear it".
                    api_key=key or None,
                )
        except (ValueError, LookupError) as exc:
            self._fail(str(exc) or "Không lưu được nhà cung cấp")
            return
        self._set_busy(False)
        self.saved.emit()
        self.accept()
