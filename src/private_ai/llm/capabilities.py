"""What a model can do, and how to read a model's JSON reply.

Two sources of truth, in priority order: Ollama's ``/api/show`` reports capabilities
authoritatively, and everything else — OpenAI-compatible servers list nothing but a
name — falls back to sniffing tokens out of the identifier.
"""

from __future__ import annotations

import json
import re
from typing import Any

from private_ai.llm import ProviderUnavailable

__all__ = [
    "OLLAMA_CAPABILITIES",
    "decode_json_object",
    "infer_capabilities",
    "normalize_graph_result",
    "normalize_ollama_capabilities",
]

OLLAMA_CAPABILITIES = frozenset({"chat", "embedding", "vision", "tools", "thinking"})
# Ollama calls plain text generation "completion"; the rest of the app calls it chat.
_OLLAMA_ALIASES = {"completion": "chat"}

_VISION_TOKENS = (
    "-vl",
    ":vl",
    "clip",
    "gemma3",
    "gpt-4o",
    "gpt-5",
    "llava",
    "minicpm-v",
    "moondream",
    "o4-mini",
    "vision",
)


def infer_capabilities(descriptor: str) -> list[str]:
    """Guess from a model name plus whatever metadata the host volunteered."""
    value = descriptor.lower()
    if "embed" in value:
        return ["embedding"]
    if any(token in value for token in _VISION_TOKENS):
        return ["chat", "vision"]
    return ["chat"]


def normalize_ollama_capabilities(value: object) -> list[str]:
    if not isinstance(value, list):
        return []
    normalized: list[str] = []
    for capability in value:
        if not isinstance(capability, str):
            continue
        name = _OLLAMA_ALIASES.get(capability.lower(), capability.lower())
        if name in OLLAMA_CAPABILITIES and name not in normalized:
            normalized.append(name)
    return normalized


def decode_json_object(raw: Any) -> dict[str, Any]:
    """Read a JSON object out of a model reply that may be wrapped in prose or fences."""
    raw_text = str(raw or "").strip()
    start = raw_text.find("{")
    end = raw_text.rfind("}")
    if start >= 0 and end > start:
        raw_text = raw_text[start : end + 1]
    try:
        parsed = json.loads(raw_text)
    except json.JSONDecodeError as exc:
        raise ProviderUnavailable("Provider returned invalid graph extraction JSON") from exc
    if not isinstance(parsed, dict):
        raise ProviderUnavailable("Provider returned invalid graph extraction data")
    return parsed


def normalize_graph_result(parsed: dict[str, Any]) -> dict[str, list[dict[str, str]]]:
    """Fold a model's entity/relation reply into deduplicated, key-addressed records.

    Models name the same entity inconsistently and happily emit a relation whose endpoints
    were never declared, so entities are keyed by casefolded name and any endpoint that is
    missing gets synthesised rather than dropping the edge.
    """
    entities: dict[str, dict[str, str]] = {}
    entity_ids: dict[str, str] = {}
    raw_entities = parsed.get("entities", [])
    if not isinstance(raw_entities, list):
        raw_entities = []
    for item in raw_entities[:30]:
        if not isinstance(item, dict):
            continue
        name = re.sub(r"\s+", " ", str(item.get("name", "")).strip())[:120]
        if not name:
            continue
        key = name.casefold()
        entities[key] = {
            "key": key,
            "name": name,
            "kind": str(item.get("kind") or item.get("type") or "entity").strip()[:40] or "entity",
        }
        if item.get("id"):
            entity_ids[str(item["id"])] = name

    relations: list[dict[str, str]] = []
    seen_relations: set[tuple[str, str, str]] = set()
    raw_relations = parsed.get("relations", [])
    if not isinstance(raw_relations, list):
        raw_relations = []
    for item in raw_relations[:30]:
        if not isinstance(item, dict):
            continue
        source_value = str(item.get("source") or item.get("subject") or "")
        target_value = str(item.get("target") or item.get("object") or "")
        source_name = re.sub(r"\s+", " ", entity_ids.get(source_value, source_value).strip())[:120]
        target_name = re.sub(r"\s+", " ", entity_ids.get(target_value, target_value).strip())[:120]
        relation = re.sub(
            r"\s+",
            "_",
            str(item.get("relation") or item.get("predicate") or "related_to").strip().casefold(),
        )[:60]
        source_key = source_name.casefold()
        target_key = target_name.casefold()
        signature = (source_key, target_key, relation)
        if (
            not source_key
            or not target_key
            or source_key == target_key
            or signature in seen_relations
        ):
            continue
        seen_relations.add(signature)
        entities.setdefault(source_key, {"key": source_key, "name": source_name, "kind": "entity"})
        entities.setdefault(target_key, {"key": target_key, "name": target_name, "kind": "entity"})
        relations.append(
            {
                "source_key": source_key,
                "target_key": target_key,
                "relation": relation or "related_to",
            }
        )
    return {"entities": list(entities.values()), "relations": relations}
