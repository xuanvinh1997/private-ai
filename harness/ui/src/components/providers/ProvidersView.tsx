import { Key } from "@solid-primitives/keyed";
import { createResource, createSignal, createUniqueId, For, onMount, Show } from "solid-js";
import { locale, S, t } from "../../lib/i18n";
import {
  activeModels,
  inputOf,
  listProviders,
  presetHint,
  providerPresets,
  removeProvider,
  saveProvider,
  setActiveProvider,
  setProviderModel,
} from "../../lib/providers";
import type { ModelChoice, Provider, ProviderInput, ProviderPreset } from "../../lib/protocol";
import Icon, { type IconName } from "./../Icon";
import { IconButton } from "./../primitives";
import ConfirmDialog from "./ConfirmDialog";
import ChunkView from "./ChunkView";
import EmbeddingView from "./EmbeddingView";
import RerankView from "./RerankView";
import SpeechView from "./SpeechView";
import VisionView from "./VisionView";
import { Banner, Button, InfoDot, Row, RowGroup, SectionHead, Select, Toggle } from "../settings/FormKit";
import ProviderForm from "./ProviderForm";

type Sheet =
  | { kind: "none" }
  | { kind: "form"; provider: Provider | null; preset: ProviderPreset | null }
  | { kind: "delete"; provider: Provider };

type ModelsTab = "chat" | "embedding" | "vision" | "rerank" | "speech";

const MODEL_TABS: readonly { id: ModelsTab; icon: IconName }[] = [
  { id: "chat", icon: "chat" },
  { id: "embedding", icon: "model" },
  { id: "vision", icon: "eye" },
  { id: "rerank", icon: "graph" },
  { id: "speech", icon: "mic" },
];

