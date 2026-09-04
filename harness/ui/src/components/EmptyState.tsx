import { For, Show, createResource } from "solid-js";
import { S, t } from "../lib/i18n";
import { displayMode } from "../lib/prefs";
import { NO_SEEDS, goiY, promptSeeds } from "../lib/prompts";
import type { ProjectKind } from "../lib/protocol";
import Icon from "./Icon";
import { InfoDot } from "./settings/FormKit";

/** Top half of the empty screen: the big question and what must be read *before* typing. Split from the chips
 * because the two halves sit on either side of the composer, with nothing between question and input. */
export function EmptyLead(props: {
  /** Kind of the open project, `null` when none; getting it wrong promises the wrong tool set above the composer. */
  kind: ProjectKind | null;
  /** Open the projects screen: the only way out of the "no project" state from here. */
  onOpenProject: () => void;
}) {
  return (
    <div class="mx-auto flex max-w-(--reading-measure) flex-col items-center gap-md px-(--page-pad-x) text-center">
      <Show
        when={props.kind !== null}
        fallback={
          <>
            {/* First line says chat already works, and must come first: a first-run user who reads about a
                missing piece concludes the app is unusable and closes it. */}
            <h2 class="m-0 text-2xl font-medium text-ink">{t(S.chat.empty.readyTitle)}</h2>
            <p class="m-0 max-w-[48ch] text-sm text-muted">{t(S.chat.empty.readyBody)}</p>

            {/* Second line states the limits in the user's words, not tool names nobody knows yet. */}
            <p class="m-0 flex max-w-[52ch] items-start gap-2xs rounded-panel bg-[var(--overlay-faint)] px-md py-sm text-left text-xs text-muted">
              <span class="mt-3xs shrink-0 text-faint">
                <Icon name="warn" size={13} />
              </span>
              <span class="flex flex-wrap items-center gap-2xs">
                {t(S.chat.empty.limitBody)}
                <InfoDot
                  label={t(S.chat.empty.limitInfo)}
                  text={t(S.chat.empty.limitInfoBody)}
                />
              </span>
            </p>

            <button
              type="button"
              onClick={props.onOpenProject}
              class="pai-btn pai-btn-primary"
            >
              <Icon name="folder-open" size={14} />
              {t(S.chat.empty.openProject)}
            </button>
          </>
        }
      >
        {/* A question, not a greeting: a question leaves a gap the input below fills; a greeting closes itself. */}
        <h2 class="m-0 text-2xl font-medium text-ink">{t(S.chat.empty.title)}</h2>
        {/* This line is a promise about permissions, so it must match the open project's tool set exactly. */}
        <p class="m-0 flex max-w-[46ch] flex-wrap items-center justify-center gap-2xs text-sm text-muted">
          <Show
            when={props.kind === "docs"}
            fallback={
              <>
                {t(S.chat.empty.codeBody)}
                <InfoDot label={t(S.chat.empty.codeInfo)} text={t(S.chat.empty.codeInfoBody)} />
              </>
            }
          >
            {t(S.chat.empty.docsBody)}
            <InfoDot label={t(S.chat.empty.docsInfo)} text={t(S.chat.empty.docsInfoBody)} />
          </Show>
        </p>
      </Show>
    </div>
  );
}

/** Bottom half: clickable prompts under the composer, matching its width and left edge so they read as one unit. */
export function PromptChips(props: {
  onPick: (text: string) => void;
  disabled?: boolean;
  kind: ProjectKind | null;
  /** Changes when the project does; `kind` is not a sufficient key, since two code projects share one kind. */
  projectKey: string;
}) {
  const [seeds] = createResource(() => props.projectKey, promptSeeds, {
    initialValue: NO_SEEDS,
  });

  // Show the static set while the core answers, rather than a gap that would shift the layout as the user starts typing.
  const goi_y = () => goiY(props.kind, seeds());

  return (
    <ul
      class="mx-auto my-0 flex w-full list-none flex-wrap gap-2xs px-2xs py-0"
      classList={{
        "max-w-(--reading-measure)": displayMode() === "bubble",
        "max-w-[min(100%,980px)]": displayMode() === "document",
      }}
    >
      <For each={goi_y()}>
        {(text) => (
          <li>
            <button
              type="button"
              disabled={props.disabled}
              onClick={() => props.onPick(text)}
              class="pai-btn pai-btn-secondary text-xs hover:border-accent hover:bg-accent-soft hover:text-accent-ink"
            >
              {text}
            </button>
          </li>
        )}
      </For>
    </ul>
  );
}
