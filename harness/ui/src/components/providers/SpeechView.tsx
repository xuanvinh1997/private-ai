import { createSignal, onMount, Show } from "solid-js";
import { asrSetting, pickAsrModel, probeAsr, setAsr } from "../../lib/asr";
import { S, t } from "../../lib/i18n";
import type { AsrSetting } from "../../lib/protocol";
import { Banner, Button, Row, RowGroup, SectionHead, TextField, Toggle } from "../settings/FormKit";

/**
 * The speech model: one local GGUF that both reads audio files in a document project and listens to
 * the microphone. Unlike the other tabs there is no provider to pick, because there is no server --
 * the file on disk is the whole setting.
 */
export default function SpeechView() {
  const [setting, setSetting] = createSignal<AsrSetting | null>(null);
  const [saving, setSaving] = createSignal(false);
  const [probing, setProbing] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => setSetting(await asrSetting()));

  /** Every change saves at once: there is no half-typed state here worth a Save button. */
  async function commit(patch: Partial<Pick<AsrSetting, "enabled" | "model" | "language">>) {
    const current = setting();
    if (current === null || saving()) return;
    const next = { ...current, ...patch };
    setSetting(next);
    setSaving(true);
    setError(null);
    try {
      setSetting(await setAsr({ enabled: next.enabled, model: next.model, language: next.language }));
    } catch (err) {
      setSetting(current);
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function choose() {
    const path = await pickAsrModel();
    if (path === "") return;
    await commit({ model: path });
  }

  /** The only honest answer to "will this model work": load it and say what came back. */
  async function probe() {
    if (probing()) return;
    setProbing(true);
    setError(null);
    try {
      setSetting(await probeAsr());
    } catch (err) {
      setError(String(err));
    } finally {
      setProbing(false);
    }
  }

  const chosen = () => (setting()?.model ?? "") !== "";
  /** The file name alone; the full path is long enough to push the button off the row. */
  const fileName = () => {
    const path = setting()?.model ?? "";
    return path.split(/[\\/]/).pop() ?? path;
  };

  return (
    <section class="flex flex-col gap-3">
      <SectionHead
        title={t(S.speech.section.title)}
        icon="mic"
        desc={t(S.speech.section.desc)}
        more={t(S.speech.section.more)}
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" title={t(S.speech.saveFailed)}>
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={setting()}>
        {(value) => (
          <>
            <RowGroup>
              <Row
                label={t(S.speech.model.label)}
                icon="model"
                desc={t(S.speech.model.desc)}
                more={t(S.speech.model.more)}
                control={() => (
                  <div class="flex items-center gap-sm">
                    <span
                      class="max-w-[260px] truncate font-mono text-xs"
                      classList={{ "text-ink": chosen(), "text-muted": !chosen() }}
                      title={value().model}
                    >
                      {chosen() ? fileName() : t(S.speech.model.none)}
                    </span>
                    <Button
                      label={t(S.speech.model.pick)}
                      variant="outline"
                      icon="folder"
                      disabled={saving()}
                      onClick={() => void choose()}
                    />
                  </div>
                )}
              />

              <Row
                label={t(S.speech.language.label)}
                icon="globe"
                desc={t(S.speech.language.desc)}
                more={t(S.speech.language.more)}
                dim={!chosen()}
                control={() => (
                  <div class="w-[140px]">
                    <TextField
                      label={t(S.speech.language.label)}
                      hideLabel
                      mono
                      value={value().language}
                      placeholder={t(S.speech.language.placeholder)}
                      disabled={saving() || !chosen()}
                      onInput={(language) => setSetting({ ...value(), language })}
                      ref={(el) => {
                        const send = () => void commit({ language: el.value.trim() });
                        el.addEventListener("blur", send);
                        el.addEventListener("keydown", (event) => {
                          if (event.key === "Enter") {
                            event.preventDefault();
                            el.blur();
                          }
                        });
                      }}
                    />
                  </div>
                )}
              />

              <Row
                label={t(S.speech.library.label)}
                icon="library"
                desc={t(S.speech.library.desc)}
                more={t(S.speech.library.more)}
                dim={!chosen()}
                control={() => (
                  <Toggle
                    label={t(S.speech.library.toggleLabel)}
                    checked={value().enabled}
                    disabled={saving() || !chosen()}
                    busy={saving()}
                    onChange={(enabled) => void commit({ enabled })}
                  />
                )}
              />

              <Show when={chosen()}>
                <Row
                  label={t(S.speech.probe.action)}
                  icon="bolt"
                  desc={t(S.speech.probe.running)}
                  control={() => (
                    <Button
                      label={t(S.speech.probe.action)}
                      variant="outline"
                      icon="refresh"
                      busy={probing()}
                      onClick={() => void probe()}
                    />
                  )}
                  below={() => (
                    <Show when={value().info}>
                      {(info) => (
                        <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-sm gap-y-3xs text-2xs text-muted">
                          <dt>{t(S.speech.probe.arch)}</dt>
                          <dd class="m-0 font-mono text-ink">{info().variant}</dd>
                          <dt>{t(S.speech.probe.backend)}</dt>
                          <dd class="m-0 font-mono text-ink">{info().backend}</dd>
                          <dt>{t(S.speech.probe.streaming)}</dt>
                          <dd class="m-0 text-ink">
                            {info().streaming ? t(S.common.on) : t(S.common.off)}
                          </dd>
                          <dt>{t(S.speech.probe.languagesLabel)}</dt>
                          <dd class="m-0 text-ink">
                            {t(S.speech.probe.languages, { n: info().languages.length })}
                          </dd>
                        </dl>
                      )}
                    </Show>
                  )}
                />
              </Show>
            </RowGroup>

            <Show when={value().reason}>
              {(reason) => (
                <Banner tone="info" icon="info">
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
