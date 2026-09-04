import { Show } from "solid-js";
import { embedState } from "../../lib/docs";
import { S, t } from "../../lib/i18n";
import type { DocumentView } from "../../lib/protocol";
import Icon, { type IconName } from "../Icon";

/** A document's embed state, told by text, colour and icon at once; queued is neutral, not a warning. */
export default function EmbedBadge(props: { doc: DocumentView }) {
  const state = () => embedState(props.doc);
  const icon = (): IconName =>
    state() === "embedded" ? "check" : state() === "queued" ? "clock" : "warn";
  const label = () =>
    state() === "embedded"
      ? t(S.docs.embed.embedded)
      : state() === "queued"
        ? t(S.docs.embed.queued)
        : t(S.docs.embed.failed);

  return (
    <span class="flex flex-col items-start gap-3xs">
      <span
        class="inline-flex items-center gap-3xs rounded-pill px-2xs py-3xs text-2xs whitespace-nowrap"
        classList={{
          "bg-success-soft text-success": state() === "embedded",
          "bg-[var(--overlay-faint)] text-muted": state() === "queued",
          "bg-danger-soft text-danger": state() === "failed",
        }}
      >
        <span classList={{ "motion-safe:animate-pulse": state() === "queued" }}>
          <Icon name={icon()} size={11} />
        </span>
        {label()}
      </span>
      {/* The failure reason sits below the badge, not behind a hover a keyboard cannot reach. */}
      <Show when={props.doc.error}>
        {(reason) => <span class="max-w-[28ch] text-2xs text-danger">{reason()}</span>}
      </Show>
    </span>
  );
}
