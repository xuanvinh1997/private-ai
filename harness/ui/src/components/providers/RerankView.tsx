import { createEffect, createSignal, onMount, Show } from "solid-js";
import { rerankSetting, setRerank } from "../../lib/providers";
import type { RerankSetting } from "../../lib/protocol";
import { S, t } from "../../lib/i18n";
import {
  Banner,
  Row,
  RowGroup,
  SectionHead,
  Select,
  TextField,
  Toggle,
} from "../settings/FormKit";

/** The rerank section, last on the Models page: it is the third model on the retrieval path, it is a downloaded model file rather than a server, and changing anything here never re-embeds. */
/** A `TextField` that commits on blur or Enter, since per-keystroke saves let the core clamp a half-typed number. */
function CommitField(props: {
  label: string;
  value: string;
  disabled?: boolean;
  mono?: boolean;
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
      mono={props.mono}
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
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => setSetting(await rerankSetting()));

  /** Save a change, then redraw from what the core returned, not from what was sent. */
  async function save(patch: Partial<Omit<RerankSetting, "reason">>) {
    const now = setting();
    if (!now || saving()) return;
    // Draw optimistically: a toggle that waits for the round trip feels stuck.
    const next = { ...now, ...patch };
    setSetting(next);
    setSaving(true);
    setError(null);
    try {
      // The core clamps `candidates` and `topN`, so the reply can differ from the request.
      setSetting(
        await setRerank({
          enabled: next.enabled,
          backend: next.backend,
          model: next.model,
          candidates: next.candidates,
          topN: next.topN,
        }),
      );
    } catch (err) {
      setSetting(now);
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

      <Show when={setting()}>
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
                    onChange={(enabled) => void save({ enabled })}
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
                          if (next !== null) void save({ candidates: next });
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
                          if (next !== null) void save({ topN: next });
                        }}
                      />
                    </div>
                  )}
                />

                <Row
                  label={t(S.embedding.rerank.backendLabel)}
                  icon="server"
                  desc={t(S.embedding.rerank.backendDesc)}
                  more={t(S.embedding.rerank.backendMore)}
                  control={() => (
                    <Select
                      label={t(S.embedding.rerank.backendSelectLabel)}
                      value={value().backend}
                      disabled={saving()}
                      options={[
                        { id: "onnx", label: t(S.embedding.rerank.backendOnnx) },
                        { id: "http", label: t(S.embedding.rerank.backendHttp) },
                      ]}
                      onPick={(backend) =>
                        void save({ backend: backend as RerankSetting["backend"] })
                      }
                    />
                  )}
                />

                <Row
                  label={t(
                    value().backend === "onnx"
                      ? S.embedding.rerank.repoLabel
                      : S.embedding.rerank.remoteModelLabel,
                  )}
                  icon="model"
                  desc={
                    value().backend === "onnx"
                      ? t(S.embedding.rerank.repoDesc)
                      : t(S.embedding.rerank.remoteModelDesc)
                  }
                  more={
                    value().backend === "onnx"
                      ? t(S.embedding.rerank.repoMore)
                      : undefined
                  }
                  control={() => (
                    <div class="w-[280px] max-w-full">
                      <CommitField
                        label={t(S.embedding.rerank.modelFieldLabel)}
                        mono
                        value={value().model}
                        disabled={saving()}
                        onCommit={(model) => void save({ model })}
                      />
                    </div>
                  )}
                />
              </Show>
            </RowGroup>

            <Show when={value().reason}>
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
          </>
        )}
      </Show>
    </section>
  );
}
