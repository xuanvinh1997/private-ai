import { createSignal, onCleanup } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { intendedDiffs } from "./diff";
import type { AgentEvent, ConversationNode, HistoryNode, TodoItem } from "./protocol";

export interface PendingApproval {
  requestId: string;
  callId: string;
  name: string;
  args: unknown;
  reason: string | null;
  timeoutMs: number | null;
}

/** A session transcript on `createStore`, not a signal: writing `nodes[i].text` only wakes readers of that field. */
/** Stored history to renderable nodes: a translation between two vocabularies, kept next to the node builders. */
export function nodesFromHistory(history: HistoryNode[]): ConversationNode[] {
  return history.map((entry): ConversationNode => {
    if (entry.kind === "tool") {
      return {
        id: entry.id,
        kind: "tool",
        at: entry.created_at,
        call: {
          callId: entry.call_id,
          name: entry.name,
          args: entry.args,
          // Stored history has no running turn: every call in it has already finished.
          state: entry.is_error ? "error" : "ok",
          preview: entry.preview,
          meta: entry.meta ?? undefined,
        },
      };
    }
    return entry.kind === "user"
      ? { id: entry.id, kind: "user", text: entry.text, at: entry.created_at }
      : { id: entry.id, kind: "assistant", text: entry.text, streaming: false, at: entry.created_at };
  });
}

export function createConversation() {
  const [state, setState] = createStore<{ nodes: ConversationNode[] }>({ nodes: [] });
  const [busy, setBusy] = createSignal(false);
  const [approval, setApproval] = createSignal<PendingApproval | null>(null);
  /** Context used by the latest step; kept, not summed, because each call resends the whole history. */
  const [usage, setUsage] = createSignal<{ used: number; window: number | null } | null>(null);

  let seq = 0;
  const nextId = (prefix: string) => `${prefix}-${seq++}`;

  /** The open assistant block; `null` means the next token starts a new one. */
  let openAssistant: string | null = null;

  // Coalesce tokens per frame: appending to a buffer is far cheaper than repeated store writes and relayouts.
  let buffer = "";
  let frame: number | undefined;

  const flush = () => {
    frame = undefined;
    if (buffer === "" || openAssistant === null) return;
    const id = openAssistant;
    const chunk = buffer;
    buffer = "";
    setState(
      produce((s) => {
        const node = s.nodes.find((n) => n.id === id);
        if (node?.kind === "assistant") node.text += chunk;
      }),
    );
  };

  const schedule = () => {
    if (frame !== undefined) return;
    frame =
      typeof requestAnimationFrame === "function" ? requestAnimationFrame(flush) : void flush();
  };

  onCleanup(() => {
    if (frame !== undefined && typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(frame);
    }
  });

  const push = (node: ConversationNode) => {
    setState(produce((s) => void s.nodes.push(node)));
  };

  /** Close the open assistant block, for when something interrupts mid-text. */
  const closeAssistant = () => {
    flush();
    if (openAssistant === null) return;
    const id = openAssistant;
    openAssistant = null;
    setState(
      produce((s) => {
        const node = s.nodes.find((n) => n.id === id);
        if (node?.kind === "assistant") node.streaming = false;
      }),
    );
  };

  const patchTool = (callId: string, fn: (node: Extract<ConversationNode, { kind: "tool" }>) => void) => {
    setState(
      produce((s) => {
        const node = s.nodes.find((n) => n.kind === "tool" && n.call.callId === callId);
        if (node?.kind === "tool") fn(node);
      }),
    );
  };

  const setTodos = (items: TodoItem[]) => {
    // The todo list is a projection, not a timeline: keep one node and overwrite it, or the transcript fills with copies.
    setState(
      produce((s) => {
        const node = s.nodes.find((n) => n.kind === "todo");
        if (node?.kind === "todo") node.items = items;
        else s.nodes.push({ id: "todo", kind: "todo", items });
      }),
    );
  };

  function applyEvent(event: AgentEvent): void {
    switch (event.kind) {
      case "token": {
        if (openAssistant === null) {
          openAssistant = nextId("a");
          push({ id: openAssistant, kind: "assistant", text: "", streaming: true, at: Date.now() });
        }
        buffer += event.text;
        schedule();
        break;
      }
      case "tool_start": {
        closeAssistant();
        const diffs = intendedDiffs(event.name, event.args);
        push({
          id: `t-${event.call_id}`,
          kind: "tool",
          call: {
            callId: event.call_id,
            name: event.name,
            args: event.args,
            state: "running",
            ...(diffs ? { intendedDiffs: diffs } : {}),
          },
        });
        break;
      }
      case "tool_end": {
        patchTool(event.call_id, (node) => {
          node.call.name = event.name;
          node.call.state = event.is_error ? "error" : "ok";
          node.call.preview = event.preview;
          if (event.meta) node.call.meta = event.meta;
        });
        break;
      }
      case "diff": {
        patchTool(event.call_id, (node) => void (node.call.intendedDiffs = event.diffs));
        break;
      }
      case "todo": {
        setTodos(event.items);
        break;
      }
      case "usage": {
        setUsage({
          used: event.input_tokens + event.output_tokens,
          window: event.context_window,
        });
        break;
      }
      case "notice": {
        closeAssistant();
        push({ id: nextId("n"), kind: "notice", message: event.message });
        break;
      }
      case "progress": {
        closeAssistant();
        push({ id: nextId("p"), kind: "progress", label: event.label, detail: event.detail });
        break;
      }
      case "error": {
        closeAssistant();
        push({ id: nextId("e"), kind: "error", message: event.message });
        break;
      }
      case "approval_request": {
        setApproval({
          requestId: event.request_id,
          callId: event.call_id,
          name: event.name,
          args: event.args,
          reason: event.reason,
          timeoutMs: event.timeout_ms,
        });
        break;
      }
      case "approval_cancel": {
        // The host withdrew the question; only close the matching one, so a late `cancel` cannot eat the next request.
        setApproval((current) => (current?.requestId === event.request_id ? null : current));
        break;
      }
      case "final": {
        closeAssistant();
        break;
      }
    }
  }

  return {
    nodes: () => state.nodes,
    busy,
    setBusy,
    usage,
    /** Forget the old counters when switching sessions; they describe the session just left. */
    clearUsage: () => setUsage(null),
    approval,
    clearApproval: () => setApproval(null),
    applyEvent,
    /** User submit. Returns the id so the caller can scroll to it. */
    addUser(text: string): string {
      closeAssistant();
      const id = nextId("u");
      push({ id, kind: "user", text, at: Date.now() });
      return id;
    },
    /** Remove a node from the *displayed* transcript only; the session log is append-only, so a reload brings it back. */
    removeNode(id: string): void {
      setState(
        produce((s) => {
          const at = s.nodes.findIndex((node) => node.id === id);
          if (at >= 0) s.nodes.splice(at, 1);
        }),
      );
    },
    reset(nodes: ConversationNode[] = []): void {
      openAssistant = null;
      buffer = "";
      setState("nodes", nodes);
    },
    /** Close everything still open when a turn ends abnormally. */
    finishTurn(): void {
      closeAssistant();
      setBusy(false);
    },
  };
}

export type Conversation = ReturnType<typeof createConversation>;
