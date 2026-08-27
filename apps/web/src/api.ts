import type {
  AsrResult,
  ChatMessage,
  ChatResponse,
  ConversationDetail,
  ConversationRecord,
  DocumentPage,
  DocumentRecord,
  Health,
  MemoryRecord,
  MemoryType,
  ModelInfo,
  ModelEvent,
  Preferences,
  ProviderDraft,
  ProviderProbeResult,
  ProviderRecord,
  WorkspaceRecord,
} from "./types";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api/v1${path}`, init);
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new Error(body.detail ?? `Request failed with ${response.status}`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export const api = {
  health: () => request<Health>("/health"),
  models: () => request<ModelInfo[]>("/models"),
  modelEvents: () => request<ModelEvent[]>("/models/events"),
  loadModel: (name: string) =>
    request<void>(`/models/${encodeURIComponent(name)}/load`, { method: "POST" }),
  updateModel: (name: string) =>
    request<{ name: string; sha256: string }>(`/models/${encodeURIComponent(name)}/update`, {
      method: "POST",
    }),
  setDefaultModel: (task: string, model: string) =>
    request<{ task: string; model: string }>(`/models/defaults/${encodeURIComponent(task)}`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model }),
    }),
  unloadModel: (name: string) =>
    request<void>(`/models/${encodeURIComponent(name)}/unload`, { method: "POST" }),
  deleteModel: (name: string) =>
    request<void>(`/models/${encodeURIComponent(name)}?confirmed=true`, { method: "DELETE" }),
  preferences: () => request<Preferences>("/preferences"),
  updatePreferences: (changes: Partial<Preferences>) =>
    request<Preferences>("/preferences", {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(changes),
    }),
  providers: () => request<ProviderRecord[]>("/providers"),
  createProvider: (draft: ProviderDraft) =>
    request<ProviderRecord>("/providers", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(draft),
    }),
  updateProvider: (id: string, changes: Partial<ProviderDraft> & { enabled?: boolean }) =>
    request<ProviderRecord>(`/providers/${id}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(changes),
    }),
  activateProvider: (id: string) =>
    request<ProviderRecord>(`/providers/${id}/activate`, { method: "POST" }),
  probeProvider: (id: string) =>
    request<ProviderProbeResult>(`/providers/${id}/probe`, { method: "POST" }),
  probeProviderDraft: (draft: Omit<ProviderDraft, "name">) =>
    request<ProviderProbeResult>("/providers/probe", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(draft),
    }),
  deleteProvider: (id: string) =>
    request<void>(`/providers/${id}?confirmed=true`, { method: "DELETE" }),
  chat: (model: string, messages: ChatMessage[]) =>
    request<ChatResponse>("/chat", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model, messages, stream: false }),
    }),
  pullModel: async (
    name: string,
    onProgress: (status: string) => void,
    signal?: AbortSignal,
  ) => {
    const response = await fetch("/api/v1/models/pull", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name }),
      signal,
    });
    if (!response.ok || !response.body) throw new Error("Could not start model download");
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    const consume = (block: string) => {
      let eventType = "message";
      const data: string[] = [];
      for (const line of block.split("\n")) {
        if (line.startsWith("event:")) eventType = line.slice(6).trim();
        if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
      }
      if (!data.length) return;
      const event = JSON.parse(data.join("\n")) as {
        detail?: string;
        status?: string;
        completed?: number;
        total?: number;
      };
      if (eventType === "error") {
        throw new Error(event.detail ?? "Model download failed");
      }
      const percent = event.total
        ? ` ${Math.round(((event.completed ?? 0) / event.total) * 100)}%`
        : "";
      onProgress(`${event.status ?? "Downloading"}${percent}`);
    };
    while (true) {
      const { done, value } = await reader.read();
      buffer += decoder.decode(value, { stream: !done }).replaceAll("\r\n", "\n");
      const blocks = buffer.split("\n\n");
      buffer = blocks.pop() ?? "";
      blocks.forEach(consume);
      if (done) break;
    }
    if (buffer.trim()) consume(buffer);
  },
  uploadDocument: (file: File, workspaceId: string) => {
    const body = new FormData();
    body.append("file", file);
    body.append("workspace_id", workspaceId);
    return request<DocumentRecord>("/documents", { method: "POST", body });
  },
  transcribeAudio: (audio: Blob, filename = "recording.webm") => {
    const body = new FormData();
    body.append("file", audio, filename);
    return request<AsrResult>("/asr/transcribe", { method: "POST", body });
  },
  documents: (
    workspaceId: string,
    limit: number,
    offset: number,
    query = "",
    status = "",
  ) => {
    const search = new URLSearchParams({
      workspace_id: workspaceId,
      limit: String(limit),
      offset: String(offset),
    });
    if (query) search.set("q", query);
    if (status) search.set("status", status);
    return request<DocumentPage>(`/documents?${search}`);
  },
  processDocument: (id: string) =>
    request<{ id: string; status: string }>(`/documents/${id}/process`, { method: "POST" }),
  deleteDocument: (id: string) =>
    request<void>(`/documents/${id}?confirmed=true`, { method: "DELETE" }),
  workspaces: () => request<WorkspaceRecord[]>("/workspaces"),
  createWorkspace: (name: string, description: string) =>
    request<WorkspaceRecord>("/workspaces", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, description }),
    }),
  updateWorkspace: (id: string, name: string, description: string) =>
    request<WorkspaceRecord>(`/workspaces/${id}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, description }),
    }),
  deleteWorkspace: (id: string) =>
    request<void>(`/workspaces/${id}?confirmed=true`, { method: "DELETE" }),
  conversations: (workspaceId: string) =>
    request<ConversationRecord[]>(`/workspaces/${workspaceId}/conversations`),
  createConversation: (workspaceId: string, model?: string) =>
    request<ConversationRecord>(`/workspaces/${workspaceId}/conversations`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ title: "Cuộc trò chuyện mới", model: model || null }),
    }),
  conversation: (id: string) => request<ConversationDetail>(`/conversations/${id}`),
  chatConversation: (id: string, model: string, content: string) =>
    request<ConversationDetail>(`/conversations/${id}/chat`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model, content }),
    }),
  streamConversation: async (
    id: string,
    model: string,
    content: string,
    onDelta: (content: string) => void,
    signal: AbortSignal,
  ): Promise<ConversationDetail> => {
    const response = await fetch(`/api/v1/conversations/${id}/chat/stream`, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "text/event-stream" },
      body: JSON.stringify({ model, content }),
      signal,
    });
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new Error(body.detail ?? `Request failed with ${response.status}`);
    }
    if (!response.body) throw new Error("Streaming is not supported by this browser");

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let completed: ConversationDetail | undefined;
    const consume = (block: string) => {
      const data = block
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart())
        .join("\n");
      if (!data) return;
      const event = JSON.parse(data) as {
        type: "delta" | "done" | "error";
        content?: string;
        message?: string;
        conversation?: ConversationDetail;
      };
      if (event.type === "delta" && event.content) onDelta(event.content);
      if (event.type === "done" && event.conversation) completed = event.conversation;
      if (event.type === "error") throw new Error(event.message ?? "Chat stream failed");
    };

    while (true) {
      const { done, value } = await reader.read();
      buffer += decoder.decode(value, { stream: !done }).replaceAll("\r\n", "\n");
      const blocks = buffer.split("\n\n");
      buffer = blocks.pop() ?? "";
      blocks.forEach(consume);
      if (done) break;
    }
    if (buffer.trim()) consume(buffer);
    if (!completed) throw new Error("Chat stream ended before completion");
    return completed;
  },
  deleteConversation: (id: string) =>
    request<void>(`/conversations/${id}?confirmed=true`, { method: "DELETE" }),
  memories: () => request<MemoryRecord[]>("/memory?include_disabled=true"),
  createMemory: (type: MemoryType, content: string) =>
    request<MemoryRecord>("/memory", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ type, content, source: "user", confidence: 1 }),
    }),
  updateMemory: (id: string, type: MemoryType, content: string) =>
    request<MemoryRecord>(`/memory/${id}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ type, content, source: "user", confidence: 1 }),
    }),
  disableMemory: (id: string) =>
    request<MemoryRecord>(`/memory/${id}/disable`, { method: "POST" }),
  enableMemory: (id: string) =>
    request<MemoryRecord>(`/memory/${id}/enable`, { method: "POST" }),
  deleteMemory: (id: string) =>
    request<void>(`/memory/${id}?confirmed=true`, { method: "DELETE" }),
};
