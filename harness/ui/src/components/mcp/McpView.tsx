import { Key } from "@solid-primitives/keyed";
import { createSignal, For, onMount, Show } from "solid-js";
import {
  listMcpServers,
  mcpCatalog,
  reloadMcpServers,
  removeMcpServer,
  saveMcpServer,
  setMcpEnabled,
} from "../../lib/mcp";
import { S, t, tn, type Msg } from "../../lib/i18n";
import type { McpCatalogEntry, McpServer, McpServerInput, McpState } from "../../lib/protocol";
import Icon from "./../Icon";
import { IconButton } from "./../primitives";
import ConfirmDialog from "./../providers/ConfirmDialog";
import { Banner, Button, InfoDot, Row, RowGroup, SectionHead, Toggle } from "../settings/FormKit";
import McpCatalog from "./McpCatalog";
import McpForm from "./McpForm";

type Sheet =
  | { kind: "none" }
  | { kind: "catalog" }
  | { kind: "form"; server: McpServer | null }
  | { kind: "delete"; server: McpServer };

const STATE_LABEL: Record<McpState, Msg> = {
  connected: S.mcp.state.connected,
  connecting: S.mcp.state.connecting,
  failed: S.mcp.state.failed,
  disabled: S.mcp.state.disabled,
};

/** MCP server screen: the untrusted-content notice is core policy and never collapses, a failed server shows its error verbatim in the row, and tool names are listed with the prefix the model actually sees. */
export default function McpView() {
  const [servers, setServers] = createSignal<McpServer[]>([]);
  const [catalog, setCatalog] = createSignal<McpCatalogEntry[]>([]);
  const [ready, setReady] = createSignal(false);
  const [sheet, setSheet] = createSignal<Sheet>({ kind: "none" });
  const [busy, setBusy] = createSignal(false);
  const [reloading, setReloading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [formError, setFormError] = createSignal<string | null>(null);
  const [open, setOpen] = createSignal(new Set<string>());

  onMount(() => {
    void (async () => {
      const [list, entries] = await Promise.all([listMcpServers(), mcpCatalog()]);
      setServers(list);
      setCatalog(entries);
      setReady(true);
    })();
  });

  // `what` is a whole sentence with a `{msg}` slot, not a prefix: clause order differs per language.
  const act = async (what: Msg, run: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await run();
      setServers(await listMcpServers());
    } catch (err) {
      setError(t(what, { msg: err instanceof Error ? err.message : String(err) }));
    } finally {
      setBusy(false);
    }
  };

  const reload = async () => {
    setReloading(true);
    setError(null);
    try {
      setServers(await reloadMcpServers());
    } catch (err) {
      setError(
        t(S.mcp.errors.reload, { msg: err instanceof Error ? err.message : String(err) }),
      );
    } finally {
      setReloading(false);
    }
  };

  const submit = async (input: McpServerInput) => {
    setBusy(true);
    setFormError(null);
    try {
      await saveMcpServer(input);
      setServers(await listMcpServers());
      setSheet({ kind: "none" });
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  // Narrow the union once here, instead of nesting two `<Show>` blocks just for the types.
  const formSheet = () => {
    const current = sheet();
    return current.kind === "form" ? current : null;
  };
  const deleteTarget = () => {
    const current = sheet();
    return current.kind === "delete" ? current.server : null;
  };

  const toggleOpen = (name: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (!next.delete(name)) next.add(name);
      return next;
    });

  return (
    <div class="flex flex-col gap-2xl">
      <SectionHead
        icon="plug"
        title={t(S.mcp.title)}
        desc={t(S.mcp.desc)}
        actions={() => (
          <>
            <Button
              label={reloading() ? t(S.mcp.reloading) : t(S.common.refresh)}
              variant="outline"
              icon="refresh"
              busy={reloading()}
              onClick={() => void reload()}
            />
            <Button
              label={t(S.mcp.add)}
              icon="plus"
              onClick={() => setSheet({ kind: "catalog" })}
            />
          </>
        )}
      />

      {/* Not collapsible and not a footnote: this is core policy. */}
      <Banner
        tone="warn"
        icon="warn"
        title={t(S.mcp.trust.title)}
        more={t(S.mcp.trust.more)}
      >
        {t(S.mcp.trust.body)}
      </Banner>

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title={t(S.mcp.errors.actionTitle)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={ready()} fallback={<Skeleton />}>
        <Show
          when={servers().length > 0}
          fallback={
            <div class="flex flex-col items-start gap-md rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-2xl">
              <p class="m-0 max-w-[52ch] text-xs text-muted">{t(S.mcp.empty)}</p>
              <Button
                label={t(S.mcp.openCatalog)}
                icon="plug"
                onClick={() => setSheet({ kind: "catalog" })}
              />
            </div>
          }
        >
          <RowGroup>
            {/* Keyed by name: the list reloads after every action, and index keying would drop focus. */}
            <Key each={servers()} by="name">
              {(entry) => (
                <ServerRow
                  server={entry()}
                  busy={busy()}
                  open={open().has(entry().name)}
                  onToggleOpen={() => toggleOpen(entry().name)}
                  onToggle={(next) =>
                    void act(S.mcp.errors.toggle, () => setMcpEnabled(entry().name, next))
                  }
                  onEdit={() => {
                    setFormError(null);
                    setSheet({ kind: "form", server: entry() });
                  }}
                  onDelete={() => setSheet({ kind: "delete", server: entry() })}
                />
              )}
            </Key>
          </RowGroup>
        </Show>
      </Show>

      <Show when={sheet().kind === "catalog"}>
        <McpCatalog
          entries={catalog()}
          busy={busy()}
          error={formError()}
          onInstall={(input) => void submit(input)}
          onManual={() => {
            setFormError(null);
            setSheet({ kind: "form", server: null });
          }}
          onClose={() => setSheet({ kind: "none" })}
        />
      </Show>

      <Show when={formSheet()} keyed>
        {(form) => (
          <McpForm
            server={form.server}
            busy={busy()}
            error={formError()}
            onSubmit={(input) => void submit(input)}
            onClose={() => setSheet({ kind: "none" })}
          />
        )}
      </Show>

      <Show when={deleteTarget()} keyed>
        {(target) => (
          <ConfirmDialog
            title={t(S.mcp.remove.title, { name: target.name })}
            body={t(S.mcp.remove.body, { n: target.tools.length })}
            more={t(S.mcp.remove.more, { n: target.tools.length })}
            detail={target.target}
            confirmLabel={t(S.mcp.remove.confirm)}
            busy={busy()}
            onConfirm={() =>
              void act(S.mcp.errors.remove, async () => {
                await removeMcpServer(target.name);
                setSheet({ kind: "none" });
              })
            }
            onClose={() => setSheet({ kind: "none" })}
          />
        )}
      </Show>
    </div>
  );
}

/** A local state dot: `primitives.tsx` has three states and this needs a fourth, `disabled`. */
function StateDot(props: { state: McpState }) {
  return (
    <span
      role="img"
      aria-label={t(STATE_LABEL[props.state])}
      title={t(STATE_LABEL[props.state])}
      class="size-2 shrink-0 rounded-pill"
      classList={{
        "bg-success": props.state === "connected",
        // Connecting pulses: a stalled server and a finished one look alike when the dot is still.
        "bg-muted motion-safe:animate-pulse": props.state === "connecting",
        "bg-danger": props.state === "failed",
        "bg-line-strong": props.state === "disabled",
      }}
    />
  );
}

function ServerRow(props: {
  server: McpServer;
  busy: boolean;
  open: boolean;
  onToggleOpen: () => void;
  onToggle: (next: boolean) => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const count = () => props.server.tools.length;

  return (
    <Row
      label={props.server.name}
      labelMono
      dim={!props.server.enabled}
      lead={() => <StateDot state={props.server.state} />}
      control={() => (
        <>
          <Toggle
            label={t(props.server.enabled ? S.mcp.turnOff : S.mcp.turnOn, {
              name: props.server.name,
            })}
            checked={props.server.enabled}
            busy={props.busy}
            onChange={props.onToggle}
          />
          <IconButton
            icon="pencil"
            label={t(S.mcp.editServer, { name: props.server.name })}
            size="sm"
            onClick={props.onEdit}
          />
          <IconButton
            icon="trash"
            label={t(S.mcp.deleteServer, { name: props.server.name })}
            size="sm"
            danger
            onClick={props.onDelete}
          />
        </>
      )}
      below={() => (
        <>
          <div class="flex min-w-0 flex-wrap items-center gap-2xs">
            <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
              <Icon name={props.server.transport === "http" ? "cloud" : "terminal"} size={10} />
              {props.server.transport === "http" ? "HTTP" : "stdio"}
            </span>
            <span
              class="inline-flex shrink-0 items-center rounded-pill px-2xs py-3xs text-2xs"
              classList={{
                "bg-accent-soft text-accent-ink": props.server.state === "connected",
                "bg-danger-soft text-danger": props.server.state === "failed",
                "bg-[var(--overlay-faint)] text-muted":
                  props.server.state !== "connected" && props.server.state !== "failed",
              }}
            >
              {t(STATE_LABEL[props.server.state])}
            </span>
            <Show when={props.server.state === "connected"}>
              <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs tabular-nums text-muted">
                <Icon name="tools" size={10} />
                {tn(count(), S.mcp.toolsOne, S.mcp.toolsMany)}
              </span>
            </Show>
            <span class="min-w-0 truncate font-mono text-2xs text-faint" title={props.server.target}>
              {props.server.target}
            </span>
          </div>

          {/* The error sits in the row, not behind an expander: a red dot needs its reason beside it. */}
          <Show when={props.server.state === "failed" && props.server.error}>
            {(message) => (
              <p
                role="alert"
                class="m-0 overflow-x-auto rounded-panel border border-danger bg-danger-soft px-sm py-2xs font-mono text-2xs whitespace-pre-wrap text-danger"
              >
                {message()}
              </p>
            )}
          </Show>

          {/* Connected with no tools is a silent state: green dot, no error, and nothing gained. */}
          <Show when={props.server.state === "connected" && count() === 0}>
            <p class="m-0 inline-flex items-center gap-2xs text-2xs text-muted">
              {t(S.mcp.noTools)}
              <InfoDot label={t(S.mcp.noToolsLabel)} text={t(S.mcp.noToolsMore)} />
            </p>
          </Show>

          <Show when={count() > 0}>
            <div class="flex flex-col gap-2xs">
              <button
                type="button"
                onClick={props.onToggleOpen}
                aria-expanded={props.open}
                class="flex items-center gap-2xs self-start rounded-btn px-2xs py-3xs text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
              >
                <Icon
                  name="chevron-right"
                  size={12}
                  class={`transition-transform duration-[var(--dur-fast)] ${props.open ? "rotate-90" : ""}`}
                />
                {props.open
                  ? t(S.mcp.hideTools)
                  : tn(count(), S.mcp.showToolsOne, S.mcp.showToolsMany)}
              </button>

              <Show when={props.open}>
                <div class="overflow-x-auto rounded-panel border border-line bg-surface-soft p-sm">
                  <p class="m-0 mb-2xs text-2xs text-faint">{t(S.mcp.toolNames)}</p>
                  <ul class="m-0 flex list-none flex-col gap-3xs p-0">
                    <For each={props.server.tools}>
                      {(tool) => (
                        <li class="font-mono text-2xs whitespace-nowrap text-text">{tool}</li>
                      )}
                    </For>
                  </ul>
                </div>
              </Show>
            </div>
          </Show>
        </>
      )}
    />
  );
}

function Skeleton() {
  return (
    <div
      class="flex flex-col divide-y divide-line rounded-card border border-line bg-surface"
      aria-hidden="true"
    >
      <For each={[0, 1, 2]}>
        {() => (
          <div class="flex flex-col gap-2xs px-(--card-pad-x) py-sm">
            <span class="h-3 w-1/4 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
            <span class="h-2.5 w-2/3 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
          </div>
        )}
      </For>
    </div>
  );
}
