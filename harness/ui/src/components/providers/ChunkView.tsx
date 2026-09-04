import { createEffect, createSignal, onMount, Show } from "solid-js";
import { chunkSetting, setChunk } from "../../lib/providers";
import type { ChunkSetting } from "../../lib/protocol";
import { S, t } from "../../lib/i18n";
import ConfirmDialog from "./ConfirmDialog";
import { Banner, Button, Row, RowGroup, SectionHead, TextField } from "../settings/FormKit";

/** A number field that commits on blur or Enter, never per keystroke: a half-typed "1" would otherwise be
 * saved as a clamped 200 and the field would fight the person typing. */
function CommitField(props: {
  label: string;
  value: number;
  disabled?: boolean;
  onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = createSignal(String(props.value));
  // A value coming back from the core, clamped or not, must overwrite the draft.
  createEffect(() => setDraft(String(props.value)));

  const commit = () => {
    const next = Number.parseInt(draft().trim(), 10);
    if (Number.isFinite(next) && next !== props.value) props.onCommit(next);
    else setDraft(String(props.value));
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

/** Chunk size and overlap. It sits under the embedding model because it is the step immediately before
 * embedding, and it asks to be confirmed for the same reason that screen does: what is already stored stops
 * describing anything the library would produce at the new numbers. */
export default function ChunkView() {
  const [setting, setSetting] = createSignal<ChunkSetting | null>(null);
  const [draft, setDraft] = createSignal<ChunkSetting | null>(null);
  const [saving, setSaving] = createSignal(false);
  const [confirming, setConfirming] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    const current = await chunkSetting();
    setSetting(current);
    setDraft(current);
  });

  const update = (patch: Partial<Omit<ChunkSetting, "reason">>) => {
    setDraft((current) => (current === null ? null : { ...current, ...patch }));
    setError(null);
  };

  const dirty = () => {
    const current = setting();
    const next = draft();
    return (
      current !== null &&
      next !== null &&
      (current.size !== next.size || current.overlap !== next.overlap)
    );
  };

  /** Persist the whole draft, then redraw from the core's clamped answer rather than from what was typed. */
  const save = async () => {
    const next = draft();
    if (next === null || saving() || !dirty()) return;
    setSaving(true);
    setError(null);
    try {
      const stored = await setChunk(next.size, next.overlap);
      setSetting(stored);
      setDraft(stored);
      setConfirming(false);
    } catch (err) {
      setError(
        t(S.embedding.chunk.saveFailed, {
          detail: err instanceof Error ? err.message : String(err),
        }),
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <section class="flex flex-col gap-3">
      <SectionHead
        title={t(S.embedding.chunk.title)}
        icon="list"
        desc={t(S.embedding.chunk.desc)}
        more={t(S.embedding.chunk.more)}
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title={t(S.common.actionFailed)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={draft()}>
        {(value) => (
          <>
            <RowGroup>
              <Row
                label={t(S.embedding.chunk.sizeLabel)}
                icon="list"
                desc={t(S.embedding.chunk.sizeDesc)}
                more={t(S.embedding.chunk.sizeMore)}
                control={() => (
                  <div class="w-[120px]">
                    <CommitField
                      label={t(S.embedding.chunk.sizeLabel)}
                      value={value().size}
                      disabled={saving()}
                      onCommit={(size) => update({ size })}
                    />
                  </div>
                )}
              />

              <Row
                label={t(S.embedding.chunk.overlapLabel)}
                icon="fold"
                desc={t(S.embedding.chunk.overlapDesc)}
                more={t(S.embedding.chunk.overlapMore)}
                control={() => (
                  <div class="w-[120px]">
                    <CommitField
                      label={t(S.embedding.chunk.overlapLabel)}
                      value={value().overlap}
                      disabled={saving()}
                      onCommit={(overlap) => update({ overlap })}
                    />
                  </div>
                )}
              />
            </RowGroup>

            <Show when={setting()?.reason}>
              {(reason) => (
                <p class="m-0 max-w-[62ch] text-2xs text-muted">{reason()}</p>
              )}
            </Show>

            <div class="flex flex-wrap items-center justify-end gap-sm">
              <span role="status" aria-live="polite" class="mr-auto text-2xs text-muted">
                {dirty() ? t(S.embedding.chunk.unsaved) : ""}
              </span>
              <Button
                label={t(S.common.save)}
                icon="check"
                busy={saving()}
                disabled={!dirty()}
                onClick={() => setConfirming(true)}
              />
            </div>
          </>
        )}
      </Show>

      <Show when={confirming()}>
        <ConfirmDialog
          title={t(S.embedding.chunk.confirmTitle)}
          body={t(S.embedding.chunk.confirmBody)}
          more={t(S.embedding.chunk.confirmMore)}
          detail={[
            t(S.embedding.chunk.confirmNow, {
              size: setting()?.size ?? "?",
              overlap: setting()?.overlap ?? "?",
            }),
            t(S.embedding.chunk.confirmNext, {
              size: draft()?.size ?? "?",
              overlap: draft()?.overlap ?? "?",
            }),
          ].join("\n")}
          confirmLabel={t(S.embedding.chunk.confirmLabel)}
          busy={saving()}
          onConfirm={() => void save()}
          onClose={() => setConfirming(false)}
        />
      </Show>
    </section>
  );
}
