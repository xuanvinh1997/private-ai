import { createSignal, createUniqueId, Show, type JSX } from "solid-js";
import type { ToolCall } from "../../lib/protocol";
import Icon from "../Icon";
import { Disclosure, StateDot, type DotState } from "../primitives";

/** Nhãn tiếng Việt cho tên tool trên wire. Tên lạ thì hiện nguyên tên. */
const TOOL_LABEL: Record<string, string> = {
  read: "Đọc tệp",
  write: "Ghi tệp",
  edit: "Sửa tệp",
  glob: "Tìm tệp",
  grep: "Tìm trong tệp",
  bash: "Chạy lệnh",
  job_output: "Đầu ra tiến trình",
  job_kill: "Dừng tiến trình",
  todo_write: "Danh sách việc",
};

export const toolLabel = (name: string): string => TOOL_LABEL[name] ?? name;

/** Rút gọn một giá trị đối số xuống một dòng đọc lướt được. */
function short(value: unknown, max = 48): string {
  const text =
    typeof value === "string" ? value : value === undefined ? "" : JSON.stringify(value) ?? "";
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

export function summarizeArgs(args: unknown): string {
  if (args === null || typeof args !== "object") return short(args);
  return Object.entries(args as Record<string, unknown>)
    .map(([key, value]) => `${key}=${short(value)}`)
    .join(" · ");
}

export function prettyArgs(args: unknown): string {
  try {
    return JSON.stringify(args, null, 2) ?? "null";
  } catch {
    return String(args);
  }
}

const DEFAULT_STATE: Record<ToolCall["state"], DotState> = {
  running: "running",
  ok: "ok",
  error: "error",
};

const STATE_TEXT: Record<ToolCall["state"], string> = {
  running: "đang chạy",
  ok: "xong",
  error: "lỗi",
};

/**
 * Khung chung của mọi thẻ tool.
 *
 * Mọi thẻ dùng chung khung này để hàng tiêu đề luôn ở cùng một chỗ dù nội dung bên
 * dưới khác hẳn nhau — mắt tìm trạng thái ở một vị trí cố định, không phải đọc lại bố
 * cục cho từng loại tool.
 *
 * Cả hàng tiêu đề là nút gập: một lượt sửa mã dài có hai chục thẻ, và người đọc lại bản
 * ghi cần gập được thứ họ đã xem qua. Viền mảnh và nền chìm hơn tin nhắn một bậc, vì
 * thẻ tool là *việc trợ lý làm*, không phải *điều trợ lý nói*.
 */
export function ToolShell(props: {
  call: ToolCall;
  /** Ghi đè chấm trạng thái — `bash` cần đỏ khi exit code khác 0 dù không phải lỗi tool. */
  state?: DotState;
  summary?: JSX.Element;
  defaultOpen?: boolean;
  children?: JSX.Element;
}) {
  const [open, setOpen] = createSignal(props.defaultOpen ?? true);
  const bodyId = createUniqueId();

  return (
    <section
      class="ml-[calc(var(--avatar)+var(--sp-md))] flex flex-col overflow-hidden rounded-panel border border-line bg-surface-soft"
      aria-label={`${toolLabel(props.call.name)} — ${STATE_TEXT[props.call.state]}`}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={props.children ? open() : undefined}
        aria-controls={props.children ? bodyId : undefined}
        disabled={!props.children}
        class="flex min-w-0 items-center gap-sm px-sm py-2xs text-left transition-colors duration-[var(--dur-fast)] enabled:hover:bg-[var(--overlay-faint)] disabled:cursor-default"
      >
        <StateDot state={props.state ?? DEFAULT_STATE[props.call.state]} />
        <span class="shrink-0 text-xs font-medium text-ink">{toolLabel(props.call.name)}</span>
        {/* Tên trên wire chỉ hiện khi nhãn khác nó — với tool từ MCP thì hai thứ trùng
            nhau, và lặp lại một chuỗi dài như `mcp__jira__...` hai lần chỉ tổ chật hàng. */}
        <Show when={toolLabel(props.call.name) !== props.call.name}>
          <code class="shrink-0 rounded-btn bg-[var(--overlay-faint)] px-3xs font-mono text-2xs text-faint">
            {props.call.name}
          </code>
        </Show>
        <Show when={props.summary}>
          <span class="min-w-0 flex-1 truncate text-xs text-muted">{props.summary}</span>
        </Show>
        <Show when={props.children}>
          <span class="ml-auto shrink-0 text-faint">
            <Icon
              name="chevron-right"
              size={13}
              class={`transition-transform duration-[var(--dur-fast)] ${open() ? "rotate-90" : ""}`}
            />
          </span>
        </Show>
      </button>
      <Show when={props.children}>
        <div id={bodyId} hidden={!open()} class="flex flex-col gap-xs border-t border-line px-sm py-sm">
          {props.children}
        </div>
      </Show>
    </section>
  );
}

/**
 * Thẻ mặc định cho tool chưa có renderer riêng.
 *
 * Không gian khoá của sổ đăng ký là mở — một tool đến từ MCP sẽ không bao giờ có thẻ
 * riêng. Thẻ này phải luôn hiện được *cái gì đó*: tên, đối số thô, kết quả thô.
 */
export default function GenericToolCard(props: { call: ToolCall }) {
  return (
    <ToolShell call={props.call} summary={summarizeArgs(props.call.args)} defaultOpen={false}>
      <Disclosure label="Đối số">
        <pre class="max-h-64 overflow-auto rounded-panel bg-surface px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
          {prettyArgs(props.call.args)}
        </pre>
      </Disclosure>
      <Show when={props.call.preview}>
        {(preview) => (
          <Disclosure label="Kết quả" hint={`${preview().length} ký tự`}>
            <pre class="max-h-64 overflow-auto rounded-panel bg-surface px-sm py-2xs font-mono text-2xs whitespace-pre-wrap text-text">
              {preview()}
            </pre>
          </Disclosure>
        )}
      </Show>
    </ToolShell>
  );
}
