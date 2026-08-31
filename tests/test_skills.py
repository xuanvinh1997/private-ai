"""SKILL.md packs: parsing, progressive disclosure, persistence and shadowing."""

from __future__ import annotations

from pathlib import Path

import pytest

from private_ai.agent.skills.loader import (
    BUILTIN_SKILLS_DIR,
    SKILL_FILENAME,
    SkillError,
    discover_skills,
    parse_frontmatter,
    parse_skill,
    render_skill_file,
)
from private_ai.agent.skills.registry import SkillRegistry
from private_ai.config import Settings
from private_ai.core.database import Database

SAMPLE = """---
name: tra-cuu-hop-dong
title: Tra cứu hợp đồng
description: Tìm điều khoản trong hợp đồng và trích dẫn đúng số điều.
version: 2.1.0
tools: [rag.keyword.search, documents.list]
strategy: keyword
keywords: [hợp đồng, điều khoản]
---

## Quy trình
1. Tìm bằng `rag.keyword.search`.
2. Dẫn nguồn theo tên tệp.
"""


def write_skill(root: Path, name: str, body: str = SAMPLE) -> Path:
    directory = root / name
    directory.mkdir(parents=True, exist_ok=True)
    (directory / SKILL_FILENAME).write_text(body, encoding="utf-8")
    return directory


# --- parsing --------------------------------------------------------------


def test_frontmatter_is_split_from_the_markdown_body() -> None:
    meta, body = parse_frontmatter(SAMPLE)
    assert meta["name"] == "tra-cuu-hop-dong"
    assert meta["tools"] == ["rag.keyword.search", "documents.list"]
    assert body.startswith("## Quy trình")
    assert "---" not in body


def test_a_missing_frontmatter_block_is_an_error() -> None:
    with pytest.raises(SkillError, match="frontmatter"):
        parse_frontmatter("# Chỉ có markdown\n")


def test_parse_skill_reads_every_field(tmp_path: Path) -> None:
    directory = write_skill(tmp_path, "tra-cuu-hop-dong")
    skill = parse_skill(directory)

    assert skill.name == "tra-cuu-hop-dong"
    assert skill.title == "Tra cứu hợp đồng"
    assert skill.version == "2.1.0"
    assert skill.tools == ("rag.keyword.search", "documents.list")
    assert skill.strategy == "keyword"
    assert "hợp đồng" in skill.keywords
    assert skill.source == "user"
    assert skill.path == directory
    assert skill.skill_file == directory / SKILL_FILENAME


def test_pointing_at_the_skill_file_itself_also_works(tmp_path: Path) -> None:
    directory = write_skill(tmp_path, "tra-cuu-hop-dong")
    assert parse_skill(directory / SKILL_FILENAME).name == "tra-cuu-hop-dong"


def test_the_name_is_constrained_and_falls_back_to_the_directory(tmp_path: Path) -> None:
    unnamed = write_skill(
        tmp_path,
        "ghi-chu",
        "---\ndescription: Mô tả\n---\n\nHướng dẫn.\n",
    )
    assert parse_skill(unnamed).name == "ghi-chu"

    bad = write_skill(
        tmp_path,
        "sai",
        "---\nname: Tên Có Hoa\ndescription: Mô tả\n---\n\nHướng dẫn.\n",
    )
    with pytest.raises(SkillError, match="không hợp lệ"):
        parse_skill(bad)


def test_a_pack_without_a_description_or_a_body_is_rejected(tmp_path: Path) -> None:
    """Both are load-bearing: the description is what the model reads to choose it."""
    no_description = write_skill(tmp_path, "a", "---\nname: a\n---\n\nHướng dẫn.\n")
    with pytest.raises(SkillError, match="description"):
        parse_skill(no_description)

    no_body = write_skill(tmp_path, "b", "---\nname: b\ndescription: Mô tả\n---\n")
    with pytest.raises(SkillError, match="hướng dẫn"):
        parse_skill(no_body)


