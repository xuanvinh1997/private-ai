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

/** What the composer renders while the microphone is open.
 *
 * Three phases, not a boolean: the first press pays for loading a speech model, which is seconds of disk
 * and GPU work, and a button that jumps straight to "recording" over a model that is not loaded yet is
 * lying. `loading` is that gap, and the button is disabled for its duration. */
export interface DictationState {
  phase: "idle" | "loading" | "recording";
  /** Text the model will not revise; safe to render without flicker. */
  committed: string;
  /** The volatile tail. Shown dimmed, because it can be rewritten on the next tick. */
  tentative: string;
  /** Audio the core has actually captured, in milliseconds. Reported by the core, so it stops when capture
   * does -- which is why it is not what the clock shows. */
  recordedMs: number;
  /** Wall time since the microphone opened, in milliseconds, kept by a timer on this side. This is the clock:
   * it counts while a device delivers nothing, and a stuck audio clock is a fault the user must be able to see
   * rather than the reason the whole bar looks dead. */
  openMs: number;
  /** Microphone peak of the last tick, `0`–`1`. Drives the meter, and nothing else. */
  level: number;
  /** `openMs` at the last tick loud enough to be a voice; `-1` while nothing has been heard at all. */
  heardMs: number;
  device: string | null;
  /** False for a model that only transcribes at the end: the UI then shows a clock, not text. */
  streaming: boolean;
  error: string | null;
}

const IDLE: DictationState = {
  phase: "idle",
  committed: "",
  tentative: "",
  recordedMs: 0,
  openMs: 0,
  level: 0,
  heardMs: -1,
  device: null,
  streaming: false,
  error: null,
};

const [dictation, setDictation] = createSignal<DictationState>(IDLE);
export { dictation };

/** How often the wall clock is republished. Four ticks a second: a second-resolution clock that never shows
 * a value more than a quarter second stale. */
const CLOCK_MS = 250;

let clock: ReturnType<typeof setInterval> | null = null;
let openedAt = 0;

/** Start the wall clock. Idempotent, because `started` is not guaranteed to arrive exactly once. */
function startClock() {
  stopClock();
  openedAt = Date.now();
  clock = setInterval(() => {
    setDictation((current) =>
      current.phase === "recording" ? { ...current, openMs: Date.now() - openedAt } : current,
    );
  }, CLOCK_MS);
}

function stopClock() {
  if (clock !== null) clearInterval(clock);
  clock = null;
}

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
  // `loading` until the core answers `started`: that event arrives once the model is in memory and the
  // device is open, which is the first moment "recording" is true.
  setDictation({ ...IDLE, phase: "loading" });

  const channel = new Channel<DictationUpdate>();
  channel.onmessage = (update) => {
    if (update.kind === "started") {
      startClock();
      setDictation((current) => ({
        ...current,
        phase: "recording",
        openMs: 0,
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
      const openMs = Date.now() - openedAt;
      setDictation((current) => ({
        ...current,
        recordedMs: update.recordedMs,
        openMs,
        level: update.level,
        heardMs: update.level >= HEARD_LEVEL ? openMs : current.heardMs,
      }));
      return;
    }
    if (update.kind === "finished") {
      stopClock();
      setDictation(IDLE);
      handlers.onFinished(update.text ?? "");
      return;
    }
    if (update.kind === "failed") {
      stopClock();
      const message = update.error ?? "";
      setDictation({ ...IDLE, error: message });
      handlers.onFailed?.(message);
    }
  };

  try {
    await invoke("start_dictation", { onUpdate: channel });
  } catch (err) {
    stopClock();
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
  stopClock();
  setDictation(IDLE);
  await invoke("cancel_dictation");
}

/** Peak that counts as "the microphone heard something", well above a quiet room and below a whisper. */
const HEARD_LEVEL = 0.03;

/** How long a microphone may stay under [`HEARD_LEVEL`] before the UI says so, in milliseconds. Long enough
 * to survive the pause between two sentences, short enough to catch a muted device before a whole paragraph. */
const QUIET_MS = 3_000;

/** Peaks below this are floor noise; above it, the meter starts to move. In dBFS, as ears hear it. */
const FLOOR_DB = -45;
/** Where the meter fills: a normal speaking peak, not the clipping point, or the bar would never fill. */
const CEIL_DB = -3;

/** A peak turned into meter fill, `0`–`1`. Logarithmic: linear amplitude spends its whole range on a shout. */
export function meterFill(level: number): number {
  if (level <= 0) return 0;
  const db = 20 * Math.log10(level);
  return Math.min(1, Math.max(0, (db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)));
}

/** Recording, but nothing has reached the microphone for a while: a muted device, the wrong input, a mic
 * the OS never granted. Worth saying, because the rest of the bar looks identical to working dictation. */
export function micQuiet(state: DictationState): boolean {
  if (state.phase !== "recording") return false;
  if (state.openMs < QUIET_MS) return false;
  return state.openMs - state.heardMs >= QUIET_MS;
}

/** `m:ss`, the only clock a dictation ever needs. */
export function elapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
