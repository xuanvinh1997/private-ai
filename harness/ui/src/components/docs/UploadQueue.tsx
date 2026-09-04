import { Key } from "@solid-primitives/keyed";
import { createMemo, Show } from "solid-js";
import { fileName, ocrCapable, type UploadFile } from "../../lib/docs";
import { S, t, tn } from "../../lib/i18n";
import Icon from "../Icon";
import { IconButton } from "../primitives";
import { InfoDot } from "../settings/FormKit";
import { Button } from "../projects/DialogShell";

/** Files staged for ingest, each with its own OCR box. Nothing is read until the batch is confirmed, because
 * OCR is a per-file decision -- one scan is worth a page-by-page model call, the report next to it is not --
 * and a switch on the project could only answer for all of them at once. */
export default function UploadQueue(props: {
  files: UploadFile[];
  /** The model that would do the reading; `null` means ticked files get skipped, and the card says so. */
  visionModel: string | null;
  busy?: boolean;
  onOcr: (path: string, ocr: boolean) => void;
  onOcrAll: (ocr: boolean) => void;
  onRemove: (path: string) => void;
  onClear: () => void;
  onConfirm: () => void;
}) {
  const scans = createMemo(() => props.files.filter((file) => ocrCapable(file.path)));
  const allTicked = () => scans().length > 0 && scans().every((file) => file.ocr === true);

  return (
    <div class="flex flex-col gap-sm rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
      <div class="flex flex-wrap items-baseline justify-between gap-sm">
        <p class="m-0 flex items-center gap-2xs text-xs font-medium text-ink">
          {tn(props.files.length, S.docs.upload.headingOne, S.docs.upload.headingOther)}
          <span class="font-normal text-muted">· {t(S.docs.upload.hint)}</span>
          <InfoDot text={t(S.docs.upload.more)} />
        </p>
        <Show when={scans().length > 1}>
          <button
            type="button"
            disabled={props.busy}
            onClick={() => props.onOcrAll(!allTicked())}
            class="rounded-icon px-2xs py-3xs text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink disabled:opacity-50"
          >
            {allTicked() ? t(S.docs.upload.untickAll) : t(S.docs.upload.tickAll)}
          </button>
        </Show>
      </div>

      <ul class="m-0 flex list-none flex-col gap-3xs p-0">
        <Key each={props.files} by={(file) => file.path}>
          {(file) => {
            const name = () => fileName(file().path);
            const scan = () => ocrCapable(file().path);
            return (
              <li class="flex items-center gap-sm rounded-panel bg-surface-soft px-sm py-2xs">
                <span class="shrink-0 text-faint">
                  <Icon name="document" size={13} />
                </span>
                <span class="min-w-0 flex-1 truncate text-xs text-text" title={file().path}>
                  {name()}
                </span>
                <Show
                  when={scan()}
                  fallback={<span class="shrink-0 text-2xs text-faint">{t(S.docs.upload.ocrNone)}</span>}
                >
                  <label class="flex shrink-0 items-center gap-2xs text-2xs text-muted">
                    <input
                      type="checkbox"
                      checked={file().ocr === true}
                      disabled={props.busy}
                      aria-label={t(S.docs.upload.ocrFor, { name: name() })}
                      onChange={(event) => props.onOcr(file().path, event.currentTarget.checked)}
                      class="size-3.5 accent-[var(--accent)]"
                    />
                    {t(S.docs.upload.ocr)}
                  </label>
                </Show>
                <IconButton
                  icon="x"
                  size="sm"
                  disabled={props.busy}
                  label={t(S.docs.upload.remove, { name: name() })}
                  onClick={() => props.onRemove(file().path)}
                />
              </li>
            );
          }}
        </Key>
      </ul>

      <Show when={scans().some((file) => file.ocr === true)}>
        <Show
          when={props.visionModel}
          fallback={
            <p class="m-0 flex items-start gap-2xs rounded-panel bg-warn-soft px-sm py-2xs text-2xs text-text">
              <span class="mt-3xs shrink-0 text-warn">
                <Icon name="warn" size={12} />
              </span>
              {t(S.docs.upload.noModel)}
            </p>
          }
        >
          {(model) => (
            <p class="m-0 text-2xs text-muted">{t(S.docs.upload.model, { model: model() })}</p>
          )}
        </Show>
      </Show>

      <div class="flex justify-end gap-2xs border-t border-line pt-sm">
        <Button variant="outline" disabled={props.busy} onClick={props.onClear}>
          {t(S.docs.upload.clear)}
        </Button>
        <Button variant="primary" icon="plus" disabled={props.busy} onClick={props.onConfirm}>
          {tn(props.files.length, S.docs.upload.confirmOne, S.docs.upload.confirmOther)}
        </Button>
      </div>
    </div>
  );
}
