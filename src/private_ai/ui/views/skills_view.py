"""Discovered skill packs, with the enable toggle and a look at what each one says.

The body preview matters more here than it looks: a skill is *operator instruction* that
the model is told to obey, so the person switching one on should be able to read exactly
what they are switching on before they do.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QFrame,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QScrollArea,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.ui.icons import icon

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.agent.skills.loader import Skill
    from private_ai.ui.context import AppContext

PREVIEW_CHARS = 4000
SOURCE_LABELS = {"builtin": "Đi kèm ứng dụng", "user": "Của bạn"}


class _SkillCard(QFrame):
    def __init__(self, view: SkillsView, skill: Skill, enabled: bool, parent=None) -> None:
        super().__init__(parent)
        self._skill = skill
        self.setProperty("class", "card")

        layout = QVBoxLayout(self)
        layout.setSpacing(6)

        top = QHBoxLayout()
        heading = QVBoxLayout()
        heading.setSpacing(2)
        title = QLabel(skill.title or skill.name)
        title.setProperty("class", "subtitle")
        heading.addWidget(title)
        meta = QLabel(
            f"{skill.name} · v{skill.version} · {SOURCE_LABELS.get(skill.source, skill.source)}"
        )
        meta.setProperty("class", "faint")
        heading.addWidget(meta)
        top.addLayout(heading, 1)

        self._toggle = QCheckBox("Đang bật" if enabled else "Đang tắt")
        self._toggle.setChecked(enabled)
        self._toggle.toggled.connect(lambda value: view.set_enabled(self, skill, value))
        top.addWidget(self._toggle, 0, Qt.AlignmentFlag.AlignTop)
        layout.addLayout(top)

        description = QLabel(skill.description)
        description.setWordWrap(True)
        description.setProperty("class", "muted")
        layout.addWidget(description)

        traits = []
        if skill.strategy:
            traits.append(f"Chiến lược: {skill.strategy}")
        if skill.tools:
            traits.append(f"Công cụ: {', '.join(skill.tools)}")
        if skill.keywords:
            traits.append(f"Từ khóa: {', '.join(skill.keywords)}")
        if traits:
            hint = QLabel(" · ".join(traits))
            hint.setWordWrap(True)
            hint.setProperty("class", "faint")
            layout.addWidget(hint)

        try:
            resources = skill.resources()
        except OSError:
            resources = []
        if resources:
            names = ", ".join(str(item.relative_to(skill.path)) for item in resources)
            files = QLabel(f"Tệp kèm theo: {names}")
            files.setWordWrap(True)
            files.setProperty("class", "faint")
            layout.addWidget(files)

        path = QLabel(str(skill.path))
        path.setWordWrap(True)
        path.setProperty("class", "faint")
        path.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        layout.addWidget(path)

        row = QHBoxLayout()
        self._expand = QToolButton()
        self._expand.setCheckable(True)
        self._expand.setText("Xem nội dung SKILL.md")
        self._expand.setIcon(icon("chevron-right"))
        self._expand.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        self._expand.toggled.connect(self._on_expand)
        row.addWidget(self._expand)
        row.addStretch(1)
        layout.addLayout(row)

        body = skill.body
        self._preview = QPlainTextEdit(
            body[:PREVIEW_CHARS] + ("\n…" if len(body) > PREVIEW_CHARS else "")
        )
        self._preview.setReadOnly(True)
        self._preview.setFixedHeight(220)
        self._preview.setProperty("class", "code")
        self._preview.hide()
        layout.addWidget(self._preview)

    def _on_expand(self, expanded: bool) -> None:
        self._preview.setVisible(expanded)
        self._expand.setIcon(icon("chevron-down" if expanded else "chevron-right"))

    def revert(self, enabled: bool) -> None:
        self._toggle.blockSignals(True)
        self._toggle.setChecked(enabled)
        self._toggle.blockSignals(False)
        self.sync_label(enabled)

    def sync_label(self, enabled: bool) -> None:
        self._toggle.setText("Đang bật" if enabled else "Đang tắt")


class SkillsView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._loading = False

        root = QVBoxLayout(self)
        root.setContentsMargins(4, 4, 4, 4)
        root.setSpacing(12)

        heading = QHBoxLayout()
        titles = QVBoxLayout()
        eyebrow = QLabel("Năng lực đóng gói")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Kỹ năng")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel(
            "Mỗi kỹ năng là một quy trình do người viết ứng dụng hoặc chính bạn soạn. "
            "Kỹ năng đang bật được liệt kê cho mô hình; nội dung đầy đủ chỉ được đưa vào "
            "khi câu hỏi thực sự khớp."
        )
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        heading.addLayout(titles, 1)
        rescan = QPushButton("Quét lại")
        rescan.setIcon(icon("refresh-cw"))
        rescan.clicked.connect(self.refresh)
        heading.addWidget(rescan, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

        self._paths = QLabel("")
        self._paths.setWordWrap(True)
        self._paths.setProperty("class", "faint")
        root.addWidget(self._paths)

        self._errors = QLabel("")
        self._errors.setWordWrap(True)
        self._errors.setProperty("class", "danger")
        self._errors.hide()
        root.addWidget(self._errors)

        self._empty = QLabel("")
        self._empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty.setWordWrap(True)
        self._empty.setProperty("class", "empty")
        root.addWidget(self._empty)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._canvas = QWidget()
        self._rows = QVBoxLayout(self._canvas)
        self._rows.setSpacing(8)
        self._rows.setContentsMargins(0, 0, 0, 0)
        self._rows.addStretch(1)
        self._scroll.setWidget(self._canvas)
        root.addWidget(self._scroll, 1)

        self.refresh()

    def on_activated(self) -> None:
        self.refresh()

    # --- data -------------------------------------------------------------

    def refresh(self) -> None:
        if self._loading:
            return
        self._loading = True
        self._ctx.run(
            self._ctx.services.skills.refresh_async(),
            on_result=self._loaded,
            on_error=self._failed,
        )

    def _loaded(self, skills: list[Skill]) -> None:
        self._loading = False
        registry = self._ctx.services.skills
        self._paths.setText(
            "Thư mục quét: " + " · ".join(str(path) for path in registry.search_paths)
        )
        errors = registry.errors
        if errors:
            self._errors.setText(
                "Bỏ qua các gói lỗi:\n"
                + "\n".join(f"• {path}: {message}" for path, message in errors)
            )
            self._errors.show()
        else:
            self._errors.hide()

        while self._rows.count() > 1:
            item = self._rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

        if not skills:
            self._empty.setText(
                "Chưa tìm thấy kỹ năng nào.\n"
                "Đặt một thư mục chứa SKILL.md vào thư mục kỹ năng của bạn rồi quét lại."
            )
            self._empty.show()
            self._scroll.hide()
            return
        self._empty.hide()
        self._scroll.show()
        for skill in skills:
            card = _SkillCard(self, skill, registry.is_enabled(skill.name), self._canvas)
            self._rows.insertWidget(self._rows.count() - 1, card)

    def _failed(self, exc: BaseException) -> None:
        self._loading = False
        self._ctx.toast(str(exc) or "Không quét được kỹ năng", "error")

    # --- actions ----------------------------------------------------------

    def set_enabled(self, card: _SkillCard, skill: Skill, enabled: bool) -> None:
        card.sync_label(enabled)
        self._ctx.run(
            self._ctx.services.skills.set_enabled_async(skill.name, enabled),
            on_result=lambda _: self._ctx.toast(
                f"{'Đã bật' if enabled else 'Đã tắt'} {skill.name}", "success"
            ),
            on_error=lambda exc: self._revert(card, skill, enabled, exc),
        )

    def _revert(self, card: _SkillCard, skill: Skill, attempted: bool, exc: BaseException) -> None:
        card.revert(not attempted)
        self._ctx.toast(str(exc) or f"Không đổi được trạng thái của {skill.name}", "error")
