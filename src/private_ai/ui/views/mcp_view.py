"""Mounted MCP servers and the tools they publish.

The eight built-ins are mounted in process and cannot be removed — they *are* the
application's own capability surface. Only the rows the user added to ``mcp_servers``
are editable, which is why the two lists are drawn separately.
"""

from __future__ import annotations

import json
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any
from uuid import uuid4

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPlainTextEdit,
    QPushButton,
    QScrollArea,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.mcp.adapter import name_for
from private_ai.mcp.client import AGENT_TOOLS, ARTIFACT_TOOLS, EXTERNAL_PREFIX
from private_ai.ui.icons import icon
from private_ai.ui.theme import (
    CARD_MARGINS,
    CARD_SPACING,
    CONTROL_HEIGHT,
    DIALOG_MARGINS,
    DIALOG_SPACING,
    PAGE_SPACING,
    SPACE,
    TOOLBAR_SPACING,
)
from private_ai.ui.widgets.confirm_button import ConfirmButton

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.schemas import McpServerRecord
    from private_ai.ui.context import AppContext

KIND_LABELS = {"builtin": "Nội bộ", "stdio": "Tiến trình (stdio)", "http": "HTTP"}


def _now() -> str:
    return datetime.now(UTC).isoformat()


async def create_mcp_server(database, record: dict[str, Any]) -> None:
    await database.execute_async(
        """
        INSERT INTO mcp_servers(
            id, name, kind, command, args_json, url, headers_json, enabled,
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
        """,
        (
            str(uuid4()),
            record["name"],
            record["kind"],
            record.get("command", ""),
            json.dumps(record.get("args") or []),
            record.get("url", ""),
            json.dumps(record.get("headers") or {}),
            _now(),
            _now(),
        ),
    )


async def delete_mcp_server(database, server_id: str) -> None:
    await database.execute_async("DELETE FROM mcp_servers WHERE id = ?", (server_id,))


def _field(caption: str, control: QWidget) -> QWidget:
    """A caption glued to its control, so the form has one rhythm and hides as one row."""
    holder = QWidget()
    box = QVBoxLayout(holder)
    box.setContentsMargins(0, 0, 0, 0)
    box.setSpacing(SPACE["2xs"])
    label = QLabel(caption)
    label.setProperty("class", "muted")
    box.addWidget(label)
    box.addWidget(control)
    return holder


class McpServerDialog(QDialog):
    """Describe one external server: a command to spawn, or a URL to dial."""

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.setModal(True)
        self.setWindowTitle("Thêm MCP server")
        self.setMinimumWidth(500)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(*DIALOG_MARGINS)
        layout.setSpacing(DIALOG_SPACING)

        heading = QLabel("Thêm MCP server")
        heading.setProperty("class", "title")
        layout.addWidget(heading)
        blurb = QLabel(
            "Công cụ của server ngoài được đặt tiền tố "
            f"“{EXTERNAL_PREFIX}.<tên>.” trước khi mô hình nhìn thấy, nên không thể trùng "
            "hay giả dạng công cụ nội bộ. Server chỉ được nối khi bạn khởi động lại ứng dụng."
        )
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        layout.addWidget(blurb)

        self._name = QLineEdit()
        self._name.setPlaceholderText("vi-du-server")
        layout.addWidget(_field("Tên", self._name))

        self._kind = QComboBox()
        self._kind.addItem(KIND_LABELS["stdio"], "stdio")
        self._kind.addItem(KIND_LABELS["http"], "http")
        self._kind.currentIndexChanged.connect(self._sync)
        layout.addWidget(_field("Kiểu kết nối", self._kind))

        self._command = QLineEdit()
        self._command.setPlaceholderText("uvx some-mcp-server --flag")
        self._command_row = _field("Lệnh và tham số", self._command)
        layout.addWidget(self._command_row)

        self._url = QLineEdit()
        self._url.setPlaceholderText("https://example.com/mcp")
        self._url_row = _field("Địa chỉ HTTP", self._url)
        layout.addWidget(self._url_row)

        self._headers = QPlainTextEdit()
        # Two control heights: enough for a couple of header lines without a scrollbar.
        self._headers.setFixedHeight(CONTROL_HEIGHT * 2)
        self._headers_row = _field("Header (mỗi dòng một cặp KEY: value)", self._headers)
        layout.addWidget(self._headers_row)

        self._error = QLabel("")
        self._error.setWordWrap(True)
        self._error.setProperty("class", "danger")
        self._error.hide()
        layout.addWidget(self._error)

        row = QHBoxLayout()
        row.setSpacing(TOOLBAR_SPACING)
        row.addStretch(1)
        cancel = QPushButton("Hủy")
        cancel.clicked.connect(self.reject)
        row.addWidget(cancel)
        save = QPushButton("Thêm")
        save.setProperty("class", "primary")
        save.setDefault(True)
        save.clicked.connect(self._on_save)
        row.addWidget(save)
        layout.addLayout(row)

        self._sync()
        self._name.setFocus(Qt.FocusReason.OtherFocusReason)

    def _sync(self) -> None:
        stdio = self._kind.currentData() == "stdio"
        self._command_row.setVisible(stdio)
        for widget in (self._url_row, self._headers_row):
            widget.setVisible(not stdio)

    def _fail(self, message: str) -> None:
        self._error.setText(message)
        self._error.show()

    def _on_save(self) -> None:
        name = self._name.text().strip()
        if not name:
            self._fail("Cần đặt tên cho server")
            return
        kind = str(self._kind.currentData())
        if kind == "stdio":
            parts = self._command.text().split()
            if not parts:
                self._fail("Cần nhập lệnh để chạy server")
                return
        elif not self._url.text().strip().startswith(("http://", "https://")):
            self._fail("Địa chỉ phải bắt đầu bằng http:// hoặc https://")
            return
        self.accept()

    def values(self) -> dict[str, Any]:
        kind = str(self._kind.currentData())
        parts = self._command.text().split()
        headers: dict[str, str] = {}
        for line in self._headers.toPlainText().splitlines():
            key, separator, value = line.partition(":")
            if separator and key.strip():
                headers[key.strip()] = value.strip()
        return {
            "name": self._name.text().strip(),
            "kind": kind,
            "command": parts[0] if (kind == "stdio" and parts) else "",
            "args": parts[1:] if kind == "stdio" else [],
            "url": self._url.text().strip() if kind == "http" else "",
            "headers": headers if kind == "http" else {},
        }


