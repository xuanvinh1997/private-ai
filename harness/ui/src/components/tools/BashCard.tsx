import { createMemo, createSignal, Show } from "solid-js";
import type { ToolCall } from "../../lib/protocol";
import { CopyButton, type DotState } from "../primitives";
import { ToolShell } from "./ToolCard";

/** Giữ chung số học gập với khối diff: nửa trên làm tròn lên. */
const MAX_LINES = 12;

function fold(text: string, expanded: boolean): { body: string; hidden: number } {
  const lines = text.split("\n");
  if (expanded || lines.length <= MAX_LINES) return { body: text, hidden: 0 };
  const head = Math.ceil(MAX_LINES / 2);
  const tail = MAX_LINES - head;
  const hidden = lines.length - head - tail;
  return {
    body: [...lines.slice(0, head), `⋯ ẩn ${hidden} dòng`, ...lines.slice(lines.length - tail)].join(
      "\n",
    ),
    hidden,
  };
}

/**
 * Thẻ `bash`.
 *
 * Một điểm khác mọi thẻ khác: **exit code khác 0 thì chấm đỏ, kể cả khi `is_error` là
 * false**. Lõi coi "lệnh chạy xong và trả 1" là thi hành thành công — đúng về mặt kỹ
 * thuật, nhưng người đọc muốn biết lệnh *hỏng*, và họ đọc chấm màu chứ không đọc số.
 *
 * Tiến trình nền không có exit code cho tới khi nó kết thúc, nên trạng thái "chưa có
 * exit code" phải nói rõ là *đang chạy nền*, không phải là *treo*.
 */
export default function BashCard(props: { call: ToolCall }) {
  const [expanded, setExpanded] = createSignal(false);
  const terminal = () => props.call.meta?.terminal;
  const command = () => {
    const bag = props.call.args as Record<string, unknown> | null;
    const fromArgs = bag && typeof bag.command === "string" ? bag.command : null;
    return terminal()?.command ?? fromArgs ?? "";
  };
  const output = () => terminal()?.output ?? props.call.preview ?? "";
  const folded = createMemo(() => fold(output(), expanded()));

  const state = (): DotState => {
    if (props.call.state === "running") return "running";
    if (props.call.state === "error") return "error";
    const code = terminal()?.exit_code;
    if (code === null || code === undefined) return terminal()?.background ? "running" : "ok";
    return code === 0 ? "ok" : "error";
  };

  return (
    <ToolShell
      call={props.call}
      state={state()}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          <code class="min-w-0 truncate font-mono text-xs text-accent-ink">$ {command()}</code>
          <Show when={terminal()?.background}>
            <span class="shrink-0 rounded-pill bg-warn-soft px-2xs text-2xs text-warn">nền</span>
          </Show>
          <Show when={terminal()?.exit_code !== null && terminal()?.exit_code !== undefined}>
            <span
              class="shrink-0 tabular-nums text-2xs"
              classList={{
                "text-success": terminal()?.exit_code === 0,
                "text-danger": terminal()?.exit_code !== 0,
              }}
            >
              exit {terminal()?.exit_code}
            </span>
          </Show>
          <Show when={terminal()?.signal}>
            {(signal) => <span class="shrink-0 text-2xs text-danger">tín hiệu {signal()}</span>}
          </Show>
        </span>
      }
    >
      <Show when={terminal()?.cwd}>
        {(cwd) => <p class="font-mono text-2xs text-faint">tại {cwd()}</p>}
      </Show>
      <Show when={output() !== ""}>
        <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
          <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
            <figcaption class="text-2xs text-muted">Đầu ra</figcaption>
            <div class="flex items-center gap-3xs">
              <Show when={folded().hidden > 0 || expanded()}>
                <button
                  type="button"
                  onClick={() => setExpanded((v) => !v)}
                  aria-expanded={expanded()}
                  class="rounded-btn px-2xs py-3xs text-2xs text-muted transition-colors hover:bg-surface-hover hover:text-text"
                >
                  {expanded() ? "Gập lại" : "Mở rộng"}
                </button>
              </Show>
              <CopyButton text={output} label="Chép đầu ra lệnh" />
            </div>
          </div>
          <div class="max-h-72 overflow-auto">
            <pre class="w-max min-w-full px-sm py-2xs font-mono text-2xs leading-[1.55] whitespace-pre text-text">
              {folded().body}
            </pre>
          </div>
        </figure>
      </Show>
      <Show when={props.call.state === "running" && terminal()?.background}>
        <p class="text-2xs text-faint">
          Chạy nền{terminal()?.job_id ? ` · mã tiến trình ${terminal()?.job_id}` : ""} — dùng
          <code class="mx-3xs font-mono">job_output</code> để xem thêm.
        </p>
      </Show>
    </ToolShell>
  );
}