/** One tab per model role. Panels remain mounted so a tab switch never discards an unfinished form. */
export default function ProvidersView() {
  const [providers, setProviders] = createSignal<Provider[]>([]);
  const [presets, setPresets] = createSignal<ProviderPreset[]>([]);
  const [ready, setReady] = createSignal(false);
  const [sheet, setSheet] = createSignal<Sheet>({ kind: "none" });
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [formError, setFormError] = createSignal<string | null>(null);
  const [tab, setTab] = createSignal<ModelsTab>("chat");
  const uid = createUniqueId();
  let tabList: HTMLDivElement | undefined;

  const tabId = (id: ModelsTab) => `${uid}-${id}-tab`;
  const panelId = (id: ModelsTab) => `${uid}-${id}-panel`;
  const tabLabel = (id: ModelsTab) => t(S.providers.tabs[id]);

  /** The provider holding the chat role; the embedding role does not pass through this screen. */
  const active = () => providers().find((entry) => entry.activeChat) ?? null;

  /** The active provider's models, via `list_models` where the `tools` flag is authoritative, keyed by config content so an unrelated toggle does not refetch. */
  const activeKey = () => {
    const entry = active();
    return entry === null ? null : `${entry.id}|${entry.kind}|${entry.baseUrl}|${entry.enabled}`;
  };
  const [models, { refetch: refetchModels }] = createResource(activeKey, () =>
    active() === null ? Promise.resolve<ModelChoice[]>([]) : activeModels(),
  );

  /** Bumped whenever the server list changes, so the embedding section re-asks the core itself. */
  const [stamp, setStamp] = createSignal(0);

  const refresh = async () => {
    setProviders(await listProviders());
    setStamp((n) => n + 1);
  };

  onMount(() => {
    void (async () => {
      const [list, catalog] = await Promise.all([listProviders(), providerPresets()]);
      setProviders(list);
      setPresets(catalog);
      setReady(true);
    })();
  });

  /** Wrap an action behind a click, so failures reach the screen instead of the console. */
  const act = async (what: string, run: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await run();
      await refresh();
    } catch (err) {
      setError(`${what}: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const submit = async (input: ProviderInput) => {
    setBusy(true);
    setFormError(null);
    try {
      await saveProvider(input);
      await refresh();
      setSheet({ kind: "none" });
      // The saved config may point at a different server, so the old model list is stale.
      void refetchModels();
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  // Narrow the union once here, rather than nesting `<Show>` layers just for the types.
  const formSheet = () => {
    const current = sheet();
    return current.kind === "form" ? current : null;
  };
  const deleteTarget = () => {
    const current = sheet();
    return current.kind === "delete" ? current.provider : null;
  };

  const chosen = (): ModelChoice | null =>
    (models() ?? []).find((entry) => entry.id === active()?.model) ?? null;

  return (
    <div class="flex flex-col gap-2xl">
      <SectionHead
        title={t(S.providers.title)}
        icon="server"
        desc={t(S.providers.desc)}
        more={t(S.providers.more)}
      />

      <div
        ref={tabList}
        role="tablist"
        aria-label={t(S.providers.tabs.label)}
        class="grid grid-cols-4 gap-3xs rounded-card border border-line bg-surface p-3xs shadow-[var(--edge-top)]"
        onKeyDown={(event) => {
          const keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"];
          if (!keys.includes(event.key)) return;
          event.preventDefault();
          const current = MODEL_TABS.findIndex((entry) => entry.id === tab());
          const next =
            event.key === "Home"
              ? 0
              : event.key === "End"
                ? MODEL_TABS.length - 1
                : (current +
                    (event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1) +
                    MODEL_TABS.length) %
                  MODEL_TABS.length;
          const target = MODEL_TABS[next];
          if (target === undefined) return;
          setTab(target.id);
          tabList?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
        }}
      >
        <For each={MODEL_TABS}>
          {(entry) => (
            <button
              type="button"
              id={tabId(entry.id)}
              role="tab"
              aria-selected={tab() === entry.id}
              aria-controls={panelId(entry.id)}
              tabIndex={tab() === entry.id ? 0 : -1}
              onClick={() => setTab(entry.id)}
              class="flex h-(--control-h) min-w-0 items-center justify-center gap-2xs rounded-btn border px-sm text-xs font-medium transition-colors duration-[var(--dur-fast)] focus-visible:ring-2 focus-visible:ring-accent"
              classList={{
                "border-accent bg-accent-soft text-accent-ink": tab() === entry.id,
                "border-transparent text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
                  tab() !== entry.id,
              }}
            >
              <Icon name={entry.icon} size={13} />
              <span class="min-w-0 truncate">{tabLabel(entry.id)}</span>
            </button>
          )}
        </For>
      </div>

      <section
        id={panelId("chat")}
        role="tabpanel"
        aria-labelledby={tabId("chat")}
        hidden={tab() !== "chat"}
        class="flex flex-col gap-2xl"
      >
        <Show when={error()}>
          {(message) => (
            <Banner tone="danger" icon="warn" role="alert" title={t(S.providers.actionFailed)}>
              {message()}
            </Banner>
          )}
        </Show>

        <Show when={ready()} fallback={<Skeleton />}>
          <Show when={providers().length > 0}>
            <ActiveNotice provider={active()} model={chosen()} loading={models.loading} />

            <RowGroup>
              {/* Keyed by id: the list reloads after every action, and index keying would drop focus. */}
              <Key each={providers()} by="id">
                {(entry) => (
                  <ProviderRow
                    provider={entry()}
                    busy={busy()}
                    models={models() ?? []}
                    modelsLoading={models.loading}
                    onActivate={() =>
                      void act(t(S.providers.err.activate), () => setActiveProvider(entry().id))
                    }
                    onToggle={(next) =>
                      void act(t(S.providers.err.toggle), () =>
                        saveProvider({ ...inputOf(entry()), enabled: next }).then(() => undefined),
                      )
                    }
                    onPickModel={(model) =>
                      void act(t(S.providers.err.pickModel), () =>
                        setProviderModel(entry().id, model),
                      )
                    }
                    onRefreshModels={() => void refetchModels()}
                    onEdit={() => {
                      setFormError(null);
                      setSheet({ kind: "form", provider: entry(), preset: null });
                    }}
                    onDelete={() => setSheet({ kind: "delete", provider: entry() })}
                  />
                )}
              </Key>
            </RowGroup>
          </Show>

          <Catalog
            presets={presets()}
            added={providers()}
            empty={providers().length === 0}
            onPick={(preset) => {
              setFormError(null);
              setSheet({ kind: "form", provider: null, preset });
            }}
            onManual={() => {
              setFormError(null);
              setSheet({ kind: "form", provider: null, preset: null });
            }}
          />
        </Show>
      </section>

      <section
        id={panelId("embedding")}
        role="tabpanel"
        aria-labelledby={tabId("embedding")}
        hidden={tab() !== "embedding"}
      >
        <div class="flex flex-col gap-2xl">
          <EmbeddingView reloadKey={stamp()} />
          {/* Under the model on purpose: chunking is the step immediately before embedding, and the two
              settings share one consequence -- changing either re-embeds the library. */}
          <ChunkView />
        </div>
      </section>

      <section
        id={panelId("vision")}
        role="tabpanel"
        aria-labelledby={tabId("vision")}
        hidden={tab() !== "vision"}
      >
        <VisionView reloadKey={stamp()} />
      </section>

      <section
        id={panelId("rerank")}
        role="tabpanel"
        aria-labelledby={tabId("rerank")}
        hidden={tab() !== "rerank"}
      >
        <RerankView />
      </section>

      <section
        id={panelId("speech")}
        role="tabpanel"
        aria-labelledby={tabId("speech")}
        hidden={tab() !== "speech"}
      >
        <SpeechView />
      </section>

      <Show when={formSheet()} keyed>
        {(open) => (
          <ProviderForm
            provider={open.provider}
            preset={open.preset}
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
            title={t(S.providers.del.title, { name: target.name })}
            body={t(S.providers.del.body)}
            more={t(S.providers.del.more)}
            detail={target.baseUrl}
            confirmLabel={t(S.providers.del.confirm)}
            busy={busy()}
            onConfirm={() =>
              void act(t(S.providers.err.remove), async () => {
                await removeProvider(target.id);
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

/** The provider catalogue, on the page rather than behind a dialog; on-device entries are open by default and remote ones sit behind a click, which is a statement, not a sort order. */
function Catalog(props: {
  presets: ProviderPreset[];
  added: Provider[];
  /** No providers yet, so the catalogue is the page's main content rather than its tail. */
  empty: boolean;
  onPick: (preset: ProviderPreset) => void;
  onManual: () => void;
}) {
  const [more, setMore] = createSignal(false);
  const local = () => props.presets.filter((entry) => entry.onDevice);
  const remote = () => props.presets.filter((entry) => !entry.onDevice);

  /** Does a provider already point at that address; a trailing `/` does not count as different. */
  const already = (preset: ProviderPreset) => {
    const bare = (url: string) => url.trim().replace(/\/+$/, "").toLowerCase();
    return props.added.some((entry) => bare(entry.baseUrl) === bare(preset.baseUrl));
  };

  /** One catalogue row: icon, name, button, and no description line; each `hint` lives in an `InfoDot`. */
  const entry = (preset: ProviderPreset) => (
    <Row
      label={preset.name}
      icon={preset.onDevice ? "plug" : "cloud"}
      more={t(preset.needsKey ? S.providers.catalog.rowMoreKey : S.providers.catalog.rowMore, {
        hint: presetHint(preset),
        url: preset.baseUrl,
      })}
      control={() => (
        <>
          {/* "Added" does not disable the button: two Ollama servers on two ports is valid. */}
          <Show when={already(preset)}>
            <span class="rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-faint">
              {t(S.providers.catalog.added)}
            </span>
          </Show>
          <Button
            label={t(S.providers.catalog.connect)}
            variant="outline"
            icon="plus"
            onClick={() => props.onPick(preset)}
          />
        </>
      )}
    />
  );

  return (
    <section class="flex flex-col gap-sm">
      <h3 class="m-0 flex items-center gap-2xs text-xs font-semibold text-ink">
        {props.empty ? t(S.providers.catalog.headingEmpty) : t(S.providers.catalog.heading)}
        <InfoDot
          label={t(S.providers.catalog.aboutLabel)}
          text={t(S.providers.catalog.aboutText)}
        />
      </h3>

      <RowGroup>
        <For each={local()}>{entry}</For>

        {/* Manual entry is a row like any other: a self-hosted server is normal here, not an exception. */}
        <Row
          label={t(S.providers.catalog.otherLabel)}
          icon="sparkle"
          more={t(S.providers.catalog.otherMore)}
          control={() => (
            <>
              <span class="rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-faint">
                {t(S.providers.catalog.otherBadge)}
              </span>
              <Button
                label={t(S.providers.catalog.otherAction)}
                variant="outline"
                icon="plus"
                onClick={props.onManual}
              />
            </>
          )}
        />

        <Show when={more()}>
          <For each={remote()}>{entry}</For>
        </Show>
      </RowGroup>

      <Show when={!more() && remote().length > 0}>
        <div>
          <Button
            label={t(S.providers.catalog.showRemote, { n: remote().length })}
            variant="ghost"
            icon="cloud"
            onClick={() => setMore(true)}
          />
        </div>
      </Show>
    </section>
  );
}

/** The notices above the list; a `tools: false` model answers fluently but can never read or edit anything, which reads as a bad agent rather than a wrong model. */
function ActiveNotice(props: { provider: Provider | null; model: ModelChoice | null; loading: boolean }) {
  return (
    <>
      <Show when={props.provider === null}>
        <Banner
          tone="warn"
          icon="warn"
          title={t(S.providers.notice.noProviderTitle)}
          more={t(S.providers.notice.noProviderMore)}
        >
          {t(S.providers.notice.noProviderBody)}
        </Banner>
      </Show>

      <Show when={props.provider !== null && props.provider?.enabled === false}>
        <Banner tone="warn" icon="warn" title={t(S.providers.notice.disabledTitle)}>
          {t(S.providers.notice.disabledBody)}
        </Banner>
      </Show>

      <Show when={props.provider !== null && props.provider?.model === null && !props.loading}>
        <Banner tone="warn" icon="warn" title={t(S.providers.notice.noModelTitle)}>
          {t(S.providers.notice.noModelBody)}
        </Banner>
      </Show>

      <Show when={props.model !== null && props.model?.tools === false}>
        <Banner
          tone="danger"
          icon="warn"
          title={t(S.providers.notice.noToolsTitle)}
          more={t(S.providers.notice.noToolsMore, {
            model: props.model?.id ?? t(S.providers.notice.noToolsFallback),
          })}
        >
          <code class="font-mono">{props.model?.id}</code> {t(S.providers.notice.noToolsBody)}
        </Banner>
      </Show>
    </>
  );
}

function ProviderRow(props: {
  provider: Provider;
  busy: boolean;
  models: ModelChoice[];
  modelsLoading: boolean;
  onActivate: () => void;
  onToggle: (next: boolean) => void;
  onPickModel: (model: string) => void;
  onRefreshModels: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  /** The API kind's name, or `null` when it only repeats the provider's name. */
  const kindLabel = () => {
    const label =
      props.provider.kind === "ollama"
        ? "Ollama"
        : props.provider.kind === "lmstudio"
          ? "LM Studio"
          : t(S.providers.kind.openai);
    const bare = (value: string) => value.trim().toLowerCase();
    return bare(props.provider.name).includes(bare(label)) ? null : label;
  };

  return (
    <Row
      label={props.provider.name}
      /* The lead icon carries the on-device fact in colour, with the full sentence in `title`; the flag comes straight from the core, never guessed from the URL. */
      lead={() => (
        <span
          class="grid size-7 shrink-0 place-items-center rounded-panel"
          classList={{
            "bg-accent-soft text-accent-ink": props.provider.onDevice,
            "bg-[var(--overlay-faint)] text-muted": !props.provider.onDevice,
          }}
          title={
            props.provider.onDevice ? t(S.providers.row.onDevice) : t(S.providers.row.remote)
          }
        >
          <Icon name={props.provider.onDevice ? "plug" : "cloud"} size={14} />
        </span>
      )}
      dim={!props.provider.enabled}
      control={() => (
        <>
          <Show when={!props.provider.activeChat}>
            <Button
              label={t(S.providers.row.useForChat)}
              variant="outline"
              disabled={props.busy || !props.provider.enabled}
              onClick={props.onActivate}
            />
          </Show>
          <Toggle
            label={t(props.provider.enabled ? S.providers.row.turnOff : S.providers.row.turnOn, {
              name: props.provider.name,
            })}
            checked={props.provider.enabled}
            busy={props.busy}
            onChange={props.onToggle}
          />
          <IconButton
            icon="pencil"
            label={t(S.providers.row.edit, { name: props.provider.name })}
            size="sm"
            onClick={props.onEdit}
          />
          <IconButton
            icon="trash"
            label={t(S.providers.row.remove, { name: props.provider.name })}
            size="sm"
            danger
            onClick={props.onDelete}
          />
        </>
      )}
      below={() => (
        <>
          <div class="flex min-w-0 flex-wrap items-center gap-2xs">
            <Roles provider={props.provider} />

            {/* Only the outbound case gets a worded badge; a label on every row stops being a warning. */}
            <Show when={!props.provider.onDevice}>
              <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-warn-soft px-2xs py-3xs text-2xs font-medium text-warn">
                <Icon name="cloud" size={10} />
                {t(S.providers.row.leaves)}
              </span>
            </Show>

            {/* The API kind shows only when it adds something the provider's name does not. */}
            <Show when={kindLabel() !== null}>
              <span class="inline-flex shrink-0 items-center rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
                {kindLabel()}
              </span>
            </Show>

            {/* Only on rows without the chat role: the active row already has a model picker below. */}
            <Show when={!props.provider.activeChat}>
              <span
                class="inline-flex min-w-0 shrink items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs"
                classList={{
                  "text-muted": props.provider.model !== null,
                  "text-faint": props.provider.model === null,
                }}
                title={props.provider.model ?? undefined}
              >
                <Icon name="model" size={10} />
                <span class="min-w-0 truncate font-mono">
                  {props.provider.model ?? t(S.providers.row.noModel)}
                </span>
              </span>
            </Show>

            {/* The key icon stands alone; the words would repeat down every remote row. */}
            <Show when={props.provider.hasKey}>
              <span
                class="inline-flex shrink-0 items-center rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-muted"
                title={t(S.providers.row.keyTitle)}
                aria-label={t(S.providers.row.keyLabel)}
              >
                <Icon name="key" size={11} />
              </span>
            </Show>

            <span class="min-w-0 truncate font-mono text-2xs text-faint" title={props.provider.baseUrl}>
              {props.provider.baseUrl}
            </span>
          </div>

          <Show when={props.provider.activeChat}>
            <ModelPicker
              models={props.models}
              loading={props.modelsLoading}
              selected={props.provider.model}
              busy={props.busy}
              onPick={props.onPickModel}
              onRefresh={props.onRefreshModels}
            />
          </Show>
        </>
      )}
    />
  );
}

/** Which roles this provider holds; compact badges keep the row readable when one local server holds all three. */
function Roles(props: { provider: Provider }) {
  const none = () =>
    !props.provider.activeChat && !props.provider.activeEmbedding && !props.provider.activeVision;
  return (
    <>
      <Show when={props.provider.activeChat}>
        <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-accent px-2xs py-3xs text-2xs font-medium text-on-accent">
          <Icon name="chat" size={10} />
        </span>
      </Show>

      <Show when={props.provider.activeVision}>
        <span
          class="inline-flex shrink-0 items-center gap-3xs rounded-pill border border-accent px-2xs py-3xs text-2xs font-medium text-accent-ink"
          title={t(S.providers.row.roleVision, {
            model: props.provider.visionModel ?? t(S.providers.row.roleEmbeddingNone),
          })}
        >
          <Icon name="eye" size={10} />
        </span>
      </Show>

      <Show when={props.provider.activeEmbedding}>
        <span
          class="inline-flex shrink-0 items-center gap-3xs rounded-pill border border-accent px-2xs py-3xs text-2xs font-medium text-accent-ink"
          title={t(S.providers.row.roleEmbedding, {
            model: props.provider.embeddingModel ?? t(S.providers.row.roleEmbeddingNone),
          })}
        >
          <Icon name="library" size={10} />
        </span>
      </Show>

      <Show when={none()}>
        <span class="inline-flex shrink-0 items-center rounded-pill border border-dashed border-line-strong px-2xs py-3xs text-2xs text-faint">
          {t(S.providers.row.noRole)}
        </span>
      </Show>
    </>
  );
}

/** The chat model picker for the active provider: a real `<select>`, since a full server lists dozens of models, with the `tools: false` warning on each option and again above for the selected one. */
function ModelPicker(props: {
  models: ModelChoice[];
  loading: boolean;
  selected: string | null;
  busy: boolean;
  onPick: (model: string) => void;
  onRefresh: () => void;
}) {
  const options = () => {
    const list = props.models.map((choice) => {
      const id = choice.id;
      const n =
        choice.contextWindow === null
          ? null
          : Intl.NumberFormat(locale() === "vi" ? "vi-VN" : "en-US").format(choice.contextWindow);
      const label = choice.tools
        ? n === null
          ? id
          : t(S.providers.opt.ctx, { id, n })
        : n === null
          ? t(S.providers.opt.noTools, { id })
          : t(S.providers.opt.noToolsCtx, { id, n });
      return { id, label };
    });
    // With nothing chosen there must be an empty option, or the browser shows the first as chosen.
    return props.selected === null
      ? [{ id: "", label: t(S.providers.opt.none) }, ...list]
      : list;
  };

  return (
    <div class="flex flex-wrap items-center gap-sm border-t border-line pt-sm">
      {/* An icon instead of a "chat model" label, which the row already says; the `<select>` keeps the `aria-label`. */}
      <span class="shrink-0 text-faint" title={t(S.providers.picker.chatModel)}>
        <Icon name="model" size={13} />
      </span>

      <Show
        when={!props.loading}
        fallback={
          <span class="text-2xs text-muted" role="status" aria-busy="true">
            {t(S.providers.picker.loading)}
          </span>
        }
      >
        <Show
          when={props.models.length > 0}
          fallback={
            <span class="flex min-w-0 flex-1 items-center gap-2xs text-xs text-warn">
              {t(S.providers.picker.unreadable)}
              <InfoDot
                label={t(S.providers.picker.unreadableLabel)}
                text={t(S.providers.picker.unreadableText)}
              />
            </span>
          }
        >
          <Select
            label={t(S.providers.picker.chatModel)}
            mono
            value={props.selected ?? ""}
            options={options()}
            disabled={props.busy}
            onPick={props.onPick}
          />
        </Show>
      </Show>

      <IconButton
        icon="refresh"
        label={t(S.providers.picker.reload)}
        size="sm"
        onClick={props.onRefresh}
      />
    </div>
  );
}

/** Loading skeleton, at row height so the list does not jump when it arrives. */
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
            <span class="h-2.5 w-1/2 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
          </div>
        )}
      </For>
    </div>
  );
}