def test_a_directory_without_a_skill_file_is_not_a_skill(tmp_path: Path) -> None:
    (tmp_path / "trống").mkdir()
    with pytest.raises(SkillError, match=SKILL_FILENAME):
        parse_skill(tmp_path / "trống")


def test_resources_list_siblings_but_never_read_them(tmp_path: Path) -> None:
    directory = write_skill(tmp_path, "tra-cuu-hop-dong")
    (directory / "reference.md").write_text("chi tiết", encoding="utf-8")
    (directory / "scripts").mkdir()
    (directory / "scripts" / "run.py").write_text("print(1)", encoding="utf-8")
    (directory / "__pycache__").mkdir()
    (directory / "__pycache__" / "x.pyc").write_bytes(b"\x00")
    (directory / ".hidden").write_text("x", encoding="utf-8")

    names = {item.name for item in parse_skill(directory).resources()}

    assert names == {"reference.md", "run.py"}


def test_discovery_skips_a_malformed_pack_instead_of_failing(tmp_path: Path) -> None:
    """One bad file a user dropped in must not stop the app from starting."""
    write_skill(tmp_path, "tot")
    write_skill(tmp_path, "hong", "không có frontmatter\n")
    errors: list[tuple[Path, BaseException]] = []

    found = discover_skills([tmp_path], on_error=lambda path, exc: errors.append((path, exc)))

    assert [skill.name for skill in found] == ["tra-cuu-hop-dong"]
    assert [path.name for path, _ in errors] == ["hong"]


def test_discovery_ignores_dot_and_underscore_directories(tmp_path: Path) -> None:
    write_skill(tmp_path, ".ẩn")
    write_skill(tmp_path, "_nháp")
    write_skill(tmp_path, "tot")
    assert [skill.name for skill in discover_skills([tmp_path])] == ["tra-cuu-hop-dong"]


def test_a_later_root_shadows_an_earlier_one(tmp_path: Path) -> None:
    first = tmp_path / "a"
    second = tmp_path / "b"
    write_skill(first, "x", "---\nname: x\ndescription: bản gốc\n---\n\nGốc.\n")
    write_skill(second, "x", "---\nname: x\ndescription: bản thay thế\n---\n\nThay thế.\n")

    found = discover_skills([first, second])

    assert [skill.description for skill in found] == ["bản thay thế"]


# --- the registry ---------------------------------------------------------


@pytest.fixture
def registry(database: Database, settings: Settings) -> SkillRegistry:
    settings.skills_dir.mkdir(parents=True, exist_ok=True)
    return SkillRegistry(database, settings)


def test_refresh_discovers_the_builtin_packs_and_records_them(
    registry: SkillRegistry,
    database: Database,
) -> None:
    skills = registry.refresh()

    names = {skill.name for skill in skills}
    assert names >= {"tom-tat-tai-lieu", "truy-van-tri-thuc", "nghien-cuu-web"}
    assert all(skill.source == "builtin" for skill in skills)

    rows = {
        str(row["name"]): row
        for row in database.fetch_all("SELECT name, source, enabled, description FROM skills")
    }
    assert set(rows) == names
    assert all(row["enabled"] == 1 for row in rows.values())
    assert all(str(row["description"]).strip() for row in rows.values())


def test_the_enabled_flag_survives_a_refresh(registry: SkillRegistry) -> None:
    """It is the user's decision; everything else about a row is regenerated from disk."""
    registry.refresh()
    registry.set_enabled("tom-tat-tai-lieu", False)
    assert registry.is_enabled("tom-tat-tai-lieu") is False

    registry.refresh()

    assert registry.is_enabled("tom-tat-tai-lieu") is False
    assert "tom-tat-tai-lieu" not in {skill.name for skill in registry.enabled_skills()}


def test_toggling_an_unknown_skill_raises(registry: SkillRegistry) -> None:
    registry.refresh()
    with pytest.raises(KeyError):
        registry.set_enabled("không-tồn-tại", True)


