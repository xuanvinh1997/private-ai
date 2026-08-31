"""Skill packs: what was found, what is on, and where a new one is written.

A skill is *operator instruction* the model is told to obey, so the person switching one
on must be able to read exactly what they are switching on — which is why every card can
open its own SKILL.md. It is also why authoring lives here and nowhere near ingestion:
what a skill says has to come from the person, never from a document or a model reply.
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
from private_ai.ui.theme import (
    BADGE_HEIGHT,
    CARD_MARGINS,
    CARD_SPACING,
    PAGE_SPACING,
    SPACE,
    TOOLBAR_SPACING,
)

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.agent.skills.loader import Skill
    from private_ai.ui.context import AppContext

PREVIEW_CHARS = 4000
# Tall enough to read a section of the pack without the card owning the whole page.
_PREVIEW_HEIGHT = SPACE["4xl"] * 5 + SPACE["xl"]
SOURCE_LABELS = {"builtin": "Có sẵn", "user": "Của bạn"}


class _SkillCard(QFrame):
    """One pack: what it is and whether it is on, with everything else behind a toggle.

    A skill is operator instruction the model is told to obey, so its full text has to be
    readable before anyone switches it on — but reading it is a deliberate act, not the
    default state of a list someone is scanning.
    """

    def __init__(self, view: SkillsView, skill: Skill, enabled: bool, parent=None) -> None:
        super().__init__(parent)
        self._skill = skill
        self.setProperty("class", "card")

        layout = QVBoxLayout(self)
        layout.setContentsMargins(*CARD_MARGINS)
        layout.setSpacing(CARD_SPACING)

        top = QHBoxLayout()
        top.setSpacing(TOOLBAR_SPACING)
        title = QLabel(skill.title or skill.name)
        title.setProperty("class", "card-title")
        title.setWordWrap(True)
        # The pill is taller than the title and only some cards carry one; the floor keeps
        # every card's first line on the same baseline.
        title.setMinimumHeight(BADGE_HEIGHT)
        top.addWidget(title, 1)
        if skill.source != "builtin":
            # Only the exception is worth marking. "Có sẵn" on every shipped pack was a
            # column of identical words that said nothing about any one row.
            source = QLabel(SOURCE_LABELS.get(skill.source, skill.source))
            source.setProperty("class", "pill")
            top.addWidget(source, 0, Qt.AlignmentFlag.AlignVCenter)
        # Disclosure sits on the title row it discloses. On its own row under the
        # description it read as an orphan, and cost every card a line of height.
        self._expand = QToolButton()
        self._expand.setCheckable(True)
        self._expand.setProperty("class", "icon")
        self._expand.setIcon(icon("chevron-right"))
        self._expand.toggled.connect(self._on_expand)
        self._sync_expand(False)
        top.addWidget(self._expand, 0, Qt.AlignmentFlag.AlignVCenter)
        # No caption: the checkbox already shows on or off, and "Đang bật" beside a ticked
        # box repeated the same fact down the whole page.
        self._toggle = QCheckBox()
        self._toggle.setChecked(enabled)
        self._toggle.toggled.connect(lambda value: view.set_enabled(self, skill, value))
        top.addWidget(self._toggle, 0, Qt.AlignmentFlag.AlignVCenter)
        layout.addLayout(top)
        self.sync_label(enabled)

        description = QLabel(skill.description)
        description.setWordWrap(True)
        description.setProperty("class", "muted")
        layout.addWidget(description)

        self._details = QWidget()
        details = QVBoxLayout(self._details)
        details.setContentsMargins(0, 0, 0, 0)
        details.setSpacing(SPACE["2xs"])
        for text in self._facts():
            fact = QLabel(text)
            fact.setWordWrap(True)
            fact.setProperty("class", "muted")
            details.addWidget(fact)

        path = QLabel(str(skill.path))
        path.setWordWrap(True)
        path.setProperty("class", "faint")
        path.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        details.addWidget(path)

        body = skill.body
        preview = QPlainTextEdit(
            body[:PREVIEW_CHARS] + ("\n…" if len(body) > PREVIEW_CHARS else "")
        )
        preview.setReadOnly(True)
        preview.setFixedHeight(_PREVIEW_HEIGHT)
        preview.setProperty("class", "code")
        details.addWidget(preview)

        self._details.hide()
        layout.addWidget(self._details)

    def _facts(self) -> list[str]:
        """What the pack declares, one line per kind rather than one packed line."""
        skill = self._skill
        facts = [f"Mã: {skill.name} (v{skill.version})"]
        if skill.strategy:
            facts.append(f"Chiến lược: {skill.strategy}")
        if skill.tools:
            facts.append(f"Công cụ: {', '.join(skill.tools)}")
        if skill.keywords:
            facts.append(f"Từ khóa: {', '.join(skill.keywords)}")
        try:
            resources = skill.resources()
        except OSError:
            resources = []
        if resources:
            names = ", ".join(str(item.relative_to(skill.path)) for item in resources)
            facts.append(f"Tệp kèm theo: {names}")
        return facts

    def _on_expand(self, expanded: bool) -> None:
        self._details.setVisible(expanded)
        self._sync_expand(expanded)

    def _sync_expand(self, expanded: bool) -> None:
        self._expand.setIcon(icon("chevron-down" if expanded else "chevron-right"))
        self._expand.setToolTip("Ẩn nội dung kỹ năng" if expanded else "Xem nội dung kỹ năng")
        self._expand.setAccessibleName(self._expand.toolTip())

    def revert(self, enabled: bool) -> None:
        self._toggle.blockSignals(True)
        self._toggle.setChecked(enabled)
        self._toggle.blockSignals(False)
        self.sync_label(enabled)

    def sync_label(self, enabled: bool) -> None:
        name = self._skill.title or self._skill.name
        self._toggle.setToolTip(f"Tắt {name}" if enabled else f"Bật {name}")
        self._toggle.setAccessibleName(f"Bật {name}")


class SkillsView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._loading = False

        root = QVBoxLayout(self)
        # Hosted inside the settings tab widget, which already supplies the page
        # padding; a second PAGE_MARGINS here would inset the tab twice.
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(PAGE_SPACING)

        heading = QHBoxLayout()
        heading.setSpacing(TOOLBAR_SPACING)
        titles = QVBoxLayout()
        titles.setSpacing(SPACE["2xs"])
        eyebrow = QLabel("Năng lực đóng gói")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Kỹ năng")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel("Quy trình đóng gói, chỉ nạp đầy đủ khi câu hỏi khớp.")
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        heading.addLayout(titles, 1)
        self._rescan = QToolButton()
        self._rescan.setProperty("class", "icon")
        self._rescan.setIcon(icon("refresh-cw"))
        self._rescan.setToolTip("Quét lại thư mục kỹ năng")
        self._rescan.setAccessibleName("Quét lại thư mục kỹ năng")
        self._rescan.clicked.connect(self.refresh)
        heading.addWidget(self._rescan, 0, Qt.AlignmentFlag.AlignTop)
        create = QPushButton("Kỹ năng mới")
        create.setIcon(icon("plus"))
        create.setProperty("class", "primary")
        create.clicked.connect(self._on_create)
        heading.addWidget(create, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

        self._errors = QLabel("")
        self._errors.setWordWrap(True)
        self._errors.setProperty("class", "danger")
        self._errors.hide()
        root.addWidget(self._errors)

        self._empty = QLabel("")
        self._empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty.setWordWrap(True)
        self._empty.setProperty("class", "empty")
        # Stretch lives here as well as on the scroll area: the empty state hides the
        # scroll, and a column with no expanding child hands the surplus to the page
        # header instead, which stretches the title to five times its own height.
        root.addWidget(self._empty, 1)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._canvas = QWidget()
        self._rows = QVBoxLayout(self._canvas)
        self._rows.setSpacing(SPACE["sm"])
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
        # Where the scan looked is the first thing anyone checks when a pack is missing —
        # but only then, so it hangs off the rescan button instead of taking a line.
        self._rescan.setToolTip(
            "Quét lại thư mục kỹ năng:\n" + "\n".join(str(path) for path in registry.search_paths)
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
            self._empty.setText("Chưa có kỹ năng nào.")
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

    def _on_create(self) -> None:
        from private_ai.ui.dialogs.skill_dialog import SkillDialog

        dialog = SkillDialog(self._ctx, self)
        dialog.created.connect(self._on_created)
        dialog.exec()

    def _on_created(self, skill: Skill) -> None:
        self._ctx.toast(f"Đã tạo {skill.name}", "success")
        self.refresh()

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
