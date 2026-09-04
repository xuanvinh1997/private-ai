import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type {
  AgentEvent,
  ApprovalDecision,
  HistoryNode,
  ModelChoice,
  SessionSummary,
  ToolScope,
} from "./protocol";

/** Whether we run inside the Tauri shell; every `invoke` throws in a plain browser, so guard it in one place. */
export const inTauri = (): boolean => {
  try {
    return isTauri();
  } catch {
    return false;
  }
};

/** Send one turn over a `Channel`, which is ordered and per-turn; `scope` is required so no turn runs at an unchosen level. */
export function sendMessage(
  sessionId: string,
  text: string,
  scope: ToolScope,
  onEvent: (event: AgentEvent) => void,
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("send_message", { input: { sessionId, text, scope }, onEvent: channel });
}

/** Answer an approval request. Fail-closed: if this never reaches the core, the core treats it as a refusal. */
export async function answerApproval(
  requestId: string,
  decision: ApprovalDecision,
): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("approval_result", { requestId, decision });
  } catch (err) {
    console.error("failed to send approval decision", err);
  }
}

/** Cancel the running turn. Silent without a core, so the button still works in the demo. */
export async function cancelTurn(sessionId: string): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("cancel_turn", { sessionId });
  } catch (err) {
    console.error("failed to cancel turn", err);
  }
}

export async function listSessions(): Promise<SessionSummary[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<SessionSummary[]>("list_sessions");
  } catch (err) {
    console.error("failed to list sessions", err);
    return [];
  }
}

export async function createSession(title: string): Promise<SessionSummary | null> {
  if (!inTauri()) return null;
  try {
    return await invoke<SessionSummary>("create_session", { title });
  } catch (err) {
    console.error("failed to create session", err);
    return null;
  }
}

/** Rename a session; failures are logged, not raised, because the on-screen title has already changed. */
export async function renameSession(sessionId: string, title: string): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("rename_session", { sessionId, title });
  } catch (err) {
    console.error("failed to rename session", err);
  }
}

/** Delete a session, throwing on failure: deletion is irreversible, so "thought it was gone" must be visible. */
export async function deleteSession(sessionId: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("delete_session", { sessionId });
}

/** Load a session's stored transcript; throws, because silence here is indistinguishable from an empty session. */
export async function loadSession(sessionId: string): Promise<HistoryNode[]> {
  if (!inTauri()) return [];
  return await invoke<HistoryNode[]>("load_session", { sessionId });
}

/** Models the server offers. An empty list means the server did not answer, not that there are no models. */
export async function listModels(): Promise<ModelChoice[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<ModelChoice[]>("list_models");
  } catch (err) {
    console.error("failed to list models", err);
    return [];
  }
}
