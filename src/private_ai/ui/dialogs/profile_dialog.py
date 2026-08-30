"""Ask for a display name — on first run, on rename, and when adding a profile.

Three modes because the same form means three different things. Onboarding is the odd
one: there is no previous answer to fall back on, so Escape and the window close button
are both suppressed and only "Để sau" gets the user out.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QVBoxLayout,
)

from private_ai.core import repositories

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.schemas import ProfileRecord
    from private_ai.ui.context import AppContext

ONBOARDING = "onboarding"
RENAME = "rename"
CREATE = "create"

_COPY: dict[str, tuple[str, str, str]] = {
    ONBOARDING: (
        "Chào bạn, mình gọi bạn là gì?",
        "Tên này chỉ được lưu trên máy của bạn và dùng để xưng hô trong ứng dụng. "
        "Bạn đổi lại lúc nào cũng được.",
        "Bắt đầu",
    ),
    RENAME: (
        "Đổi tên hiển thị",
        "Tên mới áp dụng ngay cho lời chào và ô trò chuyện.",
        "Lưu",
    ),
    CREATE: (
        "Thêm hồ sơ",
        "Hồ sơ mới có bộ nhớ riêng và sẽ được dùng ngay. Tài liệu cùng không gian làm việc "
        "vẫn dùng chung trên máy này.",
        "Tạo và chuyển sang",
    ),
}


def initials_of(name: str) -> str:
    """ "Phạm Xuân Vinh" → "PV", so the avatar follows whatever name the person chose."""
    parts = [part for part in name.strip().split() if part]
    if not parts:
        return "?"
    letters = parts[0][:2] if len(parts) == 1 else f"{parts[0][0]}{parts[-1][0]}"
    return letters.upper()


class ProfileNameDialog(QDialog):
    done_ = Signal(object)  # ProfileRecord

    def __init__(
        self,
        ctx: AppContext,
        mode: str = RENAME,
        profile: ProfileRecord | None = None,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._mode = mode if mode in _COPY else RENAME
        self._profile = profile
        self._busy = False

        title, description, action = _COPY[self._mode]
        self.setModal(True)
        self.setWindowTitle(title)
        self.setMinimumWidth(440)
        if self._mode == ONBOARDING:
            # No close button: onboarding is answered, deferred, never dismissed.
            self.setWindowFlag(Qt.WindowType.WindowCloseButtonHint, False)

        layout = QVBoxLayout(self)
        layout.setSpacing(10)

        heading = QLabel(title)
        heading.setProperty("class", "title")
        heading.setWordWrap(True)
        layout.addWidget(heading)

        blurb = QLabel(description)
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        layout.addWidget(blurb)

        layout.addWidget(QLabel("Tên hiển thị"))
        self._name = QLineEdit(profile.display_name if (profile and mode == RENAME) else "")
        self._name.setMaxLength(60)
        self._name.setPlaceholderText("Ví dụ: Vinh")
        self._name.returnPressed.connect(self._on_save)
        layout.addWidget(self._name)

        self._error = QLabel("")
        self._error.setProperty("class", "danger")
        self._error.setWordWrap(True)
        self._error.hide()
        layout.addWidget(self._error)

        row = QHBoxLayout()
        row.addStretch(1)
        later = QPushButton("Để sau" if self._mode == ONBOARDING else "Hủy")
        later.clicked.connect(self.reject)
        row.addWidget(later)
        self._save = QPushButton(action)
        self._save.setProperty("class", "primary")
        self._save.setDefault(True)
        self._save.clicked.connect(self._on_save)
        row.addWidget(self._save)
        layout.addLayout(row)

        self._name.setFocus(Qt.FocusReason.OtherFocusReason)

    # --- behaviour --------------------------------------------------------

    def keyPressEvent(self, event) -> None:  # noqa: N802
        if self._mode == ONBOARDING and event.key() == Qt.Key.Key_Escape:
            event.ignore()
            return
        super().keyPressEvent(event)

    def _fail(self, message: str) -> None:
        self._error.setText(message)
        self._error.show()
        self._busy = False
        self._save.setEnabled(True)
        self._save.setText(_COPY[self._mode][2])

    def _on_save(self) -> None:
        if self._busy:
            return
        value = self._name.text().strip()
        if not value:
            self._fail("Hãy nhập tên bạn muốn hiển thị.")
            return
        self._error.hide()
        self._busy = True
        self._save.setEnabled(False)
        self._save.setText("Đang lưu…")

        database = self._ctx.database
        if self._mode == CREATE or self._profile is None:
            coro = repositories.create_profile(database, value)
        else:
            coro = repositories.rename_profile(database, self._profile.id, value)
        self._ctx.run(
            coro,
            on_result=self._finish,
            on_error=lambda exc: self._fail(str(exc) or "Không lưu được tên"),
        )

    def _finish(self, profile: ProfileRecord) -> None:
        self._busy = False
        self.done_.emit(profile)
        self.accept()
