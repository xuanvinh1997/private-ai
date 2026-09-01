import { For, Show } from "solid-js";
import type { ToolCall } from "../../lib/protocol";
import { Disclosure, FilePath } from "../primitives";
import { ToolShell } from "./ToolCard";

/**
 * Thẻ `read`: đường dẫn + số dòng đã đọc trên tổng số.
 *
 * Tỉ lệ "đã đọc / tổng" quan trọng hơn nội dung: nó cho biết mô hình đang nhìn cả tệp
 * hay chỉ một cửa sổ — và một cửa sổ hẹp là lý do phổ biến của câu trả lời sai.
 */
export default function ReadCard(props: { call: ToolCall }) {
  const read = () => props.call.meta?.read;
  const path = () =>
    read()?.path ??
    (typeof (props.call.args as Record<string, unknown> | null)?.file_path === "string"
      ? String((props.call.args as Record<string, unknown>).file_path)
      : "");

  return (
    <ToolShell
      call={props.call}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          {/* Mở ở đúng cửa sổ mô hình đã nhìn: đó là chỗ câu trả lời của nó đến từ. */}
          <FilePath path={path()} line={read()?.offset || undefined} />
          <Show when={read()}>
            {(meta) => (
              <span class="shrink-0 tabular-nums text-faint">
                {meta().lines.length}/{meta().total_lines} dòng
                {meta().offset > 0 ? ` · từ dòng ${meta().offset}` : ""}
              </span>
            )}
          </Show>
        </span>
      }
    >
      <Show when={read()}>
        {(meta) => (
          <Disclosure label="Nội dung" hint={`${meta().lines.length} dòng`}>
            <div class="max-h-72 overflow-auto rounded-panel bg-surface">
              <div class="w-max min-w-full font-mono text-2xs leading-[1.55]">
                <For each={meta().lines}>
                  {(line) => (
                    <div class="flex items-start gap-sm px-sm">
                      <span
                        aria-hidden="true"
                        class="w-10 shrink-0 text-right text-faint tabular-nums select-none"
                      >
                        {line.number}
                      </span>
                      <span class="whitespace-pre text-text">{line.text === "" ? " " : line.text}</span>
                    </div>
                  )}
                </For>
              </div>
            </div>
          </Disclosure>
        )}
      </Show>
    </ToolShell>
  );
}
