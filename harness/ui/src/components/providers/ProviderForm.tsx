import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { S, t, TRich } from "../../lib/i18n";
import { presetHint, probeProvider, suggestedEmbeddingModel } from "../../lib/providers";
import type {
  ModelChoice,
  Provider,
  ProviderInput,
  ProviderKind,
  ProviderPreset,
  ProviderProbe,
} from "../../lib/protocol";
import ModelField from "./ModelField";
import Icon from "./../Icon";
import {
  Banner,
  Button,
  DialogShell,
  ExternalLink,
  InfoDot,
  PillChoice,
  TextField,
} from "../settings/FormKit";

/** The API key field has exactly three states: `keep` sends `null`, `set` sends the typed string, `clear` sends `""`. */
type KeyMode = "keep" | "set" | "clear";

/** Add/edit form for a provider. The core never returns a stored key, only `hasKey`, so an existing key is shown as a status line with replace and clear buttons rather than an input nobody can trust. */
export default function ProviderForm(props: {
  /** `null` adds; a value edits, and only then does the key field have three states. */
  provider: Provider | null;
  /** The whole catalogue row just connected, since `needsKey` and `onDevice` shape this dialog. */
  preset?: ProviderPreset | null;
  busy: boolean;
  error: string | null;
  onSubmit: (input: ProviderInput) => void;
  onClose: () => void;
}) {
  const start = props.provider ?? props.preset ?? null;

  const [name, setName] = createSignal(start?.name ?? "");
  const [kind, setKind] = createSignal<ProviderKind>(start?.kind ?? "ollama");
  const [baseUrl, setBaseUrl] = createSignal(start?.baseUrl ?? "");
  // Settable here, so a model seen in the probe can be chosen without leaving the dialog.
  const [model, setModel] = createSignal<string | null>(
    props.provider?.model ?? props.preset?.defaultModel ?? null,
  );
  const [enabled, setEnabled] = createSignal(props.provider?.enabled ?? true);
  // From `props.provider`, not `start`: a catalogue preset carries no embedding model.
  const [embeddingModel, setEmbeddingModel] = createSignal(props.provider?.embeddingModel ?? "");

  const hadKey = props.provider?.hasKey === true;
  const [keyMode, setKeyMode] = createSignal<KeyMode>(hadKey ? "keep" : "set");
  const [keyText, setKeyText] = createSignal("");

  /** Is the address already answered; true when connecting a catalogue row or editing a saved provider. */
  const known = props.provider !== null || (props.preset ?? null) !== null;

  /** Is the name/kind/address block open; open only when nothing is known yet. */
  const [open, setOpen] = createSignal(!known);

  /** The key field leaves the collapsed block when a key is needed and missing: the only question left. */
  const needsKeyNow = () => props.preset?.needsKey === true && !hadKey;

  const [probing, setProbing] = createSignal(false);
  const [probe, setProbe] = createSignal<ProviderProbe | null>(null);
  const [probeError, setProbeError] = createSignal<string | null>(null);

  /** `null` keeps the stored key, `""` clears it, any other string sets a new one. */
  const apiKey = (): string | null => {
    if (keyMode() === "clear") return "";
    if (keyMode() === "keep") return null;
    const typed = keyText().trim();
    // Typing then clearing the box means no change; clearing the key needs the clear button.
    return typed === "" ? null : typed;
  };

  const draft = (): ProviderInput => ({
    id: props.provider?.id ?? null,
    name: name().trim(),
    kind: kind(),
    baseUrl: baseUrl().trim(),
    apiKey: apiKey(),
    enabled: enabled(),
    model: model(),
    // An empty string means unset, so send `null`; `""` would be stored as a valid-looking name.
    embeddingModel: embeddingModel().trim() === "" ? null : embeddingModel().trim(),
  });

  const complete = () => name().trim() !== "" && baseUrl().trim() !== "";

  /** A sequence number per probe, since the later reply is not necessarily the later request. */
  let ticket = 0;
  let timer: number | undefined;

  /** `auto` marks a probe the app started: it keeps the old result, while a clicked one clears it first. */
  const runProbe = async (auto: boolean) => {
    const mine = ++ticket;
    setProbing(true);
    if (!auto) {
      setProbe(null);
      setProbeError(null);
    }
    try {
      const result = await probeProvider(draft());
      if (mine !== ticket) return;
      setProbe(result);
      setProbeError(null);
      // No answer means the address or key needs fixing, so open that block; never auto-close it.
      if (!result.ok) setOpen(true);
      // One model means nothing to choose, as with `llama-server`, so pick it automatically.
      const only = result.models.length === 1 ? (result.models[0]?.id ?? null) : null;
      if (model() === null && only !== null) setModel(only);
    } catch (err) {
      if (mine !== ticket) return;
      setProbe(null);
      setProbeError(err instanceof Error ? err.message : String(err));
      setOpen(true);
    } finally {
      if (mine === ticket) setProbing(false);
    }
  };

  /** Probe automatically 700ms after the address settles, and also on key changes, since listing models is a free `GET` and is exactly what the user wants to know. */
  createEffect(() => {
    const url = baseUrl().trim();
    // Read to register the dependency: the API kind decides which endpoint is called.
    void kind();
    void (keyMode() === "set" ? keyText().trim() : keyMode());
    if (url === "") return;
    clearTimeout(timer);
    timer = window.setTimeout(() => void runProbe(true), 700);
  });

  onCleanup(() => clearTimeout(timer));

  return (
    <DialogShell
      icon={props.provider !== null ? "pencil" : ((props.preset?.onDevice ?? false) ? "plug" : "cloud")}
      title={
        props.provider !== null
          ? t(S.providers.form.titleEdit, { name: props.provider.name })
          : props.preset !== null && props.preset !== undefined
            ? t(S.providers.form.titleConnect, { name: props.preset.name })
            : t(S.providers.form.titleManual)
      }
      desc={props.preset == null ? t(S.providers.form.desc) : presetHint(props.preset)}
      onClose={props.onClose}
      onSubmit={() => {
        if (complete() && !props.busy) props.onSubmit(draft());
      }}
      footer={() => (
        <>
          <Button
            label={probing() ? t(S.providers.form.probing) : t(S.common.retry)}
            variant="outline"
            icon="plug"
            busy={probing()}
            disabled={!complete()}
            onClick={() => void runProbe(false)}
          />
          <span class="flex-1" />
          <Button label={t(S.common.cancel)} variant="ghost" onClick={props.onClose} />
          <Button
            label={props.provider === null ? t(S.common.add) : t(S.common.save)}
            type="submit"
            busy={props.busy}
            disabled={!complete()}
          />
        </>
      )}
    >
      {/* Name, kind and address collapse, because connecting a catalogue row already answered them. */}
      <Show
        when={open()}
        fallback={
          <Summary
            name={name()}
            baseUrl={baseUrl()}
            hadKey={hadKey}
            kind={kind()}
            onOpen={() => setOpen(true)}
          />
        }
      >
        <TextField
          label={t(S.providers.form.name)}
          value={name()}
          onInput={setName}
          placeholder={t(S.providers.form.namePlaceholder)}
          ref={(el) => queueMicrotask(() => el.focus())}
        />

        <PillChoice<ProviderKind>
          label={t(S.providers.form.kindLabel)}
          value={kind()}
          onPick={setKind}
          options={[
            { id: "ollama", label: "Ollama", icon: "plug" },
            { id: "lmstudio", label: "LM Studio", icon: "plug" },
            { id: "openai", label: t(S.providers.kind.openai), icon: "cloud" },
          ]}
          hint={t(S.providers.form.kindHint)}
          more={t(S.providers.form.kindMore)}
        />

        <TextField
          label={t(S.providers.form.baseUrl)}
          value={baseUrl()}
          onInput={setBaseUrl}
          mono
          hint={t(S.providers.form.baseUrlHint)}
          more={t(S.providers.form.baseUrlMore)}
          placeholder={
            kind() === "ollama"
              ? "http://127.0.0.1:11434"
              : kind() === "lmstudio"
                ? "http://127.0.0.1:1234"
                : "https://api.openai.com/v1"
          }
        />

        <KeySection
          hadKey={hadKey}
          kind={kind()}
          mode={keyMode()}
          text={keyText()}
          onMode={setKeyMode}
          onText={setKeyText}
        />
      </Show>

      {/* With the block collapsed and a key still needed, the key field stands alone and takes focus. */}
      <Show when={!open() && needsKeyNow()}>
        <KeySection
          hadKey={hadKey}
          kind={kind()}
          mode={keyMode()}
          text={keyText()}
          onMode={setKeyMode}
          onText={setKeyText}
          focus
        />
      </Show>

      {/* The homepage link appears once, beside the key field, where the question is where to get one. */}
      <Show when={needsKeyNow() && props.preset}>
        {(preset) => (
          <p class="m-0 text-2xs text-faint">
            {t(S.providers.form.keyFrom)}{" "}
            <ExternalLink href={preset().homepage}>{preset().name}</ExternalLink>
          </p>
        )}
      </Show>

      {/* Connection state and models sit right under the fields that decide them: address and key. */}
      <ProbeResult busy={probing()} probe={probe()} error={probeError()} />

      <ModelSection
        value={model()}
        onPick={setModel}
        models={probe()?.models ?? []}
        busy={probing()}
        touched={probe() !== null || probeError() !== null}
      />

      {/* This records a model name, it does not assign the embedding role; that lives on its own screen. */}
      <ModelField
        role="embedding"
        label={t(S.providers.form.embedModel)}
        showLabel
        models={probe()?.models ?? []}
        value={embeddingModel()}
        onInput={setEmbeddingModel}
        placeholder={suggestedEmbeddingModel(kind())}
        hint={t(S.providers.form.embedHint)}
        more={t(S.providers.form.embedMore)}
      />

      {/* Only when editing: nobody adds a provider to leave it off, so the question has one answer. */}
      <Show when={props.provider !== null}>
        <label class="flex items-center gap-sm text-xs text-text">
          <input
            type="checkbox"
            checked={enabled()}
            onChange={(event) => setEnabled(event.currentTarget.checked)}
            class="size-4 accent-[var(--accent)]"
          />
          {t(S.providers.form.enable)}
          <span class="text-2xs text-faint">{t(S.providers.form.enableHint)}</span>
        </label>
      </Show>

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert">
            {message()}
          </Banner>
        )}
      </Show>
    </DialogShell>
  );
}

