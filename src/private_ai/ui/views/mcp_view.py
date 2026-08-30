"""Mounted MCP servers and the tools they publish.

The seven built-ins are mounted in process and cannot be removed — they *are* the
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
from private_ai.mcp.client import EXTERNAL_PREFIX, READ_ONLY_TOOLS
from private_ai.ui.icons import icon
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


class McpServerDialog(QDialog):
    """Describe one external server: a command to spawn, or a URL to dial."""

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.setModal(True)
        self.setWindowTitle("Thêm MCP server")
        self.setMinimumWidth(500)

        layout = QVBoxLayout(self)
        layout.setSpacing(10)

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

        layout.addWidget(QLabel("Tên"))
        self._name = QLineEdit()
        self._name.setPlaceholderText("vi-du-server")
        layout.addWidget(self._name)

        layout.addWidget(QLabel("Kiểu kết nối"))
        self._kind = QComboBox()
        self._kind.addItem(KIND_LABELS["stdio"], "stdio")
        self._kind.addItem(KIND_LABELS["http"], "http")
        self._kind.currentIndexChanged.connect(self._sync)
        layout.addWidget(self._kind)

        self._command_label = QLabel("Lệnh và tham số")
        self._command = QLineEdit()
        self._command.setPlaceholderText("uvx some-mcp-server --flag")
        layout.addWidget(self._command_label)
        layout.addWidget(self._command)

        self._url_label = QLabel("Địa chỉ HTTP")
        self._url = QLineEdit()
        self._url.setPlaceholderText("https://example.com/mcp")
        layout.addWidget(self._url_label)
        layout.addWidget(self._url)

        self._headers_label = QLabel("Header (mỗi dòng một cặp KEY: value)")
        self._headers = QPlainTextEdit()
        self._headers.setFixedHeight(70)
        layout.addWidget(self._headers_label)
        layout.addWidget(self._headers)

        self._error = QLabel("")
        self._error.setWordWrap(True)
        self._error.setProperty("class", "danger")
        self._error.hide()
        layout.addWidget(self._error)

        row = QHBoxLayout()
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
        self._command_label.setVisible(stdio)
        self._command.setVisible(stdio)
        for widget in (self._url_label, self._url, self._headers_label, self._headers):
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
        layout.setSpacing(6)

        self.header = QHBoxLayout()
        heading = QVBoxLayout()
        heading.setSpacing(2)
        name = QLabel(title)
        name.setProperty("class", "subtitle")
        heading.addWidget(name)
        detail = QLabel(subtitle)
        detail.setWordWrap(True)
        detail.setProperty("class", "muted")
        heading.addWidget(detail)
        self.header.addLayout(heading, 1)
        layout.addLayout(self.header)

        if tools:
            for tool in tools:
                visible = tool in READ_ONLY_TOOLS
                row = QHBoxLayout()
                row.setSpacing(8)
                label = QLabel(tool)
                label.setProperty("class", "code")
                row.addWidget(label, 1)
                mark = QLabel("Chỉ đọc · agent dùng được" if visible else "Chỉ dành cho ứng dụng")
                mark.setProperty("class", "chip-active" if visible else "chip")
                row.addWidget(mark, 0)
                layout.addLayout(row)
        else:
            empty = QLabel("Chưa liệt kê được công cụ nào.")
            empty.setProperty("class", "faint")
            layout.addWidget(empty)


class McpView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._loading = False

        root = QVBoxLayout(self)
        root.setContentsMargins(4, 4, 4, 4)
        root.setSpacing(12)

        heading = QHBoxLayout()
        titles = QVBoxLayout()
        eyebrow = QLabel("Công cụ")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("MCP server")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel(
            "Mọi công cụ mà trợ lý có đều đến từ một MCP server. Công cụ ghi hoặc xóa không "
            "bao giờ được đưa cho mô hình: chúng chỉ chạy khi bạn tự bấm trong ứng dụng."
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
            note.setProperty("class", "faint")
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
            note.setProperty("class", "faint")
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
