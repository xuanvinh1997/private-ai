import { Key } from "@solid-primitives/keyed";
import { createSignal, Show } from "solid-js";
import { parseMcpJson } from "../../lib/mcp";
import type { McpServer, McpServerInput } from "../../lib/protocol";
import Icon from "./../Icon";
import { IconButton } from "./../primitives";
import {
  Banner,
  Button,
  DialogShell,
  InfoDot,
  PillChoice,
  TextArea,
  TextField,
} from "../settings/FormKit";

/** Một dòng danh sách có ô nhập. `id` chỉ tồn tại để `<Key>` giữ được tiêu điểm bàn phím. */
interface Row {
  id: string;
  key: string;
  value: string;
}

let counter = 0;
const row = (key = "", value = ""): Row => ({ id: `r${(counter += 1)}`, key, value });

const rowsFrom = (table: Record<string, string>): Row[] =>
  Object.entries(table).map(([key, value]) => row(key, value));

const tableFrom = (rows: Row[]): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const entry of rows) {
    const key = entry.key.trim();
    if (key !== "") out[key] = entry.value;
  }
  return out;
};

/**
 * Thêm hoặc sửa một server MCP bằng tay.
 *
 * Hai transport là hai bộ ô khác hẳn nhau — `stdio` chạy một tiến trình con, `http` gọi
 * một địa chỉ — nên chúng không dùng chung ô nào ngoài cái tên. Hiện cả hai bộ cùng lúc
 * sẽ bắt người dùng tự đoán nửa nào áp dụng cho mình.
 *
 * Lối **dán JSON** đứng trên cùng chứ không nằm dưới đáy như một tính năng nâng cao: mọi
 * tài liệu MCP ngoài kia đều đưa ra đúng một khối `{"mcpServers": {…}}`, và bắt người
 * dùng gõ lại từng ô của một khối họ đang bôi đen sẵn là bắt họ làm việc của máy.
 */