/** The collapsed connection block: name, address and whether a key is set, with the URL wrapped rather than truncated, since a wrong base URL is usually wrong at the end. */
function Summary(props: {
  name: string;
  baseUrl: string;
  hadKey: boolean;
  kind: ProviderKind;
  onOpen: () => void;
}) {
  const local = () => props.kind === "ollama" || props.kind === "lmstudio";
  return (
    <div class="flex flex-wrap items-center gap-sm rounded-panel border border-line bg-surface-soft px-sm py-2xs">
      <span
        class="grid size-7 shrink-0 place-items-center rounded-panel"
        classList={{
          "bg-accent-soft text-accent-ink": local(),
          "bg-[var(--overlay-faint)] text-muted": !local(),
        }}
      >
        <Icon name={local() ? "plug" : "cloud"} size={14} />
      </span>

      <span class="flex min-w-0 flex-1 flex-col gap-3xs">
        <span class="truncate text-xs font-medium text-ink">{props.name}</span>
        <span class="font-mono text-2xs break-all text-faint">{props.baseUrl}</span>
      </span>

      <Show when={props.hadKey}>
        <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-accent-soft px-2xs py-3xs text-2xs text-accent-ink">
          <Icon name="key" size={10} />
          {t(S.providers.form.keySet)}
        </span>
      </Show>

      <Button label={t(S.common.edit)} variant="ghost" icon="pencil" onClick={props.onOpen} />
    </div>
  );
}

