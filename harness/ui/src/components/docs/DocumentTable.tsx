import { Key } from "@solid-primitives/keyed";
import { Show } from "solid-js";
import { formatBytes, formatLabel, stageLabel, type DocumentTask } from "../../lib/docs";
import { S, t } from "../../lib/i18n";
import type { DocumentView, IngestProgress } from "../../lib/protocol";
import { relativeTime } from "../../lib/sessions";
import { IconButton } from "../primitives";
import EmbedBadge from "./EmbedBadge";

/** Document table: a real `<table>` so screen readers pair cells with headers, scrolling inside its own frame. */
export default function DocumentTable(props: {
  docs: DocumentView[];
  busy?: boolean;
  task?: DocumentTask | null;
  onRemove: (doc: DocumentView) => void;
}) {
  const active = () => inlineProgress(props.task);
  const activeMatches = (path: string) => samePath(path, active()?.path ?? "");
  const activeHasDocument = () => {
    const frame = active();
    return frame !== null && props.docs.some((doc) => samePath(doc.path, frame.path));
  };

  return (
    <div class="overflow-x-auto rounded-card border border-line bg-surface">
      <table class="w-full min-w-[780px] border-collapse text-left">
        <caption class="sr-only">{t(S.docs.table.caption)}</caption>
        <thead>
          <tr class="border-b border-line">
            <Th>{t(S.docs.table.document)}</Th>
            <Th>{t(S.docs.table.format)}</Th>
            <Th>{t(S.docs.table.size)}</Th>
            <Th>{t(S.docs.table.chunks)}</Th>
            <Th>{t(S.docs.table.pages)}</Th>
            <Th>{t(S.docs.table.addedAt)}</Th>
            <Th>{t(S.docs.table.embed)}</Th>
            <th class="w-10 px-sm py-xs">
              <span class="sr-only">{t(S.docs.table.actions)}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          <Show when={active()}>
            {(frame) => (
              <Show when={!activeHasDocument()}>
                <tr class={`border-b border-line ${rowTone(frame().stage)}`}>
                  <td class="max-w-[280px] px-sm py-xs align-top">
                    <span class="flex flex-col gap-3xs">
                      <span class="min-w-0 truncate text-xs font-medium text-ink" title={frame().path}>
                        {fileName(frame().path)}
                      </span>
                      <span class="min-w-0 truncate font-mono text-2xs text-faint" dir="rtl" title={frame().path}>
                        <bdi>{frame().path}</bdi>
                      </span>
                    </span>
                  </td>
                  <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-accent-ink">
                    {stageLabel(frame().stage)}
                  </td>
                  <td class="px-sm py-xs text-2xs text-faint">—</td>
                  <td class="px-sm py-xs text-2xs text-faint">—</td>
                  <td class="px-sm py-xs text-2xs text-faint">—</td>
                  <td class="px-sm py-xs text-2xs text-faint">—</td>
                  <td class="px-sm py-xs text-2xs text-faint">—</td>
                  <td class="px-sm py-xs" />
                </tr>
                <FileProgressRow frame={frame()} />
              </Show>
            )}
          </Show>

          {/* Keyed by id: the array is replaced on every load, and keying by index rebuilds every row. */}
          <Key each={props.docs} by={(doc) => doc.id}>
            {(keyed) => (
              <>
                <tr class={`border-b border-line transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-faint)] ${activeMatches(keyed().path) ? rowTone(active()?.stage ?? "reading") : ""}`}>
                  <td class="max-w-[280px] px-sm py-xs align-top">
                    <span class="flex flex-col gap-3xs">
                      <span class="min-w-0 truncate text-xs text-ink" title={keyed().title}>
                        {keyed().title}
                      </span>
                      <span
                        class="min-w-0 truncate font-mono text-2xs text-faint"
                        dir="rtl"
                        title={keyed().path}
                      >
                        <bdi>{keyed().path}</bdi>
                      </span>
                    </span>
                  </td>
                  <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted">
                    {formatLabel(keyed().format)}
                  </td>
                  <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted tabular-nums">
                    {formatBytes(keyed().bytes)}
                  </td>
                  <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted tabular-nums">
                    {keyed().chunks}
                  </td>
                  <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted tabular-nums">
                    {keyed().ocrPages.length > 0
                      ? t(S.docs.table.ocrPages, { ocr: keyed().ocrPages.length, pages: keyed().pages })
                      : keyed().pages || "—"}
                  </td>
                  <td class="px-sm py-xs align-top text-2xs whitespace-nowrap text-muted">
                    {relativeTime(keyed().addedAt)}
                  </td>
                  <td class="px-sm py-xs align-top">
                    <EmbedBadge doc={keyed()} />
                  </td>
                  <td class="px-sm py-xs align-top">
                    <IconButton
                      icon="trash"
                      size="sm"
                      danger
                      disabled={props.busy}
                      tip="left"
                      label={t(S.docs.table.remove, { title: keyed().title })}
                      onClick={() => props.onRemove(keyed())}
                    />
                  </td>
                </tr>
                <Show when={activeMatches(keyed().path) && active()}>
                  {(frame) => <FileProgressRow frame={frame()} />}
                </Show>
              </>
            )}
          </Key>
        </tbody>
      </table>
    </div>
  );
}

