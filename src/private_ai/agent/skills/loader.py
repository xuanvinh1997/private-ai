"""Reading ``SKILL.md`` packs off disk.

The format is the Agent Skills convention: a directory whose ``SKILL.md`` opens with a
``---`` fenced YAML block and continues as markdown. Nothing else in the directory is
touched here — ``reference.md``, ``scripts/`` and ``templates/`` exist for the agent to
open through the file tools *after* it has decided the skill applies, which is the whole
point of progressive disclosure: a hundred skills cost a hundred one-line summaries in
the prompt, not a hundred documents.
"""

from __future__ import annotations

import json
import re
from collections.abc import Iterable, Sequence
from dataclasses import dataclass, field
from pathlib import Path

BUILTIN_SKILLS_DIR = Path(__file__).parent / "builtin"

SKILL_FILENAME = "SKILL.md"

# ``---`` on its own line, the block, then a closing ``---``. A leading BOM or blank
# lines are tolerated because editors add them.
_FRONTMATTER = re.compile(r"\A﻿?\s*---[ \t]*\r?\n(?P<meta>.*?)\r?\n---[ \t]*(?:\r?\n|\Z)", re.S)

_NAME_RE = re.compile(r"\A[a-z0-9][a-z0-9._-]{0,63}\Z")

_LIST_KEYS = frozenset({"tools", "keywords", "resources"})

_SKIP_RESOURCE_PARTS = frozenset({"__pycache__", ".git", ".DS_Store"})


class SkillError(ValueError):
    """A skill pack is malformed. Discovery skips it rather than failing the app."""