class _ServerCard(QFrame):
    def __init__(self, title: str, subtitle: str, tools: list[str], parent=None) -> None:
        super().__init__(parent)
        self.setProperty("class", "card")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(*CARD_MARGINS)
        layout.setSpacing(CARD_SPACING)

        self.header = QHBoxLayout()
        self.header.setSpacing(TOOLBAR_SPACING)
        heading = QVBoxLayout()
        heading.setSpacing(SPACE["3xs"])
        name = QLabel(title)
        name.setProperty("class", "card-title")
        heading.addWidget(name)
        detail = QLabel(subtitle)
        detail.setWordWrap(True)
        detail.setProperty("class", "muted")
        heading.addWidget(detail)
        self.header.addLayout(heading, 1)
        # The subtitle wraps, so the controls the caller appends hang from the top edge.
        self.header.setAlignment(heading, Qt.AlignmentFlag.AlignTop)
        layout.addLayout(self.header)

        if tools:
            listing = QVBoxLayout()
            listing.setSpacing(SPACE["xs"])
            for tool in tools:
                visible = tool in AGENT_TOOLS
                row = QHBoxLayout()
                row.setSpacing(TOOLBAR_SPACING)
                label = QLabel(tool)
                label.setProperty("class", "code")
                row.addWidget(label, 1, Qt.AlignmentFlag.AlignVCenter)
                if tool in ARTIFACT_TOOLS:
                    caption = "Tạo tệp · agent dùng được"
                elif visible:
                    caption = "Chỉ đọc · agent dùng được"
                else:
                    caption = "Chỉ dành cho ứng dụng"
                mark = QLabel(caption)
                # Accent marks the subset the model is actually handed; the rest is neutral.
                mark.setProperty("class", "chip-active" if visible else "chip")
                row.addWidget(mark, 0, Qt.AlignmentFlag.AlignVCenter)
                listing.addLayout(row)
            layout.addLayout(listing)
        else:
            empty = QLabel("Chưa liệt kê được công cụ nào.")
            empty.setProperty("class", "muted")
            layout.addWidget(empty)


