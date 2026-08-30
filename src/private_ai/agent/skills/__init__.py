"""Packaged capability the agent loads on demand.

A skill is a directory holding ``SKILL.md`` — YAML frontmatter plus a markdown body —
following the Agent Skills convention users already know from Claude Code. Only the
name and description of an enabled skill reach the system prompt; the body is injected
when the skill is activated for a turn, and sibling files are never read up front.
"""

from __future__ import annotations

from private_ai.agent.skills.loader import (
    BUILTIN_SKILLS_DIR,
    Skill,
    SkillError,
    discover_skills,
    parse_skill,
)
from private_ai.agent.skills.registry import SkillRegistry

__all__ = [
    "BUILTIN_SKILLS_DIR",
    "Skill",
    "SkillError",
    "SkillRegistry",
    "discover_skills",
    "parse_skill",
]
