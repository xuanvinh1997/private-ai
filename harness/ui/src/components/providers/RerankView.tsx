import { createEffect, createSignal, onMount, Show } from "solid-js";
import { rerankSetting, setRerank } from "../../lib/providers";
import type { RerankSetting } from "../../lib/protocol";
import { S, t } from "../../lib/i18n";
import {
  Banner,
  Button,
  Row,
  RowGroup,
  SectionHead,
  TextField,
  Toggle,
} from "../settings/FormKit";

/** The optional local ONNX rerank section. Changing it never re-embeds documents. */
/** A `TextField` that commits on blur or Enter, since per-keystroke saves let the core clamp a half-typed number. */
function CommitField(props: {
  label: string;
  value: string;
  disabled?: boolean;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = createSignal(props.value);
  // A value coming back from the core, clamped or not, must overwrite the draft.
  createEffect(() => setDraft(props.value));

  const commit = () => {
    if (draft() !== props.value) props.onCommit(draft());
  };

  return (
    <TextField
      label={props.label}
      hideLabel
      value={draft()}
      disabled={props.disabled}
      onInput={setDraft}
      ref={(el) => {
        el.addEventListener("blur", commit);
        el.addEventListener("keydown", (event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            commit();
            el.blur();
          }
        });
      }}
    />
  );
}

export default function RerankView() {
  const [setting, setSetting] = createSignal<RerankSetting | null>(null);
  const [draft, setDraft] = createSignal<RerankSetting | null>(null);
  const [saving, setSaving] = createSignal(false);
  const [saved, setSaved] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    const current = await rerankSetting();
    setSetting(current);
    setDraft(current);
  });

  const update = (patch: Partial<Omit<RerankSetting, "reason">>) => {
    setDraft((current) => (current === null ? null : { ...current, ...patch }));
    setSaved(false);
    setError(null);
  };

  const dirty = () => {
    const current = setting();
    const next = draft();
    return (
      current !== null &&
      next !== null &&
      (current.enabled !== next.enabled ||
        current.candidates !== next.candidates ||
        current.topN !== next.topN)
    );
  };

  /** Persist the complete draft, then redraw from the core's clamped response. */
  async function save() {
    const next = draft();
    if (next === null || saving() || !dirty()) return;
    setSaving(true);
    setError(null);
    try {
      const current = await setRerank(next);
      setSetting(current);
      setDraft(current);
      setSaved(true);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  /** The typed number, or `null` while it is not a number yet. */
  function asNumber(raw: string): number | null {
    const value = Number.parseInt(raw.trim(), 10);
    return Number.isFinite(value) ? value : null;
  }

  return (
    <section class="flex flex-col gap-3">
      <SectionHead
        title={t(S.embedding.rerank.title)}
        icon="graph"
        desc={t(S.embedding.rerank.desc)}
        more={t(S.embedding.rerank.more)}
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" title={t(S.embedding.rerank.saveFailed)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={draft()}>
        {(value) => (
          <>
            <RowGroup>
              <Row
                label={t(S.embedding.rerank.enableLabel)}
                icon="graph"
                desc={t(S.embedding.rerank.enableDesc)}
                more={t(S.embedding.rerank.enableMore)}
                control={() => (
                  <Toggle
                    label={t(S.embedding.rerank.enableToggleLabel)}
                    checked={value().enabled}
                    disabled={saving()}
                    busy={saving()}
                    onChange={(enabled) => update({ enabled })}
                  />
                )}
              />

              <Show when={value().enabled}>
                <Row
                  label={t(S.embedding.rerank.candidatesLabel)}
                  icon="list"
                  desc={t(S.embedding.rerank.candidatesDesc)}
                  more={t(S.embedding.rerank.candidatesMore)}
                  control={() => (
                    <div class="w-[120px]">
                      <CommitField
                        label={t(S.embedding.rerank.candidatesLabel)}
                        value={String(value().candidates)}
                        disabled={saving()}
                        onCommit={(raw) => {
                          const next = asNumber(raw);
                          if (next !== null) update({ candidates: next });
                        }}
                      />
                    </div>
                  )}
                />

                <Row
                  label={t(S.embedding.rerank.topNLabel)}
                  icon="check"
                  desc={t(S.embedding.rerank.topNDesc)}
                  control={() => (
                    <div class="w-[120px]">
                      <CommitField
                        label={t(S.embedding.rerank.topNLabel)}
                        value={String(value().topN)}
                        disabled={saving()}
                        onCommit={(raw) => {
                          const next = asNumber(raw);
                          if (next !== null) update({ topN: next });
                        }}
                      />
                    </div>
                  )}
                />

                <Row
                  label={t(S.embedding.rerank.localModelLabel)}
                  icon="model"
                  desc={t(S.embedding.rerank.localModelDesc)}
                  control={() => (
                    <span class="max-w-[320px] break-all font-mono text-xs text-ink-muted">
                      {value().model} · ONNX INT8
                    </span>
                  )}
                />
              </Show>
            </RowGroup>

            <Show when={setting()?.reason}>
              {(reason) => (
                <Banner
                  tone="warn"
                  icon="warn"
                  more={t(S.embedding.rerank.reasonMore)}
                >
                  {reason()}
                </Banner>
              )}
            </Show>

            <div class="flex flex-wrap items-center justify-end gap-sm">
              <span role="status" aria-live="polite" class="mr-auto text-2xs text-muted">
                {saved() ? t(S.common.saved) : dirty() ? t(S.embedding.rerank.unsaved) : ""}
              </span>
              <Button
                label={t(S.common.save)}
                icon="check"
                busy={saving()}
                disabled={!dirty()}
                onClick={() => void save()}
              />
            </div>
          </>
        )}
      </Show>
    </section>
  );
}