def _strip_scalar(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        if value[0] == '"':
            # Double-quoted YAML carries backslash escapes, and that is the form
            # :func:`render_skill_file` writes; JSON decodes exactly that subset.
            try:
                return json.loads(value)
            except ValueError:
                return value[1:-1]
        return value[1:-1]
    # A trailing ``# comment`` is only a comment when whitespace precedes it.
    return re.sub(r"\s+#.*\Z", "", value).strip()


def _parse_list(value: str) -> list[str]:
    value = value.strip()
    if value.startswith("[") and value.endswith("]"):
        value = value[1:-1]
    return [_strip_scalar(item) for item in value.split(",") if _strip_scalar(item)]


def _parse_frontmatter_fallback(text: str) -> dict[str, object]:
    """Strict reader for the flat ``key: value`` / ``key: [a, b]`` subset we document.

    Used only when PyYAML is absent — it arrives transitively today, but nothing in this
    project declares it, so the loader must not depend on that staying true.
    """
    meta: dict[str, object] = {}
    pending: str | None = None
    for raw in text.splitlines():
        line = raw.rstrip()
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if pending and line.lstrip().startswith("- "):
            items = meta.setdefault(pending, [])
            if isinstance(items, list):
                items.append(_strip_scalar(line.lstrip()[2:]))
            continue
        pending = None
        key, separator, value = line.partition(":")
        if not separator or key != key.strip() or not key.strip():
            raise SkillError(f"Không đọc được dòng frontmatter: {raw!r}")
        key = key.strip()
        value = value.strip()
        if not value:
            meta[key] = []
            pending = key
        elif value.startswith("[") or key in _LIST_KEYS:
            meta[key] = _parse_list(value)
        else:
            meta[key] = _strip_scalar(value)
    return meta


def validate_name(name: str) -> str:
    """The slug a pack is addressed by. Checked here so nothing invalid reaches disk."""
    name = name.strip()
    if not _NAME_RE.match(name):
        raise SkillError(
            f"Tên kỹ năng '{name}' không hợp lệ: chỉ dùng chữ thường, số, '.', '-' và '_'."
        )
    return name


def _scalar(value: str) -> str:
    """One frontmatter value, quoted only when leaving it bare would change its meaning.

    JSON is a subset of YAML for double-quoted strings, so ``json.dumps`` gives correct
    escaping without pulling in an emitter.
    """
    value = " ".join(value.split())
    if value and not re.search(r'^[-?:,\[\]{}#&*!|>\'"%@`]|[:#]\s|["\'\\]|\s$', value):
        return value
    return json.dumps(value, ensure_ascii=False)


def render_skill_file(
    *,
    name: str,
    description: str,
    body: str,
    title: str = "",
    version: str = "1.0.0",
    keywords: Sequence[str] = (),
) -> str:
    """A SKILL.md for a pack written in the app, validated before it is handed back."""
    name = validate_name(name)
    description = " ".join(description.split())
    body = body.strip()
    if not description:
        raise SkillError(f"Kỹ năng '{name}' thiếu trường 'description'.")
    if not body:
        raise SkillError(f"Kỹ năng '{name}' không có phần hướng dẫn.")
    lines = ["---", f"name: {name}"]
    if title.strip():
        lines.append(f"title: {_scalar(title)}")
    lines.append(f"description: {_scalar(description)}")
    lines.append(f"version: {_scalar(version)}")
    cleaned = [word for word in (item.strip() for item in keywords) if word]
    if cleaned:
        lines.append("keywords: [" + ", ".join(_scalar(word) for word in cleaned) + "]")
    lines.extend(["---", "", body, ""])
    return "\n".join(lines)


def parse_frontmatter(text: str) -> tuple[dict[str, object], str]:
    """Split a SKILL.md into its metadata mapping and its markdown body."""
    matched = _FRONTMATTER.match(text)
    if not matched:
        raise SkillError("SKILL.md thiếu khối frontmatter '---'.")
    block = matched.group("meta")
    body = text[matched.end() :]
    try:
        import yaml
    except ImportError:
        meta = _parse_frontmatter_fallback(block)
    else:
        try:
            loaded = yaml.safe_load(block)
        except yaml.YAMLError as exc:  # pragma: no cover - depends on the file on disk
            raise SkillError(f"Frontmatter YAML không hợp lệ: {exc}") from exc
        if loaded is None:
            loaded = {}
        if not isinstance(loaded, dict):
            raise SkillError("Frontmatter phải là một ánh xạ key: value.")
        meta = {str(key): value for key, value in loaded.items()}
    return meta, body.strip()


def _as_text(meta: dict[str, object], key: str, default: str = "") -> str:
    value = meta.get(key, default)
    if value is None:
        return default
    return str(value).strip()


def _as_list(meta: dict[str, object], key: str) -> tuple[str, ...]:
    value = meta.get(key)
    if value is None or value == "":
        return ()
    if isinstance(value, str):
        return tuple(_parse_list(value))
    if isinstance(value, Iterable):
        return tuple(str(item).strip() for item in value if str(item).strip())
    raise SkillError(f"Trường '{key}' phải là danh sách.")


@dataclass(frozen=True, slots=True)
class Skill:
    """One loaded skill pack.

    ``body`` is held in memory because it is small — a page of instructions — but it is
    handed to the model only through :meth:`instructions`, when the skill is activated.
    """

    name: str
    title: str
    description: str
    version: str
    body: str
    path: Path
    source: str = "builtin"
    tools: tuple[str, ...] = ()
    strategy: str | None = None
    keywords: tuple[str, ...] = field(default=())

    @property
    def skill_file(self) -> Path:
        return self.path / SKILL_FILENAME

    def summary(self) -> str:
        """The one line that goes into the system prompt by default."""
        return f"- {self.name}: {self.description}"

    def instructions(self) -> str:
        """The full body, injected only once the skill is activated for a turn."""
        return self.body

    def resources(self) -> list[Path]:
        """Sibling files the agent may open on its own. Never read here."""
        if not self.path.is_dir():
            return []
        found: list[Path] = []
        for item in sorted(self.path.rglob("*")):
            if not item.is_file() or item.name == SKILL_FILENAME:
                continue
            parts = set(item.relative_to(self.path).parts)
            if item.name.startswith(".") or parts & _SKIP_RESOURCE_PARTS:
                continue
            found.append(item)
        return found


def parse_skill(path: Path, *, source: str | None = None) -> Skill:
    """Load one skill directory (or the SKILL.md inside it)."""
    path = Path(path)
    directory = path.parent if path.name == SKILL_FILENAME else path
    skill_file = directory / SKILL_FILENAME
    if not skill_file.is_file():
        raise SkillError(f"Không tìm thấy {SKILL_FILENAME} trong {directory}")

    meta, body = parse_frontmatter(skill_file.read_text(encoding="utf-8"))
    name = validate_name(_as_text(meta, "name") or directory.name)
    description = _as_text(meta, "description")
    if not description:
        raise SkillError(f"Kỹ năng '{name}' thiếu trường 'description'.")
    if not body:
        raise SkillError(f"Kỹ năng '{name}' không có phần hướng dẫn.")

    strategy = _as_text(meta, "strategy") or None
    resolved_source = source or ("builtin" if _is_builtin(directory) else "user")
    return Skill(
        name=name,
        title=_as_text(meta, "title") or name,
        description=description,
        version=_as_text(meta, "version", "0.0.0") or "0.0.0",
        body=body,
        path=directory,
        source=resolved_source,
        tools=_as_list(meta, "tools"),
        strategy=strategy,
        keywords=_as_list(meta, "keywords"),
    )


def _is_builtin(directory: Path) -> bool:
    try:
        directory.resolve().relative_to(BUILTIN_SKILLS_DIR.resolve())
    except (ValueError, OSError):
        return False
    return True


def discover_skills(
    paths: Sequence[Path],
    *,
    source: str | None = None,
    on_error: object = None,
) -> list[Skill]:
    """Scan each root one level deep for ``*/SKILL.md``.

    Later roots win on a name clash, so a user pack shadowing a built-in replaces it
    rather than colliding with it. A malformed pack is skipped, not fatal: one bad file
    a user dropped in must not stop the app from starting.
    """
    found: dict[str, Skill] = {}
    for root in paths:
        root = Path(root)
        if not root.is_dir():
            continue
        for directory in sorted(p for p in root.iterdir() if p.is_dir()):
            if directory.name.startswith((".", "_")):
                continue
            if not (directory / SKILL_FILENAME).is_file():
                continue
            try:
                skill = parse_skill(directory, source=source)
            except (SkillError, OSError, UnicodeDecodeError) as exc:
                if callable(on_error):
                    on_error(directory, exc)
                continue
            found[skill.name] = skill
    return sorted(found.values(), key=lambda item: item.name)
