from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path

import pytest
from mcp.server.elicitation import AcceptedElicitation, DeclinedElicitation
from mcp.server.mcpserver import Context

from private_ai_api.config import Settings
from private_ai_api.mcp_server import FileAccessDecision, create_mcp_server


def _settings(tmp_path: Path, roots: list[Path] | None = None) -> Settings:
    import os

    return Settings(
        data_dir=tmp_path / "data",
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
        file_roots=os.pathsep.join(str(item) for item in (roots or [])),
    )


def _server(tmp_path: Path, roots: list[Path] | None = None):
    from conftest import FakeIndex

    return create_mcp_server(_settings(tmp_path, roots), FakeIndex())  # type: ignore[arg-type]


def _answer(allow: bool, remember: bool = False):
    """Stand in for the client's elicitation prompt with a fixed answer."""

    async def fake_elicit(self, message: str, schema):  # noqa: ANN001, ARG001
        if not allow:
            return DeclinedElicitation()
        return AcceptedElicitation(data=FileAccessDecision(allow=True, remember_folder=remember))

    return fake_elicit


@pytest.mark.asyncio
async def test_system_info_describes_this_machine(tmp_path: Path) -> None:
    server = _server(tmp_path)
    result = await server.call_tool("system.info", {})
    info = result.structured_content

    assert info["os"]["system"]
    assert info["cpu"]["logical_cores"] >= 1
    assert info["disk"]["free_bytes"] > 0
    assert info["gpu"]["capacity_bytes"] > 0
    assert info["app"]["data_dir"].endswith("data")


@pytest.mark.asyncio
async def test_system_time_agrees_with_the_clock(tmp_path: Path) -> None:
    server = _server(tmp_path)
    moment = (await server.call_tool("system.time", {})).structured_content

    parsed = datetime.fromisoformat(moment["utc_iso"])
    assert abs((datetime.now(UTC) - parsed).total_seconds()) < 60
    assert moment["date"] == datetime.now().astimezone().date().isoformat()
    assert moment["weekday"]


@pytest.mark.asyncio
async def test_a_configured_root_is_read_without_asking(tmp_path: Path) -> None:
    root = tmp_path / "notes"
    root.mkdir()
    (root / "plan.md").write_text("Kế hoạch quý 4", encoding="utf-8")
    server = _server(tmp_path, [root])

    result = await server.call_tool("files.read", {"path": str(root / "plan.md")})

    assert result.structured_content["content"] == "Kế hoạch quý 4"
    assert result.structured_content["truncated"] is False


@pytest.mark.asyncio
async def test_an_unlisted_path_asks_the_user_first(tmp_path: Path, monkeypatch) -> None:
    outside = tmp_path / "elsewhere"
    outside.mkdir()
    (outside / "diary.txt").write_text("riêng tư", encoding="utf-8")
    server = _server(tmp_path)

    monkeypatch.setattr(Context, "elicit", _answer(allow=False))
    with pytest.raises(Exception, match="declined"):
        await server.call_tool("files.read", {"path": str(outside / "diary.txt")})

    monkeypatch.setattr(Context, "elicit", _answer(allow=True))
    allowed = await server.call_tool("files.read", {"path": str(outside / "diary.txt")})
    assert allowed.structured_content["content"] == "riêng tư"


@pytest.mark.asyncio
async def test_remembering_a_folder_stops_the_second_prompt(tmp_path: Path, monkeypatch) -> None:
    outside = tmp_path / "project"
    outside.mkdir()
    (outside / "a.txt").write_text("một", encoding="utf-8")
    (outside / "b.txt").write_text("hai", encoding="utf-8")
    server = _server(tmp_path)

    monkeypatch.setattr(Context, "elicit", _answer(allow=True, remember=True))
    await server.call_tool("files.read", {"path": str(outside / "a.txt")})

    # A second read of a sibling must not need the prompt at all.
    async def refuse_to_ask(self, message: str, schema):  # noqa: ANN001, ARG001
        raise AssertionError("the user was asked twice for the same folder")

    monkeypatch.setattr(Context, "elicit", refuse_to_ask)
    second = await server.call_tool("files.read", {"path": str(outside / "b.txt")})
    assert second.structured_content["content"] == "hai"

    listed = await server.call_tool("files.allowed", {})
    assert [item["path"] for item in listed.structured_content["granted"]] == [str(outside)]


@pytest.mark.asyncio
async def test_traversal_out_of_a_root_still_needs_permission(
    tmp_path: Path,
    monkeypatch,
) -> None:
    root = tmp_path / "public"
    root.mkdir()
    (tmp_path / "secret.txt").write_text("khóa", encoding="utf-8")
    server = _server(tmp_path, [root])

    monkeypatch.setattr(Context, "elicit", _answer(allow=False))
    with pytest.raises(Exception, match="declined"):
        await server.call_tool("files.read", {"path": str(root / ".." / "secret.txt")})