export default function McpForm(props: {
  /** `null` là thêm mới. Có giá trị thì tên bị khoá — tên là khoá định danh của server. */
  server: McpServer | null;
  /** Điền sẵn khi đi từ một chỗ khác (ví dụ sửa lại một mục vừa cắm hỏng). */
  draft?: McpServerInput | null;
  busy: boolean;
  error: string | null;
  onSubmit: (input: McpServerInput) => void;
  onClose: () => void;
}) {
  const start = props.draft ?? null;

  const [name, setName] = createSignal(start?.name ?? props.server?.name ?? "");
  const [transport, setTransport] = createSignal<"stdio" | "http">(
    start?.transport ?? props.server?.transport ?? "stdio",
  );
  const [command, setCommand] = createSignal(start?.command ?? "");
  const [args, setArgs] = createSignal<Row[]>((start?.args ?? []).map((value) => row("", value)));
  const [env, setEnv] = createSignal<Row[]>(rowsFrom(start?.env ?? {}));
  const [cwd, setCwd] = createSignal(start?.cwd ?? "");
  const [url, setUrl] = createSignal(start?.url ?? "");
  const [headers, setHeaders] = createSignal<Row[]>(rowsFrom(start?.headers ?? {}));
  const [enabled, setEnabled] = createSignal(start?.enabled ?? props.server?.enabled ?? true);

  const [json, setJson] = createSignal("");
  const [jsonError, setJsonError] = createSignal<string | null>(null);
  const [jsonNote, setJsonNote] = createSignal<string | null>(null);

  const complete = () =>
    name().trim() !== "" &&
    (transport() === "stdio" ? command().trim() !== "" : url().trim() !== "");

  const draft = (): McpServerInput => ({
    name: name().trim(),
    transport: transport(),
    command: command().trim(),
    args: args()
      .map((entry) => entry.value.trim())
      .filter((value) => value !== ""),
    env: tableFrom(env()),
    cwd: cwd().trim() === "" ? null : cwd().trim(),
    url: url().trim(),
    headers: tableFrom(headers()),
    enabled: enabled(),
  });

  const applyJson = () => {
    setJsonError(null);
    setJsonNote(null);
    try {
      const parsed = parseMcpJson(json());
      const input = parsed.input;
      if (input.name !== "") setName(input.name);
      setTransport(input.transport);
      setCommand(input.command);
      setArgs(input.args.map((value) => row("", value)));
      setEnv(rowsFrom(input.env));
      setCwd(input.cwd ?? "");
      setUrl(input.url);
      setHeaders(rowsFrom(input.headers));
      setJsonNote(
        parsed.rest.length === 0
          ? `Đã điền từ mục "${parsed.name}" — xem lại rồi bấm Lưu.`
          : `Đã điền "${parsed.name}", bỏ qua ${parsed.rest.length} mục còn lại (${parsed.rest.join(", ")}).`,
      );
    } catch (err) {
      setJsonError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <DialogShell
      icon={props.server === null ? "plus" : "pencil"}
      title={props.server === null ? "Thêm server MCP" : `Sửa ${props.server.name}`}
      desc="Tool hiện dưới tên ext.<tên>.<tool>."
      wide
      onClose={props.onClose}
      onSubmit={() => {
        if (complete() && !props.busy) props.onSubmit(draft());
      }}
      footer={() => (
        <>
          <Button label="Huỷ" variant="ghost" onClick={props.onClose} />
          <Button
            label={props.server === null ? "Cắm server" : "Lưu"}
            type="submit"
            busy={props.busy}
            disabled={!complete()}
          />
        </>
      )}
    >
      <details class="rounded-panel border border-line bg-surface-soft px-sm py-2xs">
        <summary class="cursor-pointer list-none text-2xs text-muted">
          <span class="inline-flex items-center gap-2xs">
            <Icon name="copy" size={12} />
            Dán JSON từ tài liệu của server
          </span>
        </summary>
        <div class="mt-2xs flex flex-col gap-2xs">
          <TextArea
            label="Khối mcpServers"
            value={json()}
            onInput={setJson}
            invalid={jsonError() !== null}
            placeholder={'{\n  "mcpServers": {\n    "github": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"] }\n  }\n}'}
            rows={6}
          />
          <div class="flex items-center gap-sm">
            <Button label="Điền vào các ô" variant="outline" icon="arrow-down" onClick={applyJson} />
          </div>
          <div role="status" aria-live="polite">
            <Show when={jsonError()}>
              {(message) => (
                <Banner tone="danger" icon="warn" title="JSON không dùng được">
                  {message()}
                </Banner>
              )}
            </Show>
            <Show when={jsonNote()}>
              {(note) => (
                <Banner tone="accent" icon="check">
                  {note()}
                </Banner>
              )}
            </Show>
          </div>
        </div>
      </details>

      <TextField
        label="Tên"
        value={name()}
        onInput={setName}
        mono
        placeholder="github"
        disabled={props.server !== null}
        hint={
          props.server === null
            ? "Tiền tố của mọi tool; chữ thường, không dấu cách."
            : "Tên là khoá định danh nên không sửa được."
        }
        more={
          props.server === null
            ? undefined
            : "Tên là khoá định danh của server nên không sửa được — xoá rồi thêm lại nếu cần đổi."
        }
      />

      <PillChoice<"stdio" | "http">
        label="Cách kết nối"
        value={transport()}
        onPick={setTransport}
        options={[
          { id: "stdio", label: "Tiến trình con (stdio)", icon: "terminal" },
          { id: "http", label: "HTTP", icon: "cloud" },
        ]}
        hint="stdio chạy lệnh tại máy; HTTP gọi một địa chỉ."
      />

      <Show when={transport() === "stdio"}>
        <TextField label="Lệnh" value={command()} onInput={setCommand} mono placeholder="npx" />

        <RowList
          label="Tham số"
          hint="Mỗi dòng một tham số, đúng thứ tự."
          more="Đừng gộp cả dòng lệnh vào một ô — dấu cách trong một tham số là dấu cách thật."
          rows={args()}
          onRows={setArgs}
          addLabel="Thêm tham số"
          pair={false}
        />

        <RowList
          label="Biến môi trường"
          hint="Khoá và giá trị — chỗ đặt token của server."
          rows={env()}
          onRows={setEnv}
          addLabel="Thêm biến"
          pair
          secretValues
        />

        <TextField
          label="Thư mục làm việc (tuỳ chọn)"
          value={cwd()}
          onInput={setCwd}
          mono
          placeholder="/Users/ban/Workspaces/du-an"
        />
      </Show>

      <Show when={transport() === "http"}>
        <TextField
          label="URL"
          value={url()}
          onInput={setUrl}
          mono
          placeholder="https://mcp.vi-du.com/v1/sse"
        />
        <RowList
          label="Header"
          hint="Ví dụ Authorization: Bearer …"
          rows={headers()}
          onRows={setHeaders}
          addLabel="Thêm header"
          pair
          secretValues
        />
      </Show>

      <label class="flex items-center gap-sm text-xs text-text">
        <input
          type="checkbox"
          checked={enabled()}
          onChange={(event) => setEnabled(event.currentTarget.checked)}
          class="size-4 accent-[var(--accent)]"
        />
        Bật ngay sau khi lưu
        <span class="text-2xs text-faint">Tắt thì tool không đến tay mô hình.</span>
      </label>

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title="Không lưu được">
            {message()}
          </Banner>
        )}
      </Show>
    </DialogShell>
  );
}

