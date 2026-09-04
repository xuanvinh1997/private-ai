import { createEffect, createSignal, on, onMount, Show } from "solid-js";
import {
  embeddingSetting,
  listProviders,
  probeEmbedding,
  providerModels,
  setEmbedding,
  suggestedEmbeddingModel,
} from "../../lib/providers";
import type {
  EmbeddingProbe,
  EmbeddingSetting,
  ModelChoice,
  Provider,
} from "../../lib/protocol";
import { locale, S, t, TRich } from "../../lib/i18n";
import ConfirmDialog from "./ConfirmDialog";
import ModelField, { embeddable, sameModel } from "./ModelField";
import {
  Banner,
  Button,
  InfoDot,
  Row,
  RowGroup,
  SectionHead,
  Select,
} from "../settings/FormKit";

const NONE: EmbeddingSetting = {
  providerId: null,
  providerName: null,
  model: null,
  onDevice: false,
  reason: null,
};

/** The embedding model screen, kept separate because it is a privacy decision: the chat model sees questions, the embedding model sees every document in full. It says where documents go, what the probe really does, and that changing the model re-embeds the library. */
export default function EmbeddingView(props: {
  /** Bump this to make the section re-ask the core, after any provider save, delete or toggle. */
  reloadKey?: number;
}) {
  const [providers, setProviders] = createSignal<Provider[]>([]);
  const [setting, setSetting] = createSignal<EmbeddingSetting>(NONE);
  const [ready, setReady] = createSignal(false);

  // The user's draft, kept apart from the live `setting()`: comparing them decides re-embedding.
  const [providerId, setProviderId] = createSignal("");
  const [model, setModel] = createSignal("");

  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [confirming, setConfirming] = createSignal(false);

  const [probing, setProbing] = createSignal(false);
  const [probe, setProbe] = createSignal<EmbeddingProbe | null>(null);
  const [probeError, setProbeError] = createSignal<string | null>(null);

  // Models from the chosen server; empty means unanswerable, not "none", so the text field stays.
  const [models, setModels] = createSignal<ModelChoice[]>([]);
  const [listing, setListing] = createSignal(false);

  const chosen = () => providers().find((entry) => entry.id === providerId()) ?? null;
  const suggestion = () => {
    const entry = chosen();
    return entry === null ? "" : suggestedEmbeddingModel(entry.kind);
  };

  /** That provider's saved embedding model; nothing is guessed, the server list arrives later. */
  const modelFor = (entry: Provider | null) => entry?.embeddingModel ?? "";

  /** Two requests can be in flight at once, and the later reply is not the later request. */
  let ticket = 0;

  /** Ask the server for its models and fill the field only when it is still empty. */
  const loadModels = async (id: string) => {
    const mine = ++ticket;
    if (id === "") {
      setModels([]);
      setListing(false);
      return;
    }
    // The old list is not cleared first, or the control flips to a text field and back; it locks instead.
    setListing(true);
    const list = await providerModels(id);
    if (mine !== ticket) return;
    setModels(list);
    setListing(false);
    if (model().trim() === "") {
      const first = list.find(embeddable);
      if (first !== undefined) setModel(first.id);
    }
  };

  /** Load providers and the live embedding setting; `keepDraft` protects a half-typed draft. */
  const reload = async (keepDraft: boolean) => {
    const [list, current] = await Promise.all([listProviders(), embeddingSetting()]);
    setProviders(list);
    setSetting(current);

    // With nothing configured, point at the first on-device provider: the safe default here.
    const fallback =
      list.find((entry) => entry.enabled && entry.onDevice) ??
      list.find((entry) => entry.enabled) ??
      null;
    const start = list.find((entry) => entry.id === current.providerId) ?? fallback;

    // Keep the current pick; only jump when the field is still empty, i.e. the first server added.
    if (keepDraft && providerId() !== "") {
      setReady(true);
      return;
    }
    setProviderId(start?.id ?? "");
    setModel(current.model ?? modelFor(start));
    setReady(true);
    // After `setReady`: the model list is a network trip and must not hold the whole screen.
    await loadModels(start?.id ?? "");
  };

  onMount(() => void reload(false));

  /** The server list above changed; without this the picker would keep a stale snapshot. */
  createEffect(
    on(
      () => props.reloadKey ?? 0,
      () => void reload(true),
      { defer: true },
    ),
  );

  const pickProvider = (id: string) => {
    const entry = providers().find((item) => item.id === id) ?? null;
    setProviderId(id);
    setModel(modelFor(entry));
    // The old probe result describes another server; keeping it would bless an untested setup.
    setProbe(null);
    setProbeError(null);
    void loadModels(id);
  };

  /** Model changes from either control land here, since both invalidate the probe result. */
  const pickModel = (value: string) => {
    setModel(value);
    setProbe(null);
    setProbeError(null);
  };

  const complete = () => providerId() !== "" && model().trim() !== "";
  const dirty = () =>
    providerId() !== (setting().providerId ?? "") || !sameModel(model(), setting().model ?? "");

  /** With vectors already stored, a change means re-embedding, so ask first. */
  const needsConfirm = () =>
    setting().providerId !== null && setting().model !== null && dirty();

  const apply = async () => {
    if (!complete()) return;
    setBusy(true);
    setError(null);
    try {
      await setEmbedding(providerId(), model().trim());
      const [list, current] = await Promise.all([listProviders(), embeddingSetting()]);
      setProviders(list);
      setSetting(current);
      setConfirming(false);
    } catch (err) {
      setError(
        t(S.embedding.section.setFailed, {
          detail: err instanceof Error ? err.message : String(err),
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  const runProbe = async () => {
    if (probing() || !complete()) return;
    setProbing(true);
    setProbe(null);
    setProbeError(null);
    try {
      setProbe(await probeEmbedding(providerId(), model().trim()));
    } catch (err) {
      setProbeError(err instanceof Error ? err.message : String(err));
    } finally {
      setProbing(false);
    }
  };

  const options = () => {
    const list = providers()
      // Disabled providers stay listed and labelled, or a setting pointing at one would vanish.
      .map((entry) => {
        let label = entry.name;
        if (!entry.enabled) label = t(S.embedding.provider.optionOff, { name: label });
        if (entry.onDevice) label = t(S.embedding.provider.optionOnDevice, { name: label });
        return { id: entry.id, label };
      });
    return providerId() === ""
      ? [{ id: "", label: t(S.embedding.provider.unset) }, ...list]
      : list;
  };

  return (
    <div class="flex flex-col gap-2xl">
      <SectionHead
        title={t(S.embedding.section.title)}
        icon="graph"
        desc={t(S.embedding.section.desc)}
        more={t(S.embedding.section.more)}
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title={t(S.embedding.section.failed)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={ready()} fallback={<Skeleton />}>
        <Show
          when={providers().length > 0}
          fallback={
            <div class="rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-2xl">
              <p class="m-0 flex max-w-[56ch] items-center gap-2xs text-xs text-muted">
                {t(S.embedding.empty.text)}
                <InfoDot
                  label={t(S.embedding.empty.infoLabel)}
                  text={t(S.embedding.empty.infoText)}
                />
              </p>
            </div>
          }
        >
          <CurrentState setting={setting()} />

          <RowGroup>
            <Row
              label={t(S.common.provider)}
              icon="upload"
              desc={t(S.embedding.provider.desc)}
              more={t(S.embedding.provider.more)}
              control={() => (
                <Select
                  label={t(S.embedding.provider.selectLabel)}
                  value={providerId()}
                  options={options()}
                  disabled={busy()}
                  onPick={pickProvider}
                />
              )}
              below={() => (
                <Show when={chosen()}>{(entry) => <Privacy provider={entry()} />}</Show>
              )}
            />

            <Row
              label={t(S.embedding.model.label)}
              icon="model"
              desc={t(S.embedding.model.desc)}
              more={t(S.embedding.model.more)}
              control={() => (
                <div class="w-[280px] max-w-full">
                  <ModelField
                    role="embedding"
                    label={t(S.embedding.model.fieldLabel)}
                    models={models()}
                    value={model()}
                    disabled={busy() || listing() || providerId() === ""}
                    placeholder={suggestion()}
                    onInput={pickModel}
                  />
                </div>
              )}
              below={() => (
                <Show when={providerId() !== ""}>
                  <ModelSource busy={listing()} models={models()} />
                </Show>
              )}
            />

            <Row
              label={t(S.embedding.probe.label)}
              icon="bolt"
              desc={t(S.embedding.probe.desc)}
              more={t(S.embedding.probe.more)}
              control={() => (
                <Button
                  label={t(probing() ? S.embedding.probe.running : S.embedding.probe.run)}
                  variant="outline"
                  icon="plug"
                  busy={probing()}
                  disabled={!complete() || busy()}
                  onClick={() => void runProbe()}
                />
              )}
              below={() => (
                <ProbeResult busy={probing()} probe={probe()} error={probeError()} />
              )}
            />
          </RowGroup>

          <div class="flex flex-wrap items-center justify-end gap-sm">
            <Show when={dirty() && complete()}>
              <span class="mr-auto text-2xs text-muted">
                {t(
                  needsConfirm() ? S.embedding.apply.willReembed : S.embedding.apply.noReembed,
                )}
              </span>
            </Show>
            <Button
              label={t(S.embedding.apply.save)}
              icon="check"
              busy={busy()}
              disabled={!complete() || !dirty()}
              onClick={() => {
                if (needsConfirm()) setConfirming(true);
                else void apply();
              }}
            />
          </div>
        </Show>
      </Show>

      <Show when={confirming()}>
        <ConfirmDialog
          title={t(S.embedding.confirm.title)}
          body={t(S.embedding.confirm.body)}
          more={t(S.embedding.confirm.more)}
          detail={[
            t(S.embedding.confirm.detailNow, {
              provider: setting().providerName ?? "?",
              model: setting().model ?? "?",
            }),
            t(S.embedding.confirm.detailNext, {
              provider: chosen()?.name ?? "?",
              model: model().trim(),
            }),
          ].join("\n")}
          confirmLabel={t(S.embedding.confirm.confirmLabel)}
          busy={busy()}
          onConfirm={() => void apply()}
          onClose={() => setConfirming(false)}
        />
      </Show>
    </div>
  );
}

/** The live setting at the top of the page; "unset" is deliberately not an error tone, since keyword search still works. */
function CurrentState(props: { setting: EmbeddingSetting }) {
  return (
    <>
      <Show when={props.setting.providerId === null}>
        <div class="flex flex-col gap-2xs rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
          <span class="flex items-center gap-2xs text-xs font-medium text-ink">
            {t(S.embedding.current.unsetTitle)}
            <InfoDot
              label={t(S.embedding.current.unsetInfoLabel)}
              text={t(S.embedding.current.unsetInfoText)}
            />
          </span>
          <p class="m-0 max-w-[62ch] text-2xs text-muted">
            {t(S.embedding.current.unsetBody)}
          </p>
        </div>
      </Show>

      <Show when={props.setting.providerId !== null && props.setting.reason}>
        {(reason) => (
          <Banner
            tone="warn"
            icon="warn"
            title={t(S.embedding.current.brokenTitle)}
            more={t(S.embedding.current.brokenMore)}
          >
            {reason()}
          </Banner>
        )}
      </Show>

      <Show when={props.setting.providerId !== null && props.setting.reason === null}>
        <Banner
          tone="accent"
          icon="check"
          title={t(S.embedding.current.okTitle)}
          more={
            props.setting.onDevice
              ? t(S.embedding.current.okMoreOnDevice)
              : t(S.embedding.current.okMoreRemote, {
                  provider: props.setting.providerName ?? "",
                })
          }
        >
          <code class="font-mono">{props.setting.model}</code>{" "}
          {t(
            props.setting.onDevice
              ? S.embedding.current.okBodyOnDevice
              : S.embedding.current.okBodyRemote,
            { provider: props.setting.providerName ?? "" },
          )}
        </Banner>
      </Show>
    </>
  );
}

/** The privacy sentence for the chosen provider, read from the core's `onDevice` flag and never guessed from the base URL, because this badge is a promise. */
function Privacy(props: { provider: Provider }) {
  return (
    <Show
      when={props.provider.onDevice}
      fallback={
        <Banner
          tone="warn"
          icon="cloud"
          title={t(S.embedding.privacy.remoteTitle)}
          more={t(S.embedding.privacy.remoteMore, {
            name: props.provider.name,
            url: props.provider.baseUrl,
          })}
        >
          <TRich msg={S.embedding.privacy.remoteBody} params={{ url: props.provider.baseUrl }} />
        </Banner>
      }
    >
      <Banner
        tone="accent"
        icon="plug"
        title={t(S.embedding.privacy.localTitle)}
        more={t(S.embedding.privacy.localMore)}
      >
        <TRich msg={S.embedding.privacy.localBody} />
      </Banner>
    </Show>
  );
}

/** Where the model list came from, because an empty picker and an unreachable server look alike; not an error tone, since typing a name still works. */
function ModelSource(props: { busy: boolean; models: ModelChoice[] }) {
  const embed = () => props.models.filter(embeddable).length;
  return (
    <div role="status" aria-live="polite" class="flex flex-col gap-2xs">
      <p class="m-0 max-w-[62ch] text-2xs text-muted">
        <Show when={!props.busy} fallback={t(S.embedding.source.loading)}>
          <Show
            when={props.models.length > 0}
            fallback={t(S.embedding.source.unavailable)}
          >
            <Show
              when={embed() > 0}
              fallback={t(S.embedding.source.noneEmbeddable, { n: props.models.length })}
            >
              {t(S.embedding.source.someEmbeddable, {
                n: props.models.length,
                k: embed(),
              })}
            </Show>
          </Show>
        </Show>
      </p>

    </div>
  );
}

/** The probe result; the dimension count is stated, as it is the only proof a vector came back. */
function ProbeResult(props: { busy: boolean; probe: EmbeddingProbe | null; error: string | null }) {
  return (
    <div role="status" aria-live="polite" aria-busy={props.busy} class="flex flex-col gap-2xs">
      <Show when={props.busy}>
        <Banner tone="info" icon="refresh">
          {t(S.embedding.probe.busy)}
        </Banner>
      </Show>

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" title={t(S.embedding.probe.failedTitle)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={!props.busy && props.probe}>
        {(result) => (
          <Banner
            tone={result().ok ? "accent" : "danger"}
            icon={result().ok ? "check" : "warn"}
            title={t(result().ok ? S.embedding.probe.okTitle : S.embedding.probe.notOkTitle)}
            more={
              result().dimensions === null ? undefined : t(S.embedding.probe.dimsMore)
            }
          >
            <p class="m-0">{result().message}</p>
            <Show when={result().dimensions}>
              {(dims) => (
                <p class="m-0 mt-2xs tabular-nums">
                  {t(S.embedding.probe.dims, {
                    n: Intl.NumberFormat(locale() === "vi" ? "vi-VN" : "en-US").format(dims()),
                  })}
                </p>
              )}
            </Show>
          </Banner>
        )}
      </Show>
    </div>
  );
}

/** Loading skeleton, three rows tall so the page does not jump when the real rows arrive. */
function Skeleton() {
  return (
    <div class="flex flex-col divide-y divide-line rounded-card border border-line bg-surface" aria-hidden="true">
      <div class="flex items-center gap-md px-(--card-pad-x) py-sm">
        <span class="h-3 w-1/4 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
      </div>
      <div class="flex items-center gap-md px-(--card-pad-x) py-sm">
        <span class="h-3 w-1/3 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
      </div>
      <div class="flex items-center gap-md px-(--card-pad-x) py-sm">
        <span class="h-3 w-1/5 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
      </div>
    </div>
  );
}
