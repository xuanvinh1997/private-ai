import { Key } from "@solid-primitives/keyed";
import { createSignal, For, Show } from "solid-js";
import { S, t, tn, TRich } from "../../lib/i18n";
import type { McpCatalogEntry, McpServerInput } from "../../lib/protocol";
import Icon from "./../Icon";
import { Banner, Button, DialogShell, ExternalLink } from "../settings/FormKit";

/** Readable names for prerequisites; an unknown `requires` key falls back to itself. */
const requireLabel = (need: string): string => {
  const table = S.mcp.requires as Record<string, (typeof S.mcp.requires)["node"] | undefined>;
  const msg = table[need];
  return msg === undefined ? need : t(msg);
};

/** One-click catalogue of MCP servers; it states prerequisites and names the missing variables before the user clicks, and secret values are masked and never read back. */
export default function McpCatalog(props: {
  entries: McpCatalogEntry[];
  busy: boolean;
  error: string | null;
  onInstall: (input: McpServerInput) => void;
  onManual: () => void;
  onClose: () => void;
}) {
  const [picked, setPicked] = createSignal<McpCatalogEntry | null>(null);
  const [values, setValues] = createSignal<Record<string, string>>({});

  const missing = () => {
    const entry = picked();
    if (entry === null) return [];
    return entry.env
      .filter((variable) => variable.required && (values()[variable.key] ?? "").trim() === "")
      .map((variable) => variable.label);
  };

  const install = () => {
    const entry = picked();
    if (entry === null || missing().length > 0 || props.busy) return;
    const env: Record<string, string> = {};
    for (const variable of entry.env) {
      const value = (values()[variable.key] ?? "").trim();
      if (value !== "") env[variable.key] = value;
    }
    props.onInstall({
      name: entry.id,
      transport: "stdio",
      command: entry.command,
      args: [...entry.args],
      env,
      cwd: null,
      url: "",
      headers: {},
      enabled: true,
    });
  };

  return (
    <DialogShell
      icon="plug"
      title={
        picked() === null
          ? t(S.mcp.catalog.title)
          : t(S.mcp.catalog.installTitle, { name: picked()?.name ?? "" })
      }
      desc={picked() === null ? t(S.mcp.catalog.desc) : t(S.mcp.catalog.descPicked)}
      more={
        picked()?.env.some((variable) => variable.secret) === true
          ? t(S.mcp.catalog.secretMore)
          : undefined
      }
      wide
      onClose={props.onClose}
      onSubmit={install}
      footer={() => (
        <>
          <Show
            when={picked() !== null}
            fallback={
              <Button
                label={t(S.mcp.catalog.manual)}
                variant="outline"
                icon="plus"
                onClick={props.onManual}
              />
            }
          >
            <Button
              label={t(S.mcp.catalog.back)}
              variant="ghost"
              onClick={() => {
                setPicked(null);
                setValues({});
              }}
            />
          </Show>
          <span class="flex-1" />
          <Button label={t(S.common.close)} variant="ghost" onClick={props.onClose} />
          <Show when={picked()}>
            <Button
              label={t(S.mcp.catalog.submit)}
              type="submit"
              busy={props.busy}
              disabled={missing().length > 0}
            />
          </Show>
        </>
      )}
    >
      <Show
        when={picked()}
        fallback={
          <Show
            when={props.entries.length > 0}
            fallback={
              <p class="m-0 text-xs text-faint">{t(S.mcp.catalog.empty)}</p>
            }
          >
            <ul class="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-sm p-0">
              <For each={props.entries}>
                {(entry) => (
                  <li class="min-w-0">
                    <div class="flex h-full flex-col gap-2xs rounded-card border border-line bg-surface p-sm transition-colors duration-[var(--dur-fast)] hover:border-accent">
                      <button
                        type="button"
                        onClick={() => {
                          setPicked(entry);
                          setValues({});
                        }}
                        class="flex min-w-0 flex-1 flex-col items-start gap-2xs text-left"
                      >
                        <span class="flex w-full min-w-0 items-center gap-2xs">
                          <span class="grid size-6 shrink-0 place-items-center rounded-icon bg-accent-soft text-accent-ink">
                            <Icon name="plug" size={13} />
                          </span>
                          <span class="min-w-0 flex-1 truncate text-xs font-semibold text-ink">
                            {entry.name}
                          </span>
                        </span>
                        <span class="text-2xs text-muted">{entry.summary}</span>
                        {/* Remote entries say so, because nothing has to be installed for them. */}
                        <Show when={entry.url !== null}>
                          <span class="inline-flex items-center gap-3xs rounded-pill bg-accent-soft px-2xs py-3xs text-2xs text-accent-ink">
                            <Icon name="cloud" size={10} />
                            {t(S.mcp.catalog.remote)}
                          </span>
                        </Show>
                        <Show when={entry.requires.length > 0}>
                          <span class="flex flex-wrap gap-3xs">
                            <For each={entry.requires}>
                              {(need) => (
                                <span class="inline-flex items-center gap-3xs rounded-pill bg-warn-soft px-2xs py-3xs text-2xs text-warn">
                                  <Icon name="warn" size={10} />
                                  {requireLabel(need)}
                                </span>
                              )}
                            </For>
                          </span>
                        </Show>
                        <Show when={entry.env.some((variable) => variable.required)}>
                          <span class="inline-flex items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
                            <Icon name="key" size={10} />
                            {tn(
                              entry.env.filter((variable) => variable.required).length,
                              S.mcp.catalog.needsOne,
                              S.mcp.catalog.needsMany,
                            )}
                          </span>
                        </Show>
                      </button>
                      <div class="flex items-center justify-between gap-sm border-t border-line pt-2xs">
                        <span class="min-w-0 truncate font-mono text-2xs text-faint">
                          {[entry.command, ...entry.args].join(" ")}
                        </span>
                        <ExternalLink href={entry.homepage}>{t(S.common.docs)}</ExternalLink>
                      </div>
                    </div>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        }
      >
        {(entry) => (
          <>
            <p class="m-0 text-xs text-muted">{entry().summary}</p>

            <Show when={entry().requires.length > 0}>
              <Banner
                tone="warn"
                icon="warn"
                title={t(S.mcp.catalog.requiresTitle)}
                more={t(S.mcp.catalog.requiresMore)}
              >
                <ul class="m-0 list-disc pl-lg">
                  <For each={entry().requires}>{(need) => <li>{requireLabel(need)}</li>}</For>
                </ul>
                {t(S.mcp.catalog.requiresBody)}
              </Banner>
            </Show>

            <p class="m-0 overflow-x-auto rounded-panel border border-line bg-surface-soft px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
              {[entry().command, ...entry().args].join(" ")}
            </p>

            <Show
              when={entry().env.length > 0}
              fallback={
                <p class="m-0 text-2xs text-faint">{t(S.mcp.catalog.noEnv)}</p>
              }
            >
              {/* `<Key>` by variable name: index keying would rebuild inputs and drop focus mid-typing. */}
              <Key each={entry().env} by="key">
                {(variable) => (
                  <div class="flex min-w-0 flex-col gap-2xs">
                    <label class="flex items-center gap-2xs text-2xs text-faint">
                      {variable().label}
                      <span class="font-mono">{variable().key}</span>
                      <Show
                        when={variable().required}
                        fallback={<span class="text-faint">{t(S.mcp.catalog.optional)}</span>}
                      >
                        <span class="text-warn">{t(S.mcp.catalog.required)}</span>
                      </Show>
                      <Show when={variable().secret}>
                        <span class="inline-flex items-center gap-3xs text-muted">
                          <Icon name="key" size={10} />
                          {t(S.mcp.catalog.secretNote)}
                        </span>
                      </Show>
                    </label>
                    <input
                      type={variable().secret ? "password" : "text"}
                      value={values()[variable().key] ?? ""}
                      spellcheck={false}
                      autocapitalize="off"
                      autocomplete="off"
                      aria-label={variable().label}
                      aria-required={variable().required}
                      onInput={(event) => {
                        const next = event.currentTarget.value;
                        setValues((current) => ({ ...current, [variable().key]: next }));
                      }}
                      class="h-(--control-h) w-full rounded-btn border border-line-strong bg-bg px-sm font-mono text-xs text-text transition-colors duration-[var(--dur-fast)] focus:border-accent"
                    />
                  </div>
                )}
              </Key>
            </Show>

            {/* Block the button and name what is missing; a grey button alone explains nothing. */}
            <Show when={missing().length > 0}>
              <Banner tone="info" icon="warn" role="status">
                {/* One whole sentence, not fragments around a tag: word order differs per language. */}
                <TRich msg={S.mcp.catalog.missing} params={{ list: missing().join(", ") }} />
              </Banner>
            </Show>

            <Show when={props.error}>
              {(message) => (
                <Banner tone="danger" icon="warn" role="alert" title={t(S.mcp.errors.installTitle)}>
                  {message()}
                </Banner>
              )}
            </Show>
          </>
        )}
      </Show>
    </DialogShell>
  );
}
