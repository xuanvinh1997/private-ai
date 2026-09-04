import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { createSignal } from "solid-js";
import { inTauri } from "./agent";
import type { AsrSetting, DictationUpdate } from "./protocol";

/** The state of the setting when the core cannot be asked -- the demo build, and the first paint. */
const UNSET: AsrSetting = {
  enabled: true,
  model: "",
  language: "",
  info: null,
  reason: null,
};

export async function asrSetting(): Promise<AsrSetting> {
  if (!inTauri()) return UNSET;
  try {
    return await invoke<AsrSetting>("asr_setting");
  } catch (err) {
    console.error("failed to read the speech setting", err);
    return UNSET;
  }
}

export function setAsr(next: {
  enabled: boolean;
  model: string;
  language: string;
}): Promise<AsrSetting> {
  return invoke<AsrSetting>("set_asr", next);
}

/** Load the chosen model and report what it is. Slow: this is the call that pays for the weights. */
export function probeAsr(): Promise<AsrSetting> {
  return invoke<AsrSetting>("probe_asr");
}

/** OS file dialog for a GGUF model; an empty string means the user cancelled. */
export async function pickAsrModel(): Promise<string> {
  if (!inTauri()) return "";
  const picked = await open({
    directory: false,
    multiple: false,
    filters: [{ name: "GGUF", extensions: ["gguf"] }],
  });
  if (picked === null) return "";
  return Array.isArray(picked) ? (picked[0] ?? "") : picked;
}

/* --- Dictation ---------------------------------------------------------- */

/** What the composer renders while the microphone is open. */
export interface DictationState {
  /** True from the click until the final text arrives, so the button can say "stop". */
  active: boolean;
  /** Text the model will not revise; safe to render without flicker. */
  committed: string;
  /** The volatile tail. Shown dimmed, because it can be rewritten on the next tick. */
  tentative: string;
  recordedMs: number;
  device: string | null;
  /** False for a model that only transcribes at the end: the UI then shows a clock, not text. */
  streaming: boolean;
  error: string | null;
}

const IDLE: DictationState = {
  active: false,
  committed: "",
  tentative: "",
  recordedMs: 0,
  device: null,
  streaming: false,
  error: null,
};

const [dictation, setDictation] = createSignal<DictationState>(IDLE);
export { dictation };

/** The text as it should appear in the box right now. */
export function dictationText(state: DictationState): string {
  return `${state.committed}${state.tentative}`;
}

/**
 * Open the microphone. `onFinished` receives the final transcript once -- and only on a clean
 * finish, so a cancelled dictation never writes into the composer.
 *
 * Audio never reaches this side: the core captures, resamples and recognizes, and what crosses the
 * bridge is text.
 */
export async function startDictation(handlers: {
  onFinished: (text: string) => void;
  onFailed?: (message: string) => void;
}): Promise<void> {
  if (!inTauri()) throw new Error("dictation needs the desktop app");
  setDictation({ ...IDLE, active: true });

  const channel = new Channel<DictationUpdate>();
  channel.onmessage = (update) => {
    if (update.kind === "started") {
      setDictation((current) => ({
        ...current,
        device: update.device,
        streaming: update.streaming,
      }));
      return;
    }
    if (update.kind === "text") {
      setDictation((current) => ({
        ...current,
        committed: update.committed,
        tentative: update.tentative,
        recordedMs: update.recordedMs,
      }));
      return;
    }
    if (update.kind === "recording") {
      setDictation((current) => ({ ...current, recordedMs: update.recordedMs }));
      return;
    }
    if (update.kind === "finished") {
      setDictation(IDLE);
      handlers.onFinished(update.text ?? "");
      return;
    }
    if (update.kind === "failed") {
      const message = update.error ?? "";
      setDictation({ ...IDLE, error: message });
      handlers.onFailed?.(message);
    }
  };

  try {
    await invoke("start_dictation", { onUpdate: channel });
  } catch (err) {
    setDictation({ ...IDLE, error: String(err) });
    throw err;
  }
}

/** Stop and keep the text; the final transcript arrives through the channel. */
export function stopDictation(): Promise<void> {
  return invoke("stop_dictation");
}

/** Stop and throw the text away. */
export async function cancelDictation(): Promise<void> {
  setDictation(IDLE);
  await invoke("cancel_dictation");
}

/** `m:ss`, the only clock a dictation ever needs. */
export function elapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
