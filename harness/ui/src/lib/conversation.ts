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

/**
 * Bản ghi hội thoại của một phiên.
 *
 * Dùng `createStore` chứ không `createSignal<ConversationNode[]>`: với signal, mỗi
 * token phải sinh một mảng mới và mọi thứ nghe mảng đó đều chạy lại — đó chính là cái
 * bẫy mà `createEffect(() => void props.messages)` ở bản trước rơi vào. Với store, ghi
 * vào `nodes[i].text` chỉ đánh thức đúng chỗ đọc `nodes[i].text`.
 */
/**
 * Bản ghi đã lưu → node để vẽ.
 *
 * Ở đây chứ không trong `App` vì nó là phép dịch giữa hai từ vựng, và từ vựng bên trái
 * thuộc về sổ tay phiên — chỗ duy nhất hai bên chạm nhau nên ở cạnh phần dựng node.
 */
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
          // Bản ghi đã lưu không có lượt nào đang chạy: mọi lời gọi trong đó đã xong.
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

  let seq = 0;
  const nextId = (prefix: string) => `${prefix}-${seq++}`;

  /** Khối trợ lý đang mở. `null` nghĩa là token kế tiếp mở một khối mới. */
  let openAssistant: string | null = null;

  // Gộp token trong một frame. Rust đã gộp ~16ms một lần rồi, nhưng một lượt chạy
  // nhanh vẫn có thể dồn nhiều `item` vào cùng một frame; nối chuỗi trong bộ đệm rẻ
  // hơn nhiều so với ghi store nhiều lần rồi để layout chạy nhiều lần.
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

  /** Đóng khối trợ lý đang mở — dùng khi có gì đó chen vào giữa dòng chữ. */
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
    // Danh sách việc là *projection*, không phải dòng thời gian: mỗi lần host gửi là
    // toàn bộ trạng thái mới. Giữ một node duy nhất và ghi đè, đúng như dsh — nếu đẩy
    // node mới mỗi lần thì transcript đầy bản sao của cùng một danh sách.
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
        // Host rút câu hỏi. Chỉ đóng nếu đúng câu đang hỏi: một `cancel` đến muộn không
        // được phép nuốt mất câu hỏi kế tiếp.
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
    approval,
    clearApproval: () => setApproval(null),
    applyEvent,
    /** Người dùng gửi. Trả về id để chỗ gọi cuộn tới nếu cần. */
    addUser(text: string): string {
      closeAssistant();
      const id = nextId("u");
      push({ id, kind: "user", text, at: Date.now() });
      return id;
    },
    /**
     * Xoá một node khỏi bản ghi *đang xem*.
     *
     * Chỉ đụng tới bản sao trên màn hình: sổ tay phiên là sổ **chỉ ghi thêm**, và một
     * nút "xoá" trong giao diện không được phép trở thành đường duy nhất làm thủng bất
     * biến đó. Nạp lại phiên thì node quay về — đúng như nó phải thế.
     */
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
    /** Đóng mọi thứ còn mở khi lượt kết thúc bất thường. */
    finishTurn(): void {
      closeAssistant();
      setBusy(false);
    },
  };
}

export type Conversation = ReturnType<typeof createConversation>;
