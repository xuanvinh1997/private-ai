import { createSignal } from "solid-js";
import type { MermaidConfig } from "mermaid";
import { S, t } from "./i18n";
import type { Msg } from "./i18n";
import { theme } from "./theme";

/** Mermaid wrapper: lazily imported (~1 MB), `securityLevel: "strict"` with `htmlLabels: false` because diagram
 * source is model-written, and one render queue because `mermaid.render` keeps global state keyed by id. */

export type DiagramRender = { ok: true; svg: string } | { ok: false; message: string };

type MermaidModule = typeof import("mermaid").default;

let pending: Promise<MermaidModule> | null = null;

/** Loaded once per session; a failure allows a retry at the next render. */
function load(): Promise<MermaidModule> {
  if (pending === null) {
    pending = import("mermaid")
      .then((mod) => mod.default)
      .catch((err) => {
        pending = null;
        throw err;
      });
  }
  return pending;
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function prefersDark(): boolean {
  try {
    return window.matchMedia(DARK_QUERY).matches;
  } catch {
    return false;
  }
}

const [systemDark, setSystemDark] = createSignal(prefersDark());

try {
  window.matchMedia(DARK_QUERY).addEventListener("change", (event) => setSystemDark(event.matches));
} catch {
  /* no matchMedia here: assume light, which affects colours only */
}

/** Whether we are in dark mode; `theme()` alone is not enough because "system" stamps nothing on `<html>`. */
export function isDark(): boolean {
  const choice = theme();
  return choice === "dark" || (choice === "system" && systemDark());
}

function palette(): Record<string, string> {
  const style = getComputedStyle(document.documentElement);
  // Only read tokens with primitive values; `color-mix` tokens stay function strings that mermaid cannot parse.
  const read = (name: string, fallback: string): string => {
    const value = style.getPropertyValue(name).trim();
    return value === "" ? fallback : value;
  };
  return {
    bg: read("--bg", "#f5f0e8"),
    surface: read("--surface", "#faf9f5"),
    surfaceSoft: read("--surface-soft", "#f7f3ec"),
    ink: read("--ink", "#141413"),
    text: read("--text", "#3d3d3a"),
    muted: read("--muted", "#57554f"),
    line: read("--line", "#ddd2c2"),
    lineStrong: read("--line-strong", "#948468"),
    accent: read("--accent", "#ba5a3a"),
    accentSoft: read("--accent-soft", "#f7e6df"),
    accentInk: read("--accent-ink", "#8c4025"),
    warnSoft: read("--warn-soft", "#f4ebd8"),
    warn: read("--warn", "#8a6114"),
    dangerSoft: read("--danger-soft", "#f6e2e0"),
    danger: read("--danger", "#b03a35"),
    font: read("--font-ui", "sans-serif"),
  };
}

/** Config rebuilt before *every* render, coloured from repo tokens so a diagram does not look pasted in. */
function config(): MermaidConfig {
  const c = palette();
  return {
    startOnLoad: false,
    securityLevel: "strict",
    htmlLabels: false,
    // Mermaid injects its own red error box; we render failures ourselves so the source can sit beside the message.
    suppressErrorRendering: true,
    theme: "base",
    fontFamily: c.font,
    flowchart: { htmlLabels: false, useMaxWidth: true, curve: "basis", padding: 12 },
    sequence: { useMaxWidth: true, wrap: true },
    class: { htmlLabels: false, useMaxWidth: true },
    state: { useMaxWidth: true },
    er: { useMaxWidth: true },
    themeVariables: {
      darkMode: isDark(),
      background: c.surface,
      fontFamily: c.font,
      fontSize: "13px",
      primaryColor: c.accentSoft,
      primaryTextColor: c.accentInk,
      primaryBorderColor: c.accent,
      secondaryColor: c.surfaceSoft,
      secondaryTextColor: c.text,
      secondaryBorderColor: c.line,
      tertiaryColor: c.bg,
      tertiaryTextColor: c.text,
      tertiaryBorderColor: c.line,
      mainBkg: c.surfaceSoft,
      nodeBorder: c.lineStrong,
      nodeTextColor: c.ink,
      titleColor: c.ink,
      textColor: c.text,
      lineColor: c.lineStrong,
      edgeLabelBackground: c.surface,
      clusterBkg: c.bg,
      clusterBorder: c.line,
      labelBackground: c.surface,
      noteBkgColor: c.warnSoft,
      noteTextColor: c.warn,
      noteBorderColor: c.warn,
      errorBkgColor: c.dangerSoft,
      errorTextColor: c.danger,
    },
  };
}

let queue: Promise<unknown> = Promise.resolve();
let seq = 0;

function reason(err: unknown): string {
  if (err instanceof Error && err.message !== "") return err.message;
  if (typeof err === "string" && err !== "") return err;
  // Mermaid's `DetailedError` is not a real Error; its `str` holds the offending line.
  if (err !== null && typeof err === "object" && "str" in err) return String(err.str);
  return t(S.libs.diagram.parseFailed);
}

/** Render a diagram; never throws, since bad syntax is a result. `parse` runs first because a failed `render` litters `body`. */
export function renderDiagram(source: string): Promise<DiagramRender> {
  const job = async (): Promise<DiagramRender> => {
    let mermaid: MermaidModule;
    try {
      mermaid = await load();
    } catch (err) {
      console.error("failed to load mermaid", err);
      return { ok: false, message: t(S.libs.diagram.loadFailed) };
    }

    const id = `pai-mermaid-${(seq += 1)}`;
    try {
      mermaid.initialize(config());
      await mermaid.parse(source);
      const { svg } = await mermaid.render(id, source);
      return { ok: true, svg };
    } catch (err) {
      return { ok: false, message: reason(err) };
    } finally {
      // Mermaid's scratch node in `body` is cleaned up on success but not on a mid-render failure.
      document.getElementById(id)?.remove();
      document.getElementById(`d${id}`)?.remove();
    }
  };

  const next = queue.then(job, job);
  queue = next.then(
    () => undefined,
    () => undefined,
  );
  return next;
}

/** Keys are mermaid syntax keywords and are never translated; the values are user-facing labels. */
const KIND_LABEL: Record<string, Msg> = {
  flowchart: S.libs.diagram.flowchart,
  graph: S.libs.diagram.flowchart,
  sequencediagram: S.libs.diagram.sequence,
  classdiagram: S.libs.diagram.class,
  statediagram: S.libs.diagram.state,
  erdiagram: S.libs.diagram.entity,
  journey: S.libs.diagram.journey,
  gantt: S.libs.diagram.gantt,
  pie: S.libs.diagram.pie,
  mindmap: S.libs.diagram.mindmap,
  timeline: S.libs.diagram.timeline,
  gitgraph: S.libs.diagram.gitgraph,
  quadrantchart: S.libs.diagram.quadrant,
  requirementdiagram: S.libs.diagram.requirement,
  block: S.libs.diagram.block,
  sankey: S.libs.diagram.sankey,
  xychart: S.libs.diagram.xy,
  architecture: S.libs.diagram.architecture,
  packet: S.libs.diagram.packet,
  c4context: S.libs.diagram.c4,
};

/** Diagram kind from the first declaration line, used for `aria-label`: mermaid SVG is otherwise unreadable aloud. */
export function diagramKind(source: string): string {
  let body = source.trimStart();
  // A `--- ... ---` frontmatter block precedes the declaration line; skip it.
  if (body.startsWith("---")) {
    const end = body.indexOf("\n---", 3);
    if (end !== -1) body = body.slice(end + 4);
  }
  for (const raw of body.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("%%")) continue;
    const token = /^([A-Za-z0-9]+)/.exec(line)?.[1];
    if (token === undefined) break;
    return t(KIND_LABEL[token.toLowerCase()] ?? S.libs.diagram.generic);
  }
  return t(S.libs.diagram.generic);
}