class McpView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._loading = False

        root = QVBoxLayout(self)
        # The settings tab host supplies the page padding; a second inset would double it.
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(PAGE_SPACING)

        heading = QHBoxLayout()
        heading.setSpacing(TOOLBAR_SPACING)
        titles = QVBoxLayout()
        titles.setSpacing(SPACE["3xs"])
        eyebrow = QLabel("Công cụ")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("MCP server")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel(
            "Mọi công cụ của trợ lý đến từ đây. Công cụ ghi và xóa chỉ chạy khi bạn bấm."
        )
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        heading.addLayout(titles, 1)
        add = QPushButton("Thêm server")
        add.setIcon(icon("plus"))
        add.setProperty("class", "primary")
        add.clicked.connect(self._on_add)
        heading.addWidget(add, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

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
        self._ctx.run(self._load(), on_result=self._loaded, on_error=self._failed)

    async def _load(self) -> tuple[dict[str, list[str]], list[McpServerRecord]]:
        hub = self._ctx.services.mcp
        grouped: dict[str, list[str]] = {}
        if hub is not None:
            for server_id in hub.servers():
                grouped[server_id] = []
            # One flat list of aliases comes back; the dotted name says which server owns
            # it, and the longest matching prefix wins so `rag.graph` beats `rag`.
            tools = await hub.tools(allow=None)
            owners = sorted(grouped, key=len, reverse=True)
            for tool in tools:
                dotted = name_for(str(tool.name))
                owner = next(
                    (item for item in owners if dotted.startswith(f"{item}.")),
                    "core" if "core" in grouped else "",
                )
                if owner:
                    grouped[owner].append(dotted)
        configured = await repositories.list_mcp_servers(self._ctx.database)
        return grouped, configured

    def _loaded(self, payload: tuple[dict[str, list[str]], list[McpServerRecord]]) -> None:
        self._loading = False
        grouped, configured = payload
        while self._rows.count() > 1:
            item = self._rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

        self._add_heading("Đang gắn kết")
        if not grouped:
            note = QLabel("Chưa có server nào được gắn. Khởi động lại ứng dụng để thử lại.")
            note.setProperty("class", "muted")
            self._rows.insertWidget(self._rows.count() - 1, note)
        for server_id in sorted(grouped):
            tools = sorted(grouped[server_id])
            external = server_id.startswith(f"{EXTERNAL_PREFIX}.")
            origin = "server ngoài" if external else "nội bộ, trong tiến trình"
            card = _ServerCard(
                server_id,
                f"{len(tools)} công cụ · {origin}",
                tools,
                self._canvas,
            )
            self._rows.insertWidget(self._rows.count() - 1, card)

        self._add_heading("Server bạn đã thêm")
        editable = [record for record in configured if str(record.kind) != "builtin"]
        if not editable:
            note = QLabel(
                "Chưa có server ngoài nào. Thêm một tiến trình stdio hoặc một địa chỉ HTTP "
                "để mở rộng bộ công cụ."
            )
            note.setWordWrap(True)
            note.setProperty("class", "muted")
            self._rows.insertWidget(self._rows.count() - 1, note)
        for record in editable:
            self._rows.insertWidget(self._rows.count() - 1, self._configured_card(record))

    def _add_heading(self, text: str) -> None:
        label = QLabel(text)
        label.setProperty("class", "section-label")
        self._rows.insertWidget(self._rows.count() - 1, label)

    def _configured_card(self, record: McpServerRecord) -> QFrame:
        target = record.url or " ".join([record.command, *record.args]).strip()
        card = _ServerCard(
            record.name,
            f"{KIND_LABELS.get(str(record.kind), str(record.kind))} · {target}",
            [],
            self._canvas,
        )
        toggle = QCheckBox("Đang bật" if record.enabled else "Đang tắt")
        toggle.setChecked(record.enabled)
        toggle.toggled.connect(lambda value: self._set_enabled(record, value))
        card.header.addWidget(toggle, 0, Qt.AlignmentFlag.AlignTop)
        remove = ConfirmButton("Xóa", "Xác nhận xóa")
        remove.confirmed.connect(lambda: self._remove(record))
        card.header.addWidget(remove, 0, Qt.AlignmentFlag.AlignTop)
        return card

    def _failed(self, exc: BaseException) -> None:
        self._loading = False
        self._ctx.toast(str(exc) or "Không đọc được danh sách MCP server", "error")

    # --- actions ----------------------------------------------------------

    def _on_add(self) -> None:
        dialog = McpServerDialog(self)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        self._ctx.run(
            create_mcp_server(self._ctx.database, dialog.values()),
            on_result=lambda _: self._added(),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không thêm được server", "error"),
        )

    def _added(self) -> None:
        self._ctx.toast("Đã thêm server · khởi động lại ứng dụng để nối", "success")
        self.refresh()

    def _set_enabled(self, record: McpServerRecord, enabled: bool) -> None:
        self._ctx.run(
            repositories.set_mcp_server_enabled(self._ctx.database, record.id, enabled),
            on_result=lambda _: self.refresh(),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không đổi được trạng thái", "error"),
        )

    def _remove(self, record: McpServerRecord) -> None:
        self._ctx.run(
            delete_mcp_server(self._ctx.database, record.id),
            on_result=lambda _: self._removed(record.name),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không xóa được server", "error"),
        )

    def _removed(self, name: str) -> None:
        self._ctx.toast(f"Đã xóa {name}", "success")
        self.refresh()
