import { Show } from "solid-js";
import { S, t } from "../lib/i18n";
import type { ProjectKind } from "../lib/protocol";
import Icon from "./Icon";
import { InfoDot } from "./settings/FormKit";

/** The empty screen: the big question and what must be read *before* typing; it sits directly above the
 * composer, with nothing between question and input. */
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