/** The API key field: three states, each saying plainly what Save will do. */
function KeySection(props: {
  hadKey: boolean;
  kind: ProviderKind;
  mode: KeyMode;
  text: string;
  onMode: (mode: KeyMode) => void;
  onText: (text: string) => void;
  /** Focus the key field, for when the key is the dialog's only question. */
  focus?: boolean;
}) {
  // Neither on-device kind requires a key by default.
  const optional = () => props.kind === "ollama" || props.kind === "lmstudio";

  return (
    <div class="flex flex-col gap-2xs rounded-panel border border-line bg-surface-soft px-sm py-2xs">
      <div class="flex items-center gap-2xs text-2xs text-faint">
        <Icon name="key" size={12} />
        {t(S.providers.key.title)}
        <Show when={optional()}>
          <span class="text-faint">{t(S.providers.key.optional)}</span>
        </Show>
      </div>

      <Show when={props.hadKey && props.mode === "keep"}>
        <div class="flex flex-wrap items-center gap-sm">
          <span class="inline-flex items-center gap-2xs rounded-pill bg-accent-soft px-2xs py-3xs text-2xs text-accent-ink">
            <Icon name="check" size={11} />
            {t(S.providers.key.isSet)}
          </span>
          <span class="flex min-w-0 flex-1 items-center gap-2xs text-2xs text-muted">
            <TRich msg={S.providers.key.keepNote} />
            <InfoDot
              label={t(S.providers.key.whereLabel)}
              text={t(S.providers.key.whereText)}
            />
          </span>
          <Button
            label={t(S.providers.key.replace)}
            variant="outline"
            onClick={() => props.onMode("set")}
          />
          <Button
            label={t(S.providers.key.clear)}
            variant="ghost"
            icon="trash"
            onClick={() => props.onMode("clear")}
          />
        </div>
      </Show>

      <Show when={props.mode === "clear"}>
        <div class="flex flex-wrap items-center gap-sm">
          <div class="min-w-0 flex-1">
            <Banner tone="danger" icon="warn" title={t(S.providers.key.clearTitle)}>
              <TRich msg={S.providers.key.clearBody} />
            </Banner>
          </div>
          <Button
            label={t(S.providers.key.undo)}
            variant="outline"
            onClick={() => props.onMode("keep")}
          />
        </div>
      </Show>

      <Show when={props.mode === "set"}>
        <div class="flex flex-col gap-2xs">
          <TextField
            label={props.hadKey ? t(S.providers.key.newLabel) : t(S.providers.key.label)}
            type="password"
            value={props.text}
            onInput={props.onText}
            ref={
              props.focus === true
                ? (el) => queueMicrotask(() => el.focus())
                : undefined
            }
            mono
            placeholder={optional() ? t(S.providers.key.placeholderOptional) : "sk-…"}
            hint={props.hadKey ? t(S.providers.key.hintHad) : t(S.providers.key.hintNew)}
            more={t(S.providers.key.more)}
          />
          <Show when={props.hadKey}>
            <div class="flex gap-sm">
              <Button
                label={t(S.providers.key.keepOld)}
                variant="ghost"
                onClick={() => props.onMode("keep")}
              />
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
}

/** The probe result, with `message` shown verbatim because the three failure kinds need three different fixes; `role="status"` since it appears after an action. */
function ProbeResult(props: { busy: boolean; probe: ProviderProbe | null; error: string | null }) {
  // Connected with no models is `ok: true` but unusable, so it takes a warning tone.
  const tone = () => {
    if (props.error !== null) return "danger" as const;
    const probe = props.probe;
    if (probe === null) return "info" as const;
    if (!probe.ok) return "danger" as const;
    return probe.models.length === 0 ? ("warn" as const) : ("accent" as const);
  };

  return (
    <div role="status" aria-live="polite" aria-busy={props.busy} class="flex flex-col gap-2xs">
      <Show when={props.busy}>
        <Banner tone="info" icon="refresh">
          {t(S.providers.probe.busy)}
        </Banner>
      </Show>

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" title={t(S.providers.probe.failedTitle)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={!props.busy && props.probe}>
        {(probe) => (
          <Banner
            tone={tone()}
            icon={probe().ok ? "check" : "warn"}
            title={probe().ok ? t(S.providers.probe.okTitle) : t(S.providers.probe.badTitle)}
          >
            <p class="m-0 text-xs">{probe().message}</p>
          </Banner>
        )}
      </Show>
    </div>
  );
}


/** This provider's chat model, typed or picked from the list the probe just returned, so the choice happens where the intent is; one hint line below covers all three states. */
function ModelSection(props: {
  value: string | null;
  onPick: (model: string | null) => void;
  models: ModelChoice[];
  busy: boolean;
  /** Has the server been asked at least once, to tell "not asked" from "asked and empty". */
  touched: boolean;
}) {
  return (
    <div class="flex flex-col gap-2xs">
      <ModelField
        role="chat"
        label={t(S.providers.form.chatModel)}
        showLabel
        models={props.models}
        value={props.value ?? ""}
        onInput={(value) => props.onPick(value.trim() === "" ? null : value)}
        placeholder={t(S.providers.form.chatModelPlaceholder)}
        more={t(S.providers.form.chatModelMore)}
      />

      {/* One line, three states in time order: asking, asked and empty, then the standing hint. */}
      <Show
        when={props.busy && props.models.length === 0}
        fallback={
          <Show
            when={props.touched && !props.busy && props.models.length === 0}
            fallback={
              <p class="m-0 text-2xs text-faint">{t(S.providers.form.blankOk)}</p>
            }
          >
            <p class="m-0 flex items-center gap-2xs text-2xs text-muted">
              {t(S.providers.form.noModels)}
              <InfoDot
                label={t(S.providers.form.noModelsLabel)}
                text={t(S.providers.form.noModelsText)}
              />
            </p>
          </Show>
        }
      >
        <p class="m-0 text-2xs text-muted" role="status" aria-busy="true">
          {t(S.providers.picker.loading)}
        </p>
      </Show>
    </div>
  );
}
