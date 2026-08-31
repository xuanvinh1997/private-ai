"""Write a new skill pack without leaving the app.

Three fields on purpose. A SKILL.md accepts more — tools, strategy, a version — but the
name, the one line that decides when the skill is picked, and the instructions are the
only ones a pack cannot exist without; the rest is edited in the file the dialog creates.
"""

from __future__ import annotations

import re
import unicodedata
from typing import TYPE_CHECKING

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import QDialog, QLabel, QLineEdit, QPlainTextEdit, QPushButton

from private_ai.ui.dialogs import _shell
from private_ai.ui.theme import CONTROL_HEIGHT

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.agent.skills.loader import Skill
    from private_ai.ui.context import AppContext


def slugify(value: str) -> str:
    """A title typed in Vietnamese, folded into the lowercase slug the loader accepts."""
    decomposed = unicodedata.normalize("NFKD", value.casefold().replace("đ", "d"))
    ascii_text = "".join(char for char in decomposed if not unicodedata.combining(char))
    return re.sub(r"-{2,}", "-", re.sub(r"[^a-z0-9]+", "-", ascii_text)).strip("-")[:64]


class SkillDialog(QDialog):
    """Modal for a new user pack."""

    created = Signal(object)  # Skill

    def __init__(self, ctx: AppContext, parent=None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._busy = False
        self._touched_name = False

        self.setModal(True)
        self.setWindowTitle("Kỹ năng mới")
        self.setMinimumWidth(520)

        layout = _shell.dialog_layout(self)
        _shell.title_block(
            layout,
            self.windowTitle(),
            "Một quy trình bạn viết bằng lời, lưu thành SKILL.md trong thư mục kỹ năng.",
        )

        self._title = QLineEdit()
        self._title.setMaxLength(120)
        self._title.setPlaceholderText("Tóm tắt hợp đồng")
        self._title.textEdited.connect(self._suggest_name)
        _shell.field(layout, "Tên hiển thị", self._title)

        self._name = QLineEdit()
        self._name.setMaxLength(64)
        self._name.setPlaceholderText("tom-tat-hop-dong")
        self._name.textEdited.connect(self._on_name_edited)
        _shell.field(layout, "Mã kỹ năng", self._name)

        self._description = QLineEdit()
        self._description.setMaxLength(240)
        self._description.setPlaceholderText("Khi nào nên dùng kỹ năng này")
        # This line is the whole of what the model sees until the skill is activated, so
        # it is the one field worth spelling out beside the caption.
        _shell.field(layout, "Khi nào dùng", self._description)

        self._body = QPlainTextEdit()
        self._body.setPlaceholderText("Các bước bạn muốn trợ lý làm theo…")
        self._body.setFixedHeight(CONTROL_HEIGHT * 6)
        _shell.field(layout, "Hướng dẫn", self._body)

        self._error = QLabel("")
        self._error.setWordWrap(True)
        self._error.setProperty("class", "danger")
        self._error.hide()
        layout.addWidget(self._error)

        row = _shell.action_row(layout)
        row.addStretch(1)
        cancel = QPushButton("Hủy")
        cancel.clicked.connect(self.reject)
        row.addWidget(cancel)
        self._save = QPushButton("Tạo")
        self._save.setProperty("class", "primary")
        self._save.setDefault(True)
        self._save.clicked.connect(self._on_save)
        row.addWidget(self._save)

        self._title.setFocus(Qt.FocusReason.OtherFocusReason)

    # --- helpers ----------------------------------------------------------

    def _suggest_name(self, text: str) -> None:
        if not self._touched_name:
            self._name.setText(slugify(text))

    def _on_name_edited(self, text: str) -> None:
        self._touched_name = bool(text)

    def _fail(self, message: str) -> None:
        self._error.setText(message)
        self._error.show()
        self._set_busy(False)

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        self._save.setEnabled(not busy)
        self._save.setText("Đang tạo…" if busy else "Tạo")

    # --- actions ----------------------------------------------------------

    def _on_save(self) -> None:
        if self._busy:
            return
        title = self._title.text().strip()
        name = self._name.text().strip() or slugify(title)
        description = self._description.text().strip()
        body = self._body.toPlainText().strip()
        if not name:
            self._fail("Cần một mã kỹ năng, ví dụ 'tom-tat-hop-dong'.")
            return
        if not description:
            self._fail("Cần một dòng mô tả khi nào dùng kỹ năng này.")
            return
        if not body:
            self._fail("Cần phần hướng dẫn cho kỹ năng.")
            return
        self._error.hide()
        self._set_busy(True)
        self._ctx.run(
            self._ctx.services.skills.create_async(
                name=name, description=description, body=body, title=title
            ),
            on_result=self._done,
            on_error=lambda exc: self._fail(str(exc) or "Không tạo được kỹ năng"),
        )

    def _done(self, skill: Skill) -> None:
        self._set_busy(False)
        self.created.emit(skill)
        self.accept()