def test_a_pack_whose_directory_disappeared_is_dropped(
    registry: SkillRegistry,
    settings: Settings,
    database: Database,
) -> None:
    write_skill(settings.skills_dir, "tam-thoi", "---\nname: tam-thoi\ndescription: M\n---\n\nH.\n")
    registry.refresh()
    assert registry.get("tam-thoi") is not None

    (settings.skills_dir / "tam-thoi" / SKILL_FILENAME).unlink()
    (settings.skills_dir / "tam-thoi").rmdir()
    registry.refresh()

    assert registry.get("tam-thoi") is None
    assert database.fetch_one("SELECT name FROM skills WHERE name = 'tam-thoi'") is None


def test_a_user_pack_shadows_the_builtin_of_the_same_name(
    registry: SkillRegistry,
    settings: Settings,
) -> None:
    """Built-ins are searched first precisely so a user pack can replace one."""
    assert registry.search_paths[0] == BUILTIN_SKILLS_DIR
    write_skill(
        settings.skills_dir,
        "tom-tat-tai-lieu",
        "---\nname: tom-tat-tai-lieu\ntitle: Bản của tôi\n"
        "description: Cách tóm tắt của riêng tôi.\n---\n\nLàm theo cách của tôi.\n",
    )

    registry.refresh()
    shadowing = registry.get("tom-tat-tai-lieu")

    assert shadowing is not None
    assert shadowing.title == "Bản của tôi"
    assert shadowing.source == "user"
    assert shadowing.path.is_relative_to(settings.skills_dir)
    # Exactly one row, not two: the name is the identity.
    assert [skill.name for skill in registry.all_skills()].count("tom-tat-tai-lieu") == 1


def test_extra_skill_paths_are_searched_too(
    database: Database,
    settings: Settings,
    tmp_path: Path,
) -> None:
    extra = tmp_path / "kho-ngoai"
    write_skill(extra, "rieng", "---\nname: rieng\ndescription: Của riêng.\n---\n\nH.\n")
    settings.skill_paths = str(extra)

    registry = SkillRegistry(database, settings)
    registry.refresh()

    assert registry.get("rieng") is not None


# --- authoring ------------------------------------------------------------


def test_a_rendered_pack_reads_back_as_the_fields_that_went_in(tmp_path: Path) -> None:
    """The writer and the reader are one round trip, punctuation and all."""
    text = render_skill_file(
        name="tom-tat-hop-dong",
        title="Tóm tắt hợp đồng",
        description='Rút gọn hợp đồng: điều khoản, "rủi ro" và mốc thời gian.',
        body="1. Đọc toàn văn.\n2. Liệt kê nghĩa vụ.",
        keywords=["hợp đồng", "rủi ro"],
    )
    directory = write_skill(tmp_path, "tom-tat-hop-dong", text)

    skill = parse_skill(directory)

    assert skill.name == "tom-tat-hop-dong"
    assert skill.title == "Tóm tắt hợp đồng"
    assert skill.description == 'Rút gọn hợp đồng: điều khoản, "rủi ro" và mốc thời gian.'
    assert skill.keywords == ("hợp đồng", "rủi ro")
    assert skill.body.startswith("1. Đọc toàn văn.")


def test_rendering_rejects_what_the_loader_would_reject(tmp_path: Path) -> None:
    with pytest.raises(SkillError):
        render_skill_file(name="Tóm Tắt", description="mô tả", body="thân")
    with pytest.raises(SkillError):
        render_skill_file(name="tom-tat", description="", body="thân")
    with pytest.raises(SkillError):
        render_skill_file(name="tom-tat", description="mô tả", body="   ")