function FileProgressRow(props: { frame: IngestProgress }) {
  const percentage = () =>
    Math.min(100, Math.round((props.frame.done / props.frame.total) * 100));
  const count = () =>
    props.frame.stage === "ocr"
      ? t(S.docs.ingest.pages, { done: props.frame.done, total: props.frame.total })
      : t(S.docs.ingest.files, { done: props.frame.done, total: props.frame.total });

  return (
    <tr class={`border-b border-line ${rowTone(props.frame.stage)}`}>
      <td colspan="8" class="px-sm pt-0 pb-sm">
        <div class="flex flex-col gap-2xs">
          <div class={`flex items-center justify-between gap-sm text-2xs tabular-nums ${textTone(props.frame.stage)}`}>
            <span>{stageLabel(props.frame.stage)}</span>
            <span>{count()} · {percentage()}%</span>
          </div>
          <div
            role="progressbar"
            aria-valuenow={props.frame.done}
            aria-valuemin={0}
            aria-valuemax={props.frame.total}
            aria-valuetext={`${stageLabel(props.frame.stage)}, ${count()}`}
            aria-label={`${t(S.docs.ingest.progressLabel)}: ${fileName(props.frame.path)}`}
            class="h-1.5 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
          >
            <div
              class={`h-full w-full origin-left rounded-pill transition-transform duration-[var(--dur-base)] motion-reduce:transition-none ${barTone(props.frame.stage)}`}
              style={{ transform: `scaleX(${percentage() / 100})` }}
            />
          </div>
        </div>
      </td>
    </tr>
  );
}

function inlineProgress(task?: DocumentTask | null): IngestProgress | null {
  if (task?.state !== "running" || task.progress.total === 0) return null;
  return ["reading", "ocr", "stored", "failed", "skipped"].includes(task.progress.stage)
    ? task.progress
    : null;
}

function samePath(left: string, right: string): boolean {
  return (
    left.replace(/\\/g, "/").replace(/\/+$/, "") ===
    right.replace(/\\/g, "/").replace(/\/+$/, "")
  );
}

function fileName(path: string): string {
  return path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || path;
}

function rowTone(stage: string): string {
  if (stage === "failed") return "bg-danger-soft";
  if (stage === "stored") return "bg-success-soft";
  return "bg-accent-soft";
}

function textTone(stage: string): string {
  if (stage === "failed") return "text-danger";
  if (stage === "stored") return "text-success";
  return "text-accent-ink";
}

function barTone(stage: string): string {
  if (stage === "failed") return "bg-danger";
  if (stage === "stored") return "bg-success";
  return "bg-accent";
}

function Th(props: { children: string }) {
  return (
    <th scope="col" class="px-sm py-xs text-2xs font-medium whitespace-nowrap text-faint">
      {props.children}
    </th>
  );
}
