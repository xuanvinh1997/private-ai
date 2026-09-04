import { createEffect, createSignal, on, Show } from "solid-js";
import { locale, S, t } from "../../lib/i18n";
import type { ModelChoice } from "../../lib/protocol";
import { usableForChat } from "../ModelPicker";
import { InfoDot, Select, TextField } from "../settings/FormKit";

/**
 * One model field for chat, embedding and vision, since all three ask the same question.
 * Capability flags order the list rather than filter it, typing a name is always possible,
 * and a stored value is never rewritten behind the user's back.
 */

/** Models the server calls embedding-capable; used for ordering, never for filtering. */
export const embeddable = (model: ModelChoice) => model.embedding;

/** Models the server calls image-capable; ordering only, since a server that declares nothing would erase the list. */
export const seeing = (model: ModelChoice) => model.vision;

/** Do two names mean one model; `:latest` is dropped because Ollama lists it but accepts the bare name. */
export const sameModel = (left: string, right: string) => {
  const bare = (value: string) => value.trim().toLowerCase().replace(/:latest$/, "");
  return bare(left) === bare(right);
};

/** A model the server lists but says cannot see; only answerable when the list came back at all. */
export const cannotSee = (models: ModelChoice[], value: string) => {
  const hit = models.find((entry) => sameModel(entry.id, value));
  return hit !== undefined && !hit.vision;
};

/** A configured name the server does not list; only answerable when there is a list to compare against. */
export const notListed = (models: ModelChoice[], value: string) =>
  models.length > 0 && value.trim() !== "" && !models.some((entry) => sameModel(entry.id, value));

/** The "type another name" option; it starts with `<` so it cannot collide with a real model id. */
const CUSTOM = "<custom>";

export type ModelRole = "chat" | "embedding" | "vision";

/** Which models fit a role; ill-fitting ones stay in the list, just annotated. */
const FITS: Record<ModelRole, (model: ModelChoice) => boolean> = {
  chat: usableForChat,
  embedding: embeddable,
  // Ollama's `/api/show` and LM Studio's listing both declare this one, so it is usually the server's own
  // answer rather than a guess. Still only an ordering: a server that declares nothing would otherwise
  // present an empty picker for a role that has a working model in it.
  vision: seeing,
};

export default function ModelField(props: {
  role: ModelRole;
  /** The field's name, always reaching screen readers whether or not it is drawn. */
  label: string;
  /** Draw the label; off when the field sits in a `<Row>` that already labels it. */
  showLabel?: boolean;
  hint?: string;
  more?: string;
  models: ModelChoice[];
  value: string;
  placeholder?: string;
  disabled?: boolean;
  onInput: (value: string) => void;
}) {
  /** The user deliberately chose to type a name; the unlisted case below derives from props instead. */
  const [chose, setChose] = createSignal(false);

  // A new server is a new question: keeping "typing" across a list change would hide the new picker.
  createEffect(on(() => props.models, () => setChose(false), { defer: true }));

  const typing = () => chose() || notListed(props.models, props.value);

  /** The name as the server lists it, so the select matches; it is never written back out. */
  const listed = () =>
    props.models.find((entry) => sameModel(entry.id, props.value))?.id ?? props.value;

  const options = () => {
    const fits = FITS[props.role];
    const label = (model: ModelChoice, ok: boolean) => {
      // The context window is shown for chat only; beside an embedding model it means nothing.
      const n =
        props.role === "chat" && model.contextWindow !== null
          ? Intl.NumberFormat(locale() === "vi" ? "vi-VN" : "en-US").format(model.contextWindow)
          : null;
      const id = model.id;
      if (ok) return n === null ? id : t(S.providers.opt.ctx, { id, n });
      if (props.role === "vision") return t(S.providers.opt.notVision, { id });
      if (props.role === "embedding") return t(S.providers.opt.notEmbed, { id });
      return n === null
        ? t(S.providers.opt.chatOnlyEmbed, { id })
        : t(S.providers.opt.chatOnlyEmbedCtx, { id, n });
    };
    return [
      ...props.models.filter(fits).map((entry) => ({ id: entry.id, label: label(entry, true) })),
      ...props.models
        .filter((entry) => !fits(entry))
        .map((entry) => ({ id: entry.id, label: label(entry, false) })),
      { id: CUSTOM, label: t(S.providers.opt.custom) },
    ];
  };

  return (
    <div class="flex min-w-0 flex-col gap-2xs">
      {/* A `<span>`, not `<label for>`: the control swaps between select and input, and both carry `aria-label`. */}
      <Show when={props.showLabel === true}>
        <span class="flex items-center gap-2xs text-2xs text-faint">
          {props.label}
          <Show when={props.more}>
            {(text) => (
              <InfoDot text={text()} label={t(S.providers.field.about, { label: props.label })} />
            )}
          </Show>
        </span>
      </Show>

      <Show when={props.models.length > 0}>
        <Select
          label={props.label}
          mono
          full
          value={typing() ? CUSTOM : listed()}
          options={options()}
          disabled={props.disabled}
          onPick={(value) => {
            if (value === CUSTOM) {
              setChose(true);
              return;
            }
            setChose(false);
            props.onInput(value);
          }}
        />
      </Show>

      {/* The text field shows with no list, with an unlisted value, or on an explicit choice to type. */}
      <Show when={props.models.length === 0 || typing()}>
        <TextField
          label={props.label}
          hideLabel
          mono
          value={props.value}
          disabled={props.disabled}
          placeholder={props.placeholder}
          onInput={props.onInput}
        />
      </Show>

      {/* The server listed this model and said it cannot see; OCR would fail per page, hours later. */}
      <Show when={props.role === "vision" && cannotSee(props.models, props.value)}>
        <p class="m-0 flex items-center gap-2xs text-2xs text-warn">
          {t(S.providers.field.notVision)}
          <InfoDot
            label={t(S.providers.field.notVisionLabel)}
            text={t(S.providers.field.notVisionText)}
          />
        </p>
      </Show>

      {/* Warn about an unlisted name for embedding only: a wrong chat name breaks on the first message. */}
      <Show when={props.role === "embedding" && notListed(props.models, props.value)}>
        <p class="m-0 flex items-center gap-2xs text-2xs text-warn">
          {t(S.providers.field.notListed)}
          <InfoDot
            label={t(S.providers.field.notListedLabel)}
            text={t(S.providers.field.notListedText)}
          />
        </p>
      </Show>

      <Show when={props.hint}>{(hint) => <p class="m-0 text-2xs text-faint">{hint()}</p>}</Show>
    </div>
  );
}
