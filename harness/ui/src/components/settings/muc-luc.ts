import type { IconName } from "../Icon";
import { LOCALES, S, t, type Msg } from "../../lib/i18n";

/** The settings table of contents: which pages exist, in which group, and how search finds them. It is data, not an array inside `SettingsView`, because search must see pages never opened; `id` is a key, everything readable is a `Msg`. */

export type SettingsPage =
  | "chung"
  | "phim-tat"
  | "provider"
  | "mcp"
  | "hook"
  | "quyen";

export interface PageMeta {
  id: SettingsPage;
  label: Msg;
  icon: IconName;
  /** The line under the big title; omitted on pages that open with their own `SectionHead`. */
  desc?: Msg;
}

export interface NavGroup {
  /** The first group has no title: it is the default place, and naming that is redundant. */
  title?: Msg;
  pages: PageMeta[];
}

/** Three groups, ordered by how often they are opened: providers first, since nothing works without it; integrations are what plug into the core; permissions stand alone because they change what the assistant may do. */
export const NAV: NavGroup[] = [
  {
    pages: [
      {
        id: "provider",
        label: S.settings.page.provider,
        icon: "server",
      },
      {
        id: "chung",
        label: S.settings.page.chung,
        icon: "monitor",
        desc: S.settings.page.chungDesc,
      },
      {
        id: "phim-tat",
        label: S.settings.page.phimTat,
        icon: "enter",
        desc: S.settings.page.phimTatDesc,
      },
    ],
  },
  {
    title: S.settings.group.integrations,
    pages: [
      { id: "mcp", label: S.settings.page.mcp, icon: "plug" },
      {
        id: "hook",
        label: S.settings.page.hook,
        icon: "tools",
        desc: S.settings.page.hookDesc,
      },
    ],
  },
  {
    title: S.settings.group.advanced,
    pages: [
      {
        id: "quyen",
        label: S.settings.page.quyen,
        icon: "hand",
        desc: S.settings.page.quyenDesc,
      },
    ],
  },
];

const ALL: PageMeta[] = NAV.flatMap((group) => group.pages);

export function pageMeta(id: SettingsPage): PageMeta {
  // Unreachable: the list is a constant and `SettingsPage` is a closed union; it avoids a `!`.
  return ALL.find((page) => page.id === id) ?? ALL[0]!;
}

/** One search hit: which page it leads to, and why it matched. */
export interface SearchHit {
  page: SettingsPage;
  label: Msg;
  desc: Msg;
}

/** What the search box can see; written by hand so it also covers rows that only exist inside a dialog, and deliberately excluding per-machine data such as provider and MCP server names. */
export const SEARCH_INDEX: SearchHit[] = [
  {
    page: "chung",
    label: S.settings.index.themeLabel,
    desc: S.settings.index.themeDesc,
  },
  {
    page: "chung",
    label: S.settings.index.localeLabel,
    desc: S.settings.index.localeDesc,
  },
  {
    page: "chung",
    label: S.settings.index.layoutLabel,
    desc: S.settings.index.layoutDesc,
  },
  {
    page: "phim-tat",
    label: S.settings.index.findSessionLabel,
    desc: S.settings.index.findSessionDesc,
  },
  {
    page: "phim-tat",
    label: S.settings.index.sendLabel,
    desc: S.settings.index.sendDesc,
  },
  {
    page: "provider",
    label: S.settings.index.providerLabel,
    desc: S.settings.index.providerDesc,
  },
  {
    page: "provider",
    label: S.settings.index.apiKeyLabel,
    desc: S.settings.index.apiKeyDesc,
  },
  {
    page: "provider",
    label: S.settings.index.baseUrlLabel,
    desc: S.settings.index.baseUrlDesc,
  },
  {
    page: "provider",
    label: S.settings.index.embedModelLabel,
    desc: S.settings.index.embedModelDesc,
  },
  {
    page: "provider",
    label: S.settings.index.chatModelLabel,
    desc: S.settings.index.chatModelDesc,
  },
  {
    page: "provider",
    label: S.settings.index.rerankLabel,
    desc: S.settings.index.rerankDesc,
  },
  {
    page: "provider",
    label: S.settings.index.rerankDepthLabel,
    desc: S.settings.index.rerankDepthDesc,
  },
  {
    page: "mcp",
    label: S.settings.index.mcpLabel,
    desc: S.settings.index.mcpDesc,
  },
  {
    page: "hook",
    label: S.settings.index.hookLabel,
    desc: S.settings.index.hookDesc,
  },
  {
    page: "quyen",
    label: S.settings.index.scopeLabel,
    desc: S.settings.index.scopeDesc,
  },
  {
    page: "quyen",
    label: S.settings.index.sandboxLabel,
    desc: S.settings.index.sandboxDesc,
  },
];

/** Strip Vietnamese diacritics, since people type without them and an exact-accent search returns nothing. */
export function khongDau(raw: string): string {
  return raw
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/đ/g, "d");
}

/** A message's search text: the active locale first, since that is what is on screen, then every other locale, so an English term still finds the row. */
function tuKhoa(msg: Msg): string {
  return [t(msg), ...LOCALES.map((ma) => msg[ma])].join(" ");
}

/** Filter the index by what was typed; an empty string returns nothing, not everything. */
export function timTrongCaiDat(query: string): SearchHit[] {
  const needle = khongDau(query.trim());
  if (needle === "") return [];
  return SEARCH_INDEX.filter((hit) => {
    // Match label and description alike: half of what people look for has no name of its own.
    const hay = khongDau(
      `${tuKhoa(hit.label)} ${tuKhoa(hit.desc)} ${tuKhoa(pageMeta(hit.page).label)}`,
    );
    return hay.includes(needle);
  });
}