@pytest.mark.asyncio
async def test_the_mcp_token_is_never_readable(tmp_path: Path) -> None:
    settings = _settings(tmp_path)
    settings.data_dir.mkdir(parents=True, exist_ok=True)
    settings.mcp_token_path.write_text("super-secret", encoding="utf-8")
    # The data dir is deliberately a configured root, so only the guard can refuse this.
    server = _server(tmp_path, [settings.data_dir])

    with pytest.raises(Exception, match="never readable"):
        await server.call_tool("files.read", {"path": str(settings.mcp_token_path)})


@pytest.mark.asyncio
async def test_binary_files_are_refused_rather_than_mangled(tmp_path: Path) -> None:
    root = tmp_path / "bin"
    root.mkdir()
    (root / "logo.png").write_bytes(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR")
    server = _server(tmp_path, [root])

    with pytest.raises(Exception, match="binary"):
        await server.call_tool("files.read", {"path": str(root / "logo.png")})


@pytest.mark.asyncio
async def test_listing_puts_folders_first_and_flags_truncation(tmp_path: Path) -> None:
    root = tmp_path / "many"
    root.mkdir()
    (root / "zeta").mkdir()
    for index in range(5):
        (root / f"file-{index}.txt").write_text(str(index), encoding="utf-8")
    server = _server(tmp_path, [root])

    listed = await server.call_tool("files.list", {"path": str(root), "limit": 3})
    body = listed.structured_content

    assert body["entries"][0]["type"] == "directory"
    assert len(body["entries"]) == 3
    assert body["total_entries"] == 6
    assert body["truncated"] is True


@pytest.mark.asyncio
async def test_a_client_that_cannot_ask_gets_a_pointer_to_the_setting(tmp_path: Path) -> None:
    """With no session there is nobody to prompt, so the refusal names the way in."""
    outside = tmp_path / "nope"
    outside.mkdir()
    (outside / "x.txt").write_text("x", encoding="utf-8")
    server = _server(tmp_path)

    with pytest.raises(Exception, match="PRIVATE_AI_FILE_ROOTS"):
        await server.call_tool("files.read", {"path": str(outside / "x.txt")})


@pytest.mark.asyncio
async def test_tool_failures_keep_their_message(tmp_path: Path) -> None:
    """SDK v2 only forwards the text of a ToolError.

    Raising a plain ValueError makes the SDK treat an anticipated refusal as a crash: the
    model is told nothing but "Error executing tool <name>", and the server logs a traceback
    for something that was never a bug. Every tool here has to keep its own wording.
    """
    from mcp.server.mcpserver.exceptions import ToolError, UnexpectedToolError

    server = _server(tmp_path)

    with pytest.raises(ToolError) as refused:
        await server.call_tool("documents.search", {"query": "x", "workspace_id": "nope"})

    assert not isinstance(refused.value, UnexpectedToolError)
    assert "call workspaces.list for valid IDs" in str(refused.value)

    with pytest.raises(ToolError) as unconfirmed:
        await server.call_tool("documents.delete", {"document_id": "any"})

    assert "requires confirmed=true" in str(unconfirmed.value)

class ToolCallingOllama:
    """Answers the first turn with a tool call, then with prose, like a real model does."""

    def __init__(self, tool_alias: str) -> None:
        self.tool_alias = tool_alias
        self.rounds = 0
        self.tools_offered: list[list[dict] | None] = []

    async def chat(self, request):
        self.tools_offered.append(request.tools)
        self.rounds += 1
        if self.rounds == 1:
            return {
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {"id": "c1", "function": {"name": self.tool_alias, "arguments": {}}}
                    ],
                },
                "done": True,
            }
        observed = [m for m in request.messages if m.role == "tool"]
        return {
            "message": {"role": "assistant", "content": f"Đã đọc: {observed[-1].content[:40]}"},
            "done": True,
        }


def test_chat_calls_a_real_mcp_tool(client) -> None:
    """The whole point: the model asks for a tool and the answer carries the tool's data."""
    fake = ToolCallingOllama("system__time")
    client.app.state.services.ai = fake
    conversation = client.post(
        "/api/v1/workspaces/personal/conversations",
        json={"model": "test-model"},
    ).json()

    response = client.post(
        f"/api/v1/conversations/{conversation['id']}/chat",
        json={"model": "test-model", "content": "hôm nay ngày mấy"},
    )

    assert response.status_code == 200
    assert fake.rounds == 2
    # Round one is offered the tools; the answer round still has them but chose not to call.
    assert fake.tools_offered[0]
    answer = response.json()["messages"][-1]["content"]
    assert "local_iso" in answer or "date" in answer


def test_write_tools_are_never_offered_to_chat(client) -> None:
    """A document could otherwise talk the model into deleting things it should not."""
    fake = ToolCallingOllama("system__time")
    client.app.state.services.ai = fake
    conversation = client.post(
        "/api/v1/workspaces/personal/conversations",
        json={"model": "test-model"},
    ).json()
    client.post(
        f"/api/v1/conversations/{conversation['id']}/chat",
        json={"model": "test-model", "content": "bất kỳ"},
    )

    offered = {spec["function"]["name"] for spec in fake.tools_offered[0] or []}
    assert "system__time" in offered
    for forbidden in ("documents__delete", "memory__forget", "documents__ingest_text"):
        assert forbidden not in offered
