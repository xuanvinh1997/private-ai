import { For } from "solid-js";
import { S, t, type Msg } from "../../lib/i18n";
import type { IconName } from "../Icon";
import { Row, RowGroup, SectionHead } from "./FormKit";

/** The Shortcuts page, kept apart because it changes nothing; the keys sit in the right column, where every page puts a row's current value. */

interface Shortcut {
  /** Key names, never translated: `Enter` on the keyboard is `Enter` in every language. */
  keys: string;
  what: Msg;
  desc?: Msg;
  /** The long sentence behind the row, kept in an `InfoDot` next to the label. */
  more?: Msg;
}

const NHOM: { title: Msg; desc: Msg; icon: IconName; items: Shortcut[] }[] = [
  {
    title: S.settings.shortcuts.navTitle,
    desc: S.settings.shortcuts.navDesc,
    icon: "search",
    items: [
      {
        keys: "⌘K / Ctrl+K",
        what: S.settings.shortcuts.findSession,
        desc: S.settings.shortcuts.findSessionDesc,
        more: S.settings.shortcuts.findSessionMore,
      },
      {
        keys: "Esc",
        what: S.settings.shortcuts.closeOpen,
        desc: S.settings.shortcuts.closeOpenDesc,
      },
    ],
  },
  {
    title: S.settings.shortcuts.composerTitle,
    desc: S.settings.shortcuts.composerDesc,
    icon: "pencil",
    items: [
      { keys: "Enter", what: S.settings.shortcuts.send },
      {
        keys: "Shift+Enter",
        what: S.settings.shortcuts.newLine,
        desc: S.settings.shortcuts.newLineDesc,
        more: S.settings.shortcuts.newLineMore,
      },
      {
        keys: "Enter",
        what: S.settings.shortcuts.queue,
        desc: S.settings.shortcuts.queueDesc,
        more: S.settings.shortcuts.queueMore,
      },
    ],
  },
  {
    title: S.settings.shortcuts.completionTitle,
    desc: S.settings.shortcuts.completionDesc,
    icon: "sparkle",
    items: [
      {
        keys: "@",
        what: S.settings.shortcuts.insertPath,
        desc: S.settings.shortcuts.insertPathDesc,
        more: S.settings.shortcuts.insertPathMore,
      },
      {
        keys: "/",
        what: S.settings.shortcuts.commandPalette,
        desc: S.settings.shortcuts.commandPaletteDesc,
        more: S.settings.shortcuts.commandPaletteMore,
      },
      { keys: "↑ / ↓", what: S.settings.shortcuts.moveInList },
      { keys: "Enter / Tab", what: S.settings.shortcuts.acceptHit },
      {
        keys: "Esc",
        what: S.settings.shortcuts.closeList,
        desc: S.settings.shortcuts.closeListDesc,
        more: S.settings.shortcuts.closeListMore,
      },
    ],
  },
];

export default function ShortcutsPage() {
  return (
    <div class="flex flex-col gap-2xl">
      <For each={NHOM}>
        {(group) => (
          <section class="flex flex-col gap-md">
            <SectionHead icon={group.icon} title={t(group.title)} desc={t(group.desc)} />
            <RowGroup>
              <For each={group.items}>
                {(item) => (
                  <Row
                    label={t(item.what)}
                    desc={item.desc === undefined ? undefined : t(item.desc)}
                    more={item.more === undefined ? undefined : t(item.more)}
                    control={() => (
                      <kbd class="rounded-btn border border-line bg-surface-soft px-2xs py-3xs font-mono text-2xs text-text">
                        {item.keys}
                      </kbd>
                    )}
                  />
                )}
              </For>
            </RowGroup>
          </section>
        )}
      </For>
    </div>
  );
}