def test_creating_a_pack_writes_it_and_picks_it_up(registry: SkillRegistry) -> None:
    registry.refresh()

    skill = registry.create(
        name="tom-tat-hop-dong",
        title="Tóm tắt hợp đồng",
        description="Rút gọn hợp đồng dài thành điều khoản và mốc thời gian.",
        body="1. Đọc toàn văn.\n2. Liệt kê nghĩa vụ.",
    )

    assert skill.source == "user"
    assert skill.skill_file.is_file()
    assert registry.get("tom-tat-hop-dong") is skill
    # New packs arrive switched on, the same default a discovered one gets.
    assert registry.is_enabled("tom-tat-hop-dong") is True


def test_creating_over_an_existing_pack_is_refused(registry: SkillRegistry) -> None:
    registry.refresh()
    fields = {
        "name": "tom-tat-hop-dong",
        "description": "Rút gọn hợp đồng.",
        "body": "1. Đọc toàn văn.",
    }
    registry.create(**fields)

    with pytest.raises(SkillError):
        registry.create(**fields)


def test_an_invalid_name_leaves_nothing_behind(registry: SkillRegistry, settings: Settings) -> None:
    registry.refresh()

    with pytest.raises(SkillError):
        registry.create(name="Tóm Tắt", description="Rút gọn hợp đồng.", body="1. Đọc.")

    assert list(settings.skills_dir.iterdir()) == []


# --- progressive disclosure ----------------------------------------------


def test_the_catalog_carries_summaries_and_not_instructions(
    registry: SkillRegistry,
) -> None:
    """A hundred skills must cost a hundred one-line summaries, not a hundred documents."""
    skills = registry.refresh()
    catalog = registry.catalog_prompt()

    for skill in skills:
        assert skill.summary() in catalog
        assert skill.description in catalog
    # No skill body leaks into the always-on half of the prompt.
    longest = max(skills, key=lambda skill: len(skill.body))
    assert longest.body not in catalog


def test_activation_injects_the_full_body_and_frames_it_as_trusted(
    registry: SkillRegistry,
) -> None:
    registry.refresh()
    skill = registry.get("tom-tat-tai-lieu")
    assert skill is not None

    prompt = registry.activation_prompt([skill])

    assert skill.instructions() in prompt
    assert f'<skill name="{skill.name}"' in prompt
    assert f'version="{skill.version}"' in prompt
    assert 'strategy="summary"' in prompt
    assert "ĐÁNG TIN" in prompt
    # The mirror image of the untrusted-excerpt warning is stated in the same breath.
    assert "không đáng tin cậy" in prompt
    assert ", ".join(skill.tools) in prompt


def test_activating_nothing_produces_nothing(registry: SkillRegistry) -> None:
    registry.refresh()
    assert registry.activation_prompt([]) == ""


def test_the_catalog_is_empty_when_skills_are_switched_off(
    database: Database,
    settings: Settings,
) -> None:
    settings.skills_enabled = False
    registry = SkillRegistry(database, settings)
    registry.refresh()
    assert registry.enabled_skills() == []
    assert registry.catalog_prompt() == ""


def test_selection_matches_a_skill_by_its_words_and_stays_deterministic(
    registry: SkillRegistry,
) -> None:
    registry.refresh()

    chosen = registry.select("Tóm tắt toàn bộ tài liệu hợp đồng này")

    assert chosen
    assert chosen[0].name == "tom-tat-tai-lieu"
    assert [skill.name for skill in registry.select("Tóm tắt toàn bộ tài liệu hợp đồng này")] == [
        skill.name for skill in chosen
    ]


def test_selection_returns_nothing_for_a_question_no_skill_covers(
    registry: SkillRegistry,
) -> None:
    registry.refresh()
    assert registry.select("") == []
    assert registry.select("xyzzy plugh", limit=0) == []


def test_a_disabled_skill_is_never_selected(registry: SkillRegistry) -> None:
    registry.refresh()
    assert registry.select("Tóm tắt toàn bộ tài liệu")
    registry.set_enabled("tom-tat-tai-lieu", False)
    assert "tom-tat-tai-lieu" not in {
        skill.name for skill in registry.select("Tóm tắt toàn bộ tài liệu")
    }
