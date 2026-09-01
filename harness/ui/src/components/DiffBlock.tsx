import { createMemo, createSignal, For, Show } from "solid-js";
import { diffRows, diffToText, diffTotals, foldRows } from "../lib/diff";
import type { DiffHunk } from "../lib/protocol";
import { CopyButton } from "./primitives";

/** Trong dòng chat, tám dòng là vừa đủ để nhận ra thay đổi mà không nuốt cả màn hình. */
const CHAT_MAX_LINES = 8;

/**
 * Khối diff *xếp chồng* — không side-by-side, không unified có prefix.
 *
 * Dòng thêm/xoá phân biệt bằng **nền màu**, không bằng ký tự `+`/`-` ở đầu dòng. Lý do
 * rất thực dụng: bôi đen một đoạn trên màn hình rồi dán vào editor phải ra đúng mã.
 * Ký tự prefix chỉ xuất hiện trong văn bản nút "Chép" sinh ra (xem `diffToText`).
 *
 * Nền màu một mình không đủ cho người mù màu, nên mỗi dòng còn mang `aria-label` nói rõ
 * "thêm"/"xoá" và cột số dòng cho biết dòng thuộc bản cũ hay bản mới.
 */
export default function DiffBlock(props: { diffs: DiffHunk[]; maxLines?: number }) {
  const [expanded, setExpanded] = createSignal(false);
  const all = createMemo(() => diffRows(props.diffs));
  const limit = () => props.maxLines ?? CHAT_MAX_LINES;
  const shown = createMemo(() => (expanded() ? all() : foldRows(all(), limit())));
  const totals = createMemo(() => diffTotals(props.diffs));
  const foldable = () => all().length > limit();

  return (
    <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
      <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
        <figcaption class="text-2xs text-muted">
          Thay đổi ({totals().files} tệp)
        </figcaption>
        <div class="flex items-center gap-3xs">
          <Show when={foldable()}>
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              aria-expanded={expanded()}
              class="rounded-btn px-2xs py-3xs text-2xs text-muted transition-colors hover:bg-surface-hover hover:text-text"
            >
              {expanded() ? "Gập lại" : `Mở rộng (${all().length} dòng)`}
            </button>
          </Show>
          <CopyButton text={() => diffToText(props.diffs)} label="Chép diff dạng unified" />
        </div>
      </div>

      {/* Cuộn ngang nằm trong khung riêng: dòng mã dài không được kéo giãn cả trang. */}
      <div class="overflow-x-auto">
        <div class="w-max min-w-full font-mono text-2xs leading-[1.55]">
          <For each={shown()}>
            {(row) => (
              <div
                class="flex items-start gap-sm px-sm"
                classList={{
                  "bg-surface-soft text-muted": row.kind === "path",
                  "bg-danger-soft text-text": row.kind === "del",
                  "bg-success-soft text-text": row.kind === "add",
                  "text-faint italic": row.kind === "gap",
                }}
              >
                <span
                  aria-hidden="true"
                  class="w-8 shrink-0 text-right text-faint tabular-nums select-none"
                >
                  {row.oldNo ?? row.newNo ?? ""}
                </span>
                <span
                  class="whitespace-pre"
                  aria-label={
                    row.kind === "add"
                      ? `dòng thêm: ${row.text}`
                      : row.kind === "del"
                        ? `dòng xoá: ${row.text}`
                        : undefined
                  }
                >
                  {row.text === "" ? " " : row.text}
                </span>
              </div>
            )}
          </For>
        </div>
      </div>

      <div class="border-t border-line px-sm py-3xs text-2xs text-faint tabular-nums">
        └ <span class="text-success">+{totals().added}</span>{" "}
        <span class="text-danger">−{totals().removed}</span> · {totals().files} tệp
      </div>
    </figure>
  );
}
