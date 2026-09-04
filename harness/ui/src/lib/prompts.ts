import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import { S, t } from "./i18n";
import type { Msg } from "./i18n";
import type { ProjectKind, PromptSeeds } from "./protocol";

/** Nothing to build suggestions from, which is a state rather than an error. */
export const NO_SEEDS: PromptSeeds = { symbols: [], directories: [], documents: [] };

/** Seed material from the open project; errors are swallowed because this runs on screen build, not after a click. */
export async function promptSeeds(): Promise<PromptSeeds> {
  if (!inTauri()) return NO_SEEDS;
  try {
    return await invoke<PromptSeeds>("prompt_seeds");
  } catch (err) {
    console.error("failed to fetch prompt seeds", err);
    return NO_SEEDS;
  }
}

/** Suggestions for *code* projects: each touches a different tool, and none names anything specific to this repo. */
const SUGGESTIONS: Msg[] = [
  S.libs.prompt.codeArchitecture,
  S.libs.prompt.codeTests,
  S.libs.prompt.codeChanges,
  S.libs.prompt.codeUntested,
  S.libs.prompt.codeErrors,
];

/** Suggestions for *docs* projects, which only get `rag` tools, and which assume nothing about the library contents. */
const SUGGESTIONS_TAI_LIEU: Msg[] = [
  S.libs.prompt.docsList,
  S.libs.prompt.docsSummaries,
  S.libs.prompt.docsTopics,
  S.libs.prompt.docsQuote,
];

/** Suggestions with *no project open*: no disk tools are plugged in, so each must be answerable from model knowledge. */
const SUGGESTIONS_KHONG_DU_AN: Msg[] = [
  S.libs.prompt.idleAsync,
  S.libs.prompt.idleRegex,
  S.libs.prompt.idleDatabase,
  S.libs.prompt.idleRebase,
];

/** A *ceiling* on chips, not a fixed count: padding to five with a generic line would waste what the chips are for. */
const SO_CHIP = 5;

/** Questions built from the user's own repo: three tools, and each names something the index just confirmed exists. */
function tuMaNguon(seeds: PromptSeeds): string[] {
  const ra: string[] = [];
  const [sym1, sym2] = seeds.symbols;
  if (sym1) ra.push(t(S.libs.prompt.symbolWhat, { name: sym1 }));
  if (sym2) ra.push(t(S.libs.prompt.symbolCallers, { name: sym2 }));
  const thu_muc = seeds.directories[0];
  if (thu_muc) ra.push(t(S.libs.prompt.dirContents, { path: thu_muc }));
  return ra;
}

/** Questions built from the user's library; the compare prompt only appears once there are two documents. */
function tuTaiLieu(titles: string[]): string[] {
  const ra: string[] = [];
  const [t1, t2] = titles;
  if (t1) ra.push(t(S.libs.prompt.docSummary, { title: t1 }));
  if (t1 && t2) ra.push(t(S.libs.prompt.docCompare, { first: t1, second: t2 }));
  return ra;
}

/** Dynamic prompts first, then the static set, capped at [`SO_CHIP`]; with no seeds the static set alone is returned. */
export function goiY(kind: ProjectKind | null, seeds: PromptSeeds): string[] {
  const tinh =
    kind === null
      ? SUGGESTIONS_KHONG_DU_AN
      : kind === "docs"
        ? SUGGESTIONS_TAI_LIEU
        : SUGGESTIONS;
  const dong = kind === "docs" ? tuTaiLieu(seeds.documents) : kind === "code" ? tuMaNguon(seeds) : [];

  const ra: string[] = [];
  for (const cau of [...dong, ...tinh.map((msg) => t(msg))]) {
    if (ra.length >= SO_CHIP) break;
    if (!ra.includes(cau)) ra.push(cau);
  }
  return ra;
}
