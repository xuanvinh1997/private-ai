import { Show } from "solid-js";
import { useDragDrop } from "../../hooks/useDragDrop";
import { S, t } from "../../lib/i18n";
import Icon from "../Icon";
import { InfoDot } from "../settings/FormKit";
import { Button } from "../projects/DialogShell";

export interface DropZoneLabels {
  title: string;
  hint: string;
  more: string;
  pick: string;
}

/** Drop zone over Tauri's drag-drop, not HTML5, because the browser hides the real file path; it fills the screen while the library is empty and shrinks to a row afterwards. */
export default function DropZone(props: {
  compact?: boolean;
  busy?: boolean;
  labels?: DropZoneLabels;
  onPaths: (paths: string[]) => void;
  onPick: () => void;
}) {
  useDragDrop((paths) => {
    if (props.busy !== true) props.onPaths(paths);
  });

  return (
    <Show
      when={props.compact}
      fallback={
        <div class="flex flex-col items-center gap-md rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-4xl text-center">
          <span class="grid size-12 place-items-center rounded-panel bg-accent-soft text-accent-ink">
            <Icon name="upload" size={24} />
          </span>
          <div class="flex flex-col items-center gap-2xs">
            <p class="m-0 flex items-center gap-2xs text-sm font-medium text-ink">
              {props.labels?.title ?? t(S.docs.drop.emptyTitle)}
              <InfoDot text={props.labels?.more ?? t(S.docs.drop.emptyMore)} />
            </p>
            <p class="m-0 max-w-[46ch] text-xs text-muted">
              {props.labels?.hint ?? t(S.docs.drop.emptyHint)}
            </p>
          </div>
          <Button variant="primary" icon="plus" disabled={props.busy} onClick={props.onPick}>
            {props.labels?.pick ?? t(S.docs.drop.pick)}
          </Button>
        </div>
      }
    >
      <div class="flex flex-wrap items-center gap-sm rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
        <span class="text-faint">
          <Icon name="upload" size={15} />
        </span>
        <span class="flex-1 text-xs text-muted">
          {props.labels?.hint ?? t(S.docs.drop.compactHint)}
        </span>
        <Button variant="outline" icon="plus" disabled={props.busy} onClick={props.onPick}>
          {props.labels?.pick ?? t(S.docs.drop.pick)}
        </Button>
      </div>
    </Show>
  );
}
