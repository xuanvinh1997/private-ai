import { createEffect, createSignal, on, onMount, Show } from "solid-js";
import {
  listProviders,
  probeVision,
  providerModels,
  setOcr,
  setVision,
  suggestedVisionModel,
  visionSetting,
} from "../../lib/providers";
import type { ModelChoice, Provider, VisionProbe, VisionSetting } from "../../lib/protocol";
import { S, t } from "../../lib/i18n";
import ModelField, { sameModel, seeing } from "./ModelField";
import { Banner, Button, InfoDot, Row, RowGroup, SectionHead, Select, Toggle } from "../settings/FormKit";

const NONE: VisionSetting = {
  providerId: null,
  providerName: null,
  model: null,
  onDevice: false,
  reason: null,
  ocrEnabled: true,
};

/** The vision model screen. Two settings that only make sense together: whether scans are read at all, and
 * who reads them. Unset is a normal state here -- documents with a text layer index either way, images wait --
 * so nothing on this screen is drawn as an error. */
export default function VisionView(props: {
  /** Bump this to make the section re-ask the core, after any provider save, delete or toggle. */
  reloadKey?: number;
}) {
  const [providers, setProviders] = createSignal<Provider[]>([]);
  const [setting, setSetting] = createSignal<VisionSetting>(NONE);
  const [ready, setReady] = createSignal(false);

  // The user's draft, kept apart from the live `setting()`, so a half-typed name is never what OCR uses.
  const [providerId, setProviderId] = createSignal("");
  const [model, setModel] = createSignal("");

  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [ocrBusy, setOcrBusy] = createSignal(false);

  const [probing, setProbing] = createSignal(false);
  const [probe, setProbe] = createSignal<VisionProbe | null>(null);
  const [probeError, setProbeError] = createSignal<string | null>(null);

  // Models from the chosen server; empty means unanswerable, not "none", so the text field stays.
  const [models, setModels] = createSignal<ModelChoice[]>([]);
  const [listing, setListing] = createSignal(false);

  const chosen = () => providers().find((entry) => entry.id === providerId()) ?? null;
  const suggestion = () => {
    const entry = chosen();
    return entry === null ? "" : suggestedVisionModel(entry.kind);
  };

  /** That provider's saved vision model; nothing is guessed, since a guessed name fails per page. */
  const modelFor = (entry: Provider | null) => entry?.visionModel ?? "";

  /** Two requests can be in flight at once, and the later reply is not the later request. */
  let ticket = 0;

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
  };

  /** Load providers and the live setting; `keepDraft` protects a half-typed draft. */
  const reload = async (keepDraft: boolean) => {
    const [list, current] = await Promise.all([listProviders(), visionSetting()]);
    setProviders(list);
    setSetting(current);

    // With nothing configured, point at the first on-device provider: page images are the private case.
    const fallback =
      list.find((entry) => entry.enabled && entry.onDevice) ??
      list.find((entry) => entry.enabled) ??
      null;
    const start = list.find((entry) => entry.id === current.providerId) ?? fallback;

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

  /** The provider list above changed; without this the picker would keep a stale snapshot. */
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

  const pickModel = (value: string) => {
    setModel(value);
    setProbe(null);
    setProbeError(null);
  };

  const complete = () => providerId() !== "" && model().trim() !== "";
  const dirty = () =>
    providerId() !== (setting().providerId ?? "") || !sameModel(model(), setting().model ?? "");

  const apply = async () => {
    if (!complete()) return;
    setBusy(true);
    setError(null);
    try {
      setSetting(await setVision(providerId(), model().trim()));
      setProviders(await listProviders());
    } catch (err) {
      setError(
        t(S.vision.section.setFailed, {
          detail: err instanceof Error ? err.message : String(err),
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  const toggleOcr = async (enabled: boolean) => {
    setOcrBusy(true);
    setError(null);
    try {
      setSetting(await setOcr(enabled));
    } catch (err) {
      setError(
        t(S.vision.ocr.saveFailed, {
          detail: err instanceof Error ? err.message : String(err),
        }),
      );
    } finally {
      setOcrBusy(false);
    }
  };

  const runProbe = async () => {
    if (probing() || !complete()) return;
    setProbing(true);
    setProbe(null);
    setProbeError(null);
    try {
      setProbe(await probeVision(providerId(), model().trim()));
    } catch (err) {
      setProbeError(err instanceof Error ? err.message : String(err));
    } finally {
      setProbing(false);
    }
  };

  const options = () => {
    const list = providers().map((entry) => {
      // Disabled providers stay listed and labelled, or a setting pointing at one would vanish.
      let label = entry.name;
      if (!entry.enabled) label = t(S.vision.provider.optionOff, { name: label });
      if (entry.onDevice) label = t(S.vision.provider.optionOnDevice, { name: label });
      return { id: entry.id, label };
    });
    return providerId() === ""
      ? [{ id: "", label: t(S.vision.provider.unset) }, ...list]
      : list;
  };

  return (
    <div class="flex flex-col gap-2xl">
      <SectionHead
        title={t(S.vision.section.title)}
        icon="eye"
        desc={t(S.vision.section.desc)}
        more={t(S.vision.section.more)}
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title={t(S.vision.section.failed)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={ready()} fallback={<Skeleton />}>
        <CurrentState setting={setting()} />

        <RowGroup>
          <Row
            label={t(S.vision.ocr.label)}
            icon="eye"
            desc={t(S.vision.ocr.desc)}
            more={t(S.vision.ocr.more)}
            control={() => (
              <Toggle
                label={t(S.vision.ocr.toggleLabel)}
                checked={setting().ocrEnabled}
                disabled={busy()}
                busy={ocrBusy()}
                onChange={(enabled) => void toggleOcr(enabled)}
              />
            )}
          />
        </RowGroup>

        <Show
          when={providers().length > 0}
          fallback={
            <div class="rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-2xl">
              <p class="m-0 flex max-w-[56ch] items-center gap-2xs text-xs text-muted">
                {t(S.vision.empty.text)}
                <InfoDot label={t(S.vision.empty.infoLabel)} text={t(S.vision.empty.infoText)} />
              </p>
            </div>
          }
        >
          <RowGroup>
            <Row
              label={t(S.common.provider)}
              icon="upload"
              desc={t(S.vision.provider.desc)}
              more={t(S.vision.provider.more)}
              control={() => (
                <Select
                  label={t(S.vision.provider.selectLabel)}
                  value={providerId()}
                  options={options()}
                  disabled={busy()}
                  onPick={pickProvider}
                />
              )}
              below={() => <Show when={chosen()}>{(entry) => <Privacy provider={entry()} />}</Show>}
            />

            <Row
              label={t(S.vision.model.label)}
              icon="model"
              desc={t(S.vision.model.desc)}
              more={t(S.vision.model.more)}
              control={() => (
                <div class="w-[280px] max-w-full">
                  <ModelField
                    role="vision"
                    label={t(S.vision.model.fieldLabel)}
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
              label={t(S.vision.probe.label)}
              icon="bolt"
              desc={t(S.vision.probe.desc)}
              more={t(S.vision.probe.more)}
              control={() => (
                <Button
                  label={t(probing() ? S.vision.probe.running : S.vision.probe.run)}
                  variant="outline"
                  icon="plug"
                  busy={probing()}
                  disabled={!complete() || busy()}
                  onClick={() => void runProbe()}
                />
              )}
              below={() => <ProbeResult busy={probing()} probe={probe()} error={probeError()} />}
            />
          </RowGroup>

          <div class="flex flex-wrap items-center justify-end gap-sm">
            <Show when={dirty() && complete()}>
              <span class="mr-auto text-2xs text-muted">{t(S.vision.apply.note)}</span>
            </Show>
            <Button
              label={t(S.vision.apply.save)}
              icon="check"
              busy={busy()}
              disabled={!complete() || !dirty()}
              onClick={() => void apply()}
            />
          </div>
        </Show>
      </Show>
    </div>
  );
}

/** The live setting at the top. "Unset" is not an error tone: text documents index either way, and this
 * screen's whole point is that skipping images is a choice rather than a fault. */
function CurrentState(props: { setting: VisionSetting }) {
  return (
    <>
      <Show when={!props.setting.ocrEnabled}>
        <Banner tone="info" icon="warn" title={t(S.vision.ocr.offTitle)}>
          {t(S.vision.ocr.offBody)}
        </Banner>
      </Show>

      <Show when={props.setting.ocrEnabled && props.setting.providerId === null}>
        <div class="flex flex-col gap-2xs rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
          <span class="flex items-center gap-2xs text-xs font-medium text-ink">
            {t(S.vision.current.unsetTitle)}
            <InfoDot
              label={t(S.vision.current.unsetInfoLabel)}
              text={t(S.vision.current.unsetInfoText)}
            />
          </span>
          <p class="m-0 max-w-[62ch] text-2xs text-muted">{t(S.vision.current.unsetBody)}</p>
        </div>
      </Show>

      <Show when={props.setting.ocrEnabled && props.setting.providerId !== null && props.setting.reason}>
        {(reason) => (
          <Banner
            tone="warn"
            icon="warn"
            title={t(S.vision.current.brokenTitle)}
            more={t(S.vision.current.brokenMore)}
          >
            {reason()}
          </Banner>
        )}
      </Show>

      <Show
        when={
          props.setting.ocrEnabled &&
          props.setting.providerId !== null &&
          props.setting.reason === null
        }
      >
        <Banner
          tone="accent"
          icon="check"
          title={t(S.vision.current.okTitle)}
          more={
            props.setting.onDevice
              ? t(S.vision.current.okMoreOnDevice)
              : t(S.vision.current.okMoreRemote, { provider: props.setting.providerName ?? "" })
          }
        >
          <code class="font-mono">{props.setting.model}</code>{" "}
          {t(
            props.setting.onDevice
              ? S.vision.current.okBodyOnDevice
              : S.vision.current.okBodyRemote,
            { provider: props.setting.providerName ?? "" },
          )}
        </Banner>
      </Show>
    </>
  );
}

/** The privacy sentence for the chosen provider, read from the core's `onDevice` flag. OCR uploads whole
 * pages rather than a query, so this says page, not text. */
function Privacy(props: { provider: Provider }) {
  return (
    <Show
      when={props.provider.onDevice}
      fallback={
        <Banner
          tone="warn"
          icon="cloud"
          title={t(S.vision.privacy.remoteTitle)}
          more={t(S.vision.privacy.remoteMore, {
            name: props.provider.name,
            url: props.provider.baseUrl,
          })}
        >
          {t(S.vision.privacy.remoteBody, { url: props.provider.baseUrl })}
        </Banner>
      }
    >
      <Banner
        tone="accent"
        icon="plug"
        title={t(S.vision.privacy.localTitle)}
        more={t(S.vision.privacy.localMore)}
      >
        {t(S.vision.privacy.localBody)}
      </Banner>
    </Show>
  );
}

/** Where the model list came from, and how many of those models the server itself calls image-capable.
 * Not an error tone: a server that declares nothing still runs a vision model, so typing a name stays open. */
function ModelSource(props: { busy: boolean; models: ModelChoice[] }) {
  const sighted = () => props.models.filter(seeing).length;
  return (
    <div role="status" aria-live="polite" class="flex flex-col gap-2xs">
      <p class="m-0 max-w-[62ch] text-2xs text-muted">
        <Show when={!props.busy} fallback={t(S.vision.source.loading)}>
          <Show when={props.models.length > 0} fallback={t(S.vision.source.unavailable)}>
            <Show
              when={sighted() > 0}
              fallback={t(S.vision.source.noneSeeing, { n: props.models.length })}
            >
              {t(S.vision.source.someSeeing, { n: props.models.length, k: sighted() })}
            </Show>
          </Show>
        </Show>
      </p>
    </div>
  );
}

/** The probe result. The model's own answer is shown, because "it replied, but with an apology" is a
 * different problem from "it refused the image", and only the text tells them apart. */
function ProbeResult(props: { busy: boolean; probe: VisionProbe | null; error: string | null }) {
  return (
    <div role="status" aria-live="polite" aria-busy={props.busy} class="flex flex-col gap-2xs">
      <Show when={props.busy}>
        <Banner tone="info" icon="refresh">
          {t(S.vision.probe.busy)}
        </Banner>
      </Show>

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" title={t(S.vision.probe.failedTitle)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={!props.busy && props.probe}>
        {(result) => (
          <Banner
            tone={result().ok ? "accent" : "danger"}
            icon={result().ok ? "check" : "warn"}
            title={t(result().ok ? S.vision.probe.okTitle : S.vision.probe.notOkTitle)}
          >
            <p class="m-0">{result().message}</p>
            <Show when={result().text}>
              {(text) => (
                <p class="m-0 mt-2xs text-2xs text-muted">
                  {t(S.vision.probe.answer)}{" "}
                  <code class="font-mono break-all">{text()}</code>
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
    <div
      class="flex flex-col divide-y divide-line rounded-card border border-line bg-surface"
      aria-hidden="true"
    >
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
