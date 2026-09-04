import { Key } from "@solid-primitives/keyed";
import { formatBytes, formatLabel } from "../../lib/docs";
import { S, t } from "../../lib/i18n";
import type { DocumentView } from "../../lib/protocol";
import { relativeTime } from "../../lib/sessions";
import { IconButton } from "../primitives";
import EmbedBadge from "./EmbedBadge";

/** Document table: a real `<table>` so screen readers pair cells with headers, scrolling inside its own frame. */
export default function DocumentTable(props: {
  docs: DocumentView[];
  busy?: boolean;
  onRemove: (doc: DocumentView) => void;
}) {
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
          {/* Keyed by id: the array is replaced on every load, and keying by index rebuilds every row. */}
          <Key each={props.docs} by={(doc) => doc.id}>
            {(keyed) => (
              <tr class="border-b border-line last:border-0 transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-faint)]">
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
            )}
          </Key>
        </tbody>
      </table>
    </div>
  );
}

function Th(props: { children: string }) {
  return (
    <th scope="col" class="px-sm py-xs text-2xs font-medium whitespace-nowrap text-faint">
      {props.children}
    </th>
  );
}