/**
 * Danh sách dòng nhập thêm/bớt được.
 *
 * `<Key>` chứ không `<For>`: xoá dòng thứ hai trong bốn dòng làm cả mảng dịch chỗ, và
 * `<For>` khớp theo vị trí sẽ dựng lại DOM từ dòng đó trở đi — tiêu điểm bàn phím rơi
 * về `body` ngay giữa lúc người dùng đang gõ. `id` của mỗi dòng tồn tại đúng vì việc này.
 */
function RowList(props: {
  label: string;
  hint: string;
  /** Đoạn giải thích dài, cất trong `InfoDot` cạnh câu gợi ý. */
  more?: string;
  rows: Row[];
  onRows: (rows: Row[]) => void;
  addLabel: string;
  /** `true` là cặp khoá/giá trị, `false` là một ô giá trị đơn (tham số dòng lệnh). */
  pair: boolean;
  /** Che giá trị khi gõ: token dán vào một ô hiện chữ là token nằm trên ảnh chụp màn hình. */
  secretValues?: boolean;
}) {
  const patch = (id: string, next: Partial<Row>) =>
    props.onRows(props.rows.map((entry) => (entry.id === id ? { ...entry, ...next } : entry)));

  return (
    <fieldset class="m-0 flex min-w-0 flex-col gap-2xs border-0 p-0">
      <legend class="p-0 text-2xs text-faint">{props.label}</legend>

      <Show when={props.rows.length > 0}>
        <div class="flex flex-col gap-2xs">
          <Key each={props.rows} by="id">
            {(entry) => (
              <div class="flex min-w-0 items-center gap-2xs">
                <Show when={props.pair}>
                  <input
                    type="text"
                    value={entry().key}
                    spellcheck={false}
                    autocapitalize="off"
                    autocomplete="off"
                    aria-label={`${props.label} — khoá`}
                    placeholder="KHOA"
                    onInput={(event) => patch(entry().id, { key: event.currentTarget.value })}
                    class="h-(--control-h) w-40 shrink-0 rounded-btn border border-line bg-bg px-sm font-mono text-2xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
                  />
                </Show>
                <input
                  type={props.secretValues ? "password" : "text"}
                  value={entry().value}
                  spellcheck={false}
                  autocapitalize="off"
                  autocomplete="off"
                  aria-label={`${props.label} — giá trị`}
                  onInput={(event) => patch(entry().id, { value: event.currentTarget.value })}
                  class="h-(--control-h) min-w-0 flex-1 rounded-btn border border-line bg-bg px-sm font-mono text-2xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
                />
                <IconButton
                  icon="x"
                  label={`Bỏ dòng này khỏi ${props.label}`}
                  size="sm"
                  onClick={() => props.onRows(props.rows.filter((other) => other.id !== entry().id))}
                />
              </div>
            )}
          </Key>
        </div>
      </Show>

      <div class="flex items-center gap-sm">
        <Button
          label={props.addLabel}
          variant="ghost"
          icon="plus"
          onClick={() => props.onRows([...props.rows, row()])}
        />
        <span class="inline-flex min-w-0 flex-1 items-center gap-2xs text-2xs text-faint">
          {props.hint}
          <Show when={props.more}>
            {(more) => <InfoDot text={more()} label={`Về ${props.label}`} />}
          </Show>
        </span>
      </div>
    </fieldset>
  );
}
