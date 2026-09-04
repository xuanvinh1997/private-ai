import { Key } from "@solid-primitives/keyed";
import { createSignal, Show } from "solid-js";
import { S, t } from "../../lib/i18n";
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

/** One editable list row; `id` exists only so `<Key>` can keep keyboard focus. */
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

/** Add or edit an MCP server by hand: the two transports share no field but the name, and the paste-JSON path sits on top because every MCP doc hands out one `mcpServers` block. */
export default function McpForm(props: {
  /** `null` adds a server; otherwise the name is locked, since the name is the identity. */
  server: McpServer | null;
  /** Prefilled when arriving from elsewhere, such as fixing an install that just failed. */
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
          ? t(S.mcp.form.jsonFilled, { name: parsed.name })
          : t(S.mcp.form.jsonFilledRest, {
              name: parsed.name,
              n: parsed.rest.length,
              list: parsed.rest.join(", "),
            }),
      );
    } catch (err) {
      setJsonError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <DialogShell
      icon={props.server === null ? "plus" : "pencil"}
      title={
        props.server === null
          ? t(S.mcp.form.addTitle)
          : t(S.mcp.form.editTitle, { name: props.server.name })
      }
      desc={t(S.mcp.form.desc)}
      wide
      onClose={props.onClose}
      onSubmit={() => {
        if (complete() && !props.busy) props.onSubmit(draft());
      }}
      footer={() => (
        <>
          <Button label={t(S.common.cancel)} variant="ghost" onClick={props.onClose} />
          <Button
            label={props.server === null ? t(S.mcp.form.submit) : t(S.common.save)}
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
            {t(S.mcp.form.pasteSummary)}
          </span>
        </summary>
        <div class="mt-2xs flex flex-col gap-2xs">
          <TextArea
            label={t(S.mcp.form.jsonLabel)}
            value={json()}
            onInput={setJson}
            invalid={jsonError() !== null}
            placeholder={'{\n  "mcpServers": {\n    "github": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"] }\n  }\n}'}
            rows={6}
          />
          <div class="flex items-center gap-sm">
            <Button
              label={t(S.mcp.form.jsonFill)}
              variant="outline"
              icon="arrow-down"
              onClick={applyJson}
            />
          </div>
          <div role="status" aria-live="polite">
            <Show when={jsonError()}>
              {(message) => (
                <Banner tone="danger" icon="warn" title={t(S.mcp.form.jsonBadTitle)}>
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
        label={t(S.mcp.form.name)}
        value={name()}
        onInput={setName}
        mono
        placeholder="github"
        disabled={props.server !== null}
        hint={props.server === null ? t(S.mcp.form.nameHint) : t(S.mcp.form.nameLocked)}
        more={props.server === null ? undefined : t(S.mcp.form.nameLockedMore)}
      />

      <PillChoice<"stdio" | "http">
        label={t(S.mcp.form.transport)}
        value={transport()}
        onPick={setTransport}
        options={[
          { id: "stdio", label: t(S.mcp.form.stdio), icon: "terminal" },
          { id: "http", label: "HTTP", icon: "cloud" },
        ]}
        hint={t(S.mcp.form.transportHint)}
      />

      <Show when={transport() === "stdio"}>
        <TextField
          label={t(S.mcp.form.command)}
          value={command()}
          onInput={setCommand}
          mono
          placeholder="npx"
        />

        <RowList
          label={t(S.mcp.form.args)}
          hint={t(S.mcp.form.argsHint)}
          more={t(S.mcp.form.argsMore)}
          rows={args()}
          onRows={setArgs}
          addLabel={t(S.mcp.form.argsAdd)}
          pair={false}
        />

        <RowList
          label={t(S.mcp.form.env)}
          hint={t(S.mcp.form.envHint)}
          rows={env()}
          onRows={setEnv}
          addLabel={t(S.mcp.form.envAdd)}
          pair
          secretValues
        />

        <TextField
          label={t(S.mcp.form.cwd)}
          hint={t(S.common.optional)}
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
          label={t(S.mcp.form.headers)}
          hint={t(S.mcp.form.headersHint)}
          rows={headers()}
          onRows={setHeaders}
          addLabel={t(S.mcp.form.headersAdd)}
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
        {t(S.mcp.form.enable)}
        <span class="text-2xs text-faint">{t(S.mcp.form.enableHint)}</span>
      </label>

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title={t(S.mcp.errors.saveTitle)}>
            {message()}
          </Banner>
        )}
      </Show>
    </DialogShell>
  );
}

/** A grow/shrink list of input rows; `<Key>` by row id, since deleting a row would otherwise drop focus. */
function RowList(props: {
  label: string;
  hint: string;
  /** The long explanation, tucked into an `InfoDot` beside the hint. */
  more?: string;
  rows: Row[];
  onRows: (rows: Row[]) => void;
  addLabel: string;
  /** `true` for key/value pairs, `false` for a single value field such as a CLI argument. */
  pair: boolean;
  /** Mask values while typing: a token in a visible field is a token in the next screenshot. */
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
                    aria-label={t(S.mcp.form.rowKey, { field: props.label })}
                    placeholder={t(S.mcp.form.rowKeyPlaceholder)}
                    onInput={(event) => patch(entry().id, { key: event.currentTarget.value })}
                    class="h-(--control-h) w-40 shrink-0 rounded-btn border border-line-strong bg-bg px-sm font-mono text-2xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
                  />
                </Show>
                <input
                  type={props.secretValues ? "password" : "text"}
                  value={entry().value}
                  spellcheck={false}
                  autocapitalize="off"
                  autocomplete="off"
                  aria-label={t(S.mcp.form.rowValue, { field: props.label })}
                  onInput={(event) => patch(entry().id, { value: event.currentTarget.value })}
                  class="h-(--control-h) min-w-0 flex-1 rounded-btn border border-line-strong bg-bg px-sm font-mono text-2xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
                />
                <IconButton
                  icon="x"
                  label={t(S.mcp.form.rowRemove, { field: props.label })}
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
            {(more) => (
              <InfoDot text={more()} label={t(S.mcp.form.rowAbout, { field: props.label })} />
            )}
          </Show>
        </span>
      </div>
    </fieldset>
  );
}
