import { Dialog } from "@kobalte/core/dialog";
import {
  BookOpenText,
  Boxes,
  BrainCircuit,
  ChevronDown,
  FileUp,
  HardDrive,
  MessageSquareText,
  Mic2,
  MoreHorizontal,
  Paperclip,
  Plus,
  RotateCw,
  Send,
  Settings2,
  ShieldCheck,
  Sparkles,
  Square,
  Trash2,
  Type,
  X,
} from "lucide-solid";
import {
  For,
  Index,
  Match,
  Show,
  Switch,
  createEffect,
  createMemo,
  createResource,
  createSignal,
  onCleanup,
} from "solid-js";
import { api } from "./api";
import { LibraryView, MemoryView, WorkspaceDialog } from "./components/DataViews";
import { Markdown } from "./components/Markdown";
import type {
  ChatMessage,
  ConversationDetail,
  ModelInfo,
  ServiceState,
  WorkspaceRecord,
} from "./types";

type View = "chat" | "library" | "models" | "memory" | "settings";
type Theme = "light" | "dark";
type FontScale = "normal" | "large";

const navigation = [
  { id: "chat" as const, label: "Trò chuyện", icon: MessageSquareText },
  { id: "library" as const, label: "Tài liệu", icon: BookOpenText },
  { id: "models" as const, label: "Mô hình", icon: Boxes },
  { id: "memory" as const, label: "Bộ nhớ", icon: BrainCircuit },
];

const starterPrompts = [
  "Tóm tắt các tài liệu mới trong thư viện",
  "Giúp tôi lên kế hoạch công việc hôm nay",
  "Tìm lại thông tin tôi đã lưu về dự án",
];

function getStoredPreference<T extends string>(key: string, allowed: T[], fallback: T): T {
  if (typeof window === "undefined") return fallback;
  const value = window.localStorage.getItem(key) as T | null;
  return value && allowed.includes(value) ? value : fallback;
}

const formatBytes = (bytes: number) => {
  if (!bytes) return "0 GB";
  const gib = bytes / 1024 ** 3;
  return `${gib < 10 ? gib.toFixed(1) : gib.toFixed(0)} GB`;
};

const formatRelativeTime = (value: string) => {
  const elapsed = Date.now() - new Date(value).getTime();
  const minutes = Math.max(0, Math.floor(elapsed / 60_000));
  if (minutes < 1) return "Bây giờ";
  if (minutes < 60) return `${minutes} phút`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} giờ`;
  return new Intl.DateTimeFormat("vi-VN", { day: "2-digit", month: "2-digit" }).format(new Date(value));
};

function StatusPip(props: { state: ServiceState | "idle" }) {
  return <span class={`status-pip status-${props.state}`} aria-hidden="true" />;
}

function ModelRow(props: { model: ModelInfo; onRefresh: () => void }) {
  const [working, setWorking] = createSignal(false);
  const [confirmDelete, setConfirmDelete] = createSignal(false);
  const [statusText, setStatusText] = createSignal("");
  const initials = () => props.model.name.split(/[/:_-]/).slice(0, 2).map((part) => part[0]).join("").toUpperCase();

  const load = async () => {
    setWorking(true);
    setStatusText("Đang nạp vào bộ nhớ…");
    try {
      await api.loadModel(props.model.name);
      props.onRefresh();
      setStatusText("");
    } catch (cause) {
      setStatusText(cause instanceof Error ? cause.message : "Không thể nạp mô hình");
    } finally {
      setWorking(false);
    }
  };

  const unload = async () => {
    setWorking(true);
    try {
      await api.unloadModel(props.model.name);
      props.onRefresh();
    } finally {
      setWorking(false);
    }
  };

  const remove = async () => {
    if (!confirmDelete()) {
      setConfirmDelete(true);
      return;
    }
    setWorking(true);
    try {
      await api.deleteModel(props.model.name);
      props.onRefresh();
    } finally {
      setWorking(false);
    }
  };

  const update = async () => {
    setWorking(true);
    setStatusText("Đang kiểm tra bản cập nhật…");
    try {
      if (props.model.model_type === "asr") {
        await api.updateModel(props.model.name);
      } else {
        await api.pullModel(props.model.name, setStatusText);
      }
      props.onRefresh();
      setStatusText("");
    } catch (cause) {
      setStatusText(cause instanceof Error ? cause.message : "Không thể cập nhật mô hình");
    } finally {
      setWorking(false);
    }
  };

  const selectVision = async () => {
    setWorking(true);
    setStatusText("Đang đặt làm mô hình OCR…");
    try {
      await api.setDefaultModel("vision", props.model.name);
      props.onRefresh();
      setStatusText("");
    } catch (cause) {
      setStatusText(cause instanceof Error ? cause.message : "Không thể chọn mô hình OCR");
    } finally {
      setWorking(false);
    }
  };

  return (
    <article class="model-row">
      <div class="model-glyph">{initials()}</div>
      <div class="model-identity"><strong>{props.model.name}</strong><span>{props.model.runtime} · {props.model.capabilities.join(" · ") || props.model.model_type}</span><Show when={props.model.default_for.length}><small>Mặc định: {props.model.default_for.join(", ")}</small></Show><Show when={statusText()}><small>{statusText()}</small></Show></div>
      <div class="model-metric"><span>Dung lượng</span><strong>{formatBytes(props.model.size_bytes)}</strong><Show when={props.model.sha256}><small>SHA {props.model.sha256?.slice(0, 12)}…</small></Show></div>
      <div class="model-state"><StatusPip state={props.model.state === "loaded" ? "online" : "idle"} />{props.model.state === "loaded" ? "Đang dùng" : "Sẵn sàng"}</div>
      <div class="model-actions">
        <Show when={props.model.model_type === "asr" && props.model.state === "unloaded"}><button disabled={working()} onClick={load}>Nạp</button></Show>
        <Show when={props.model.state === "loaded"}><button disabled={working()} onClick={unload}>Dỡ khỏi bộ nhớ</button></Show>
        <Show when={props.model.capabilities.includes("vision") && !props.model.default_for.includes("vision")}><button disabled={working()} onClick={selectVision}>Dùng cho OCR</button></Show>
        <button disabled={working()} onClick={update}>Cập nhật</button>
        <button classList={{ danger: confirmDelete() }} disabled={working()} onClick={remove}>{confirmDelete() ? "Xác nhận xóa" : "Xóa"}</button>
      </div>
    </article>
  );
}

function AddModelDialog(props: { onCompleted: () => void }) {
  const [open, setOpen] = createSignal(false);
  const [name, setName] = createSignal("");
  const [progress, setProgress] = createSignal("");
  const [error, setError] = createSignal("");
  const [controller, setController] = createSignal<AbortController>();

  const cancel = () => {
    controller()?.abort();
    setController(undefined);
    setProgress("");
  };

  const pull = async () => {
    if (!name().trim()) return;
    setError("");
    setProgress("Đang kết nối…");
    const nextController = new AbortController();
    setController(nextController);
    try {
      await api.pullModel(name().trim(), setProgress, nextController.signal);
      props.onCompleted();
      setOpen(false);
      setName("");
      setProgress("");
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === "AbortError") return;
      setError(cause instanceof Error ? cause.message : "Không thể tải mô hình");
      setProgress("");
    } finally {
      setController(undefined);
    }
  };

  return (
    <Dialog
      open={open()}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) cancel();
        setOpen(nextOpen);
      }}
    >
      <Dialog.Trigger class="button button-primary"><Plus size={18} /> Thêm mô hình</Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay class="dialog-overlay" />
        <div class="dialog-positioner">
          <Dialog.Content class="dialog-content">
            <div class="dialog-mark"><Sparkles size={22} /></div>
            <Dialog.Title>Tải mô hình Ollama</Dialog.Title>
            <Dialog.Description>Nhập tên trong thư viện Ollama, ví dụ qwen3:8b. Bạn có thể theo dõi tiến trình ngay tại đây.</Dialog.Description>
            <label class="field-label" for="model-name">Tên mô hình</label>
            <input id="model-name" class="text-input" placeholder="qwen3:8b" autocomplete="off" value={name()} onInput={(event) => setName(event.currentTarget.value)} />
            <Show when={progress()}><p class="field-status">{progress()}</p></Show>
            <Show when={error()}><p class="field-error">{error()}</p></Show>
            <div class="dialog-actions">
              <Dialog.CloseButton class="button button-secondary" onClick={cancel}>Hủy</Dialog.CloseButton>
              <button class="button button-primary" type="button" disabled={!!progress()} onClick={pull}>{progress() ? "Đang tải" : "Bắt đầu tải"}</button>
            </div>
            <Dialog.CloseButton class="icon-button dialog-close" aria-label="Đóng hộp thoại"><X size={20} /></Dialog.CloseButton>
          </Dialog.Content>
        </div>
      </Dialog.Portal>
    </Dialog>
  );
}

function App() {
  const [view, setView] = createSignal<View>("chat");
  const [theme, setTheme] = createSignal<Theme>(getStoredPreference("private-ai-theme", ["light", "dark"], "light"));
  const [fontScale, setFontScale] = createSignal<FontScale>(getStoredPreference("private-ai-font-scale", ["normal", "large"], "normal"));
  const [activeWorkspace, setActiveWorkspace] = createSignal("");
  const [activeConversation, setActiveConversation] = createSignal("");
  const [confirmConversationDelete, setConfirmConversationDelete] = createSignal(false);
  const [confirmWorkspaceDelete, setConfirmWorkspaceDelete] = createSignal("");
  const [sidebarOpen, setSidebarOpen] = createSignal(false);
  const [messages, setMessages] = createSignal<ChatMessage[]>([]);
  const [draft, setDraft] = createSignal("");
  const [selectedModel, setSelectedModel] = createSignal("");
  const [sending, setSending] = createSignal(false);
  const [chatError, setChatError] = createSignal("");
  const [uploading, setUploading] = createSignal(false);
  const [uploadError, setUploadError] = createSignal("");
  const [recording, setRecording] = createSignal(false);
  const [transcribing, setTranscribing] = createSignal(false);
  const [health, { refetch: refetchHealth }] = createResource(api.health);
  const [models, { refetch: refetchModels }] = createResource(api.models);
  const [workspaceList, { refetch: refetchWorkspaces }] = createResource(api.workspaces);
  // createResource only skips a fetch for false/null/undefined, so an empty id would be
  // sent as a real request and leave both resources stuck in an error state.
  const workspaceSource = createMemo(() => activeWorkspace() || undefined);
  const [conversations, { refetch: refetchConversations }] = createResource(
    workspaceSource,
    api.conversations,
  );
  const [documents, { refetch: refetchDocuments }] = createResource(
    workspaceSource,
    api.documents,
  );
  let fileInput!: HTMLInputElement;
  let messageList!: HTMLDivElement;
  let activeChatController: AbortController | undefined;
  let mediaRecorder: MediaRecorder | undefined;
  let mediaStream: MediaStream | undefined;
  let audioSocket: WebSocket | undefined;
  let audioContext: AudioContext | undefined;
  let audioWorklet: AudioWorkletNode | undefined;
  let audioSource: MediaStreamAudioSourceNode | undefined;
  let audioMute: GainNode | undefined;
  let voiceMode: "native" | "batch" | undefined;
  let voiceDraftBase = "";

  const currentWorkspace = createMemo<WorkspaceRecord>(() =>
    workspaceList()?.find((workspace) => workspace.id === activeWorkspace()) ?? {
      id: activeWorkspace(),
      name: "Không gian làm việc",
      description: "",
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      conversation_count: 0,
    },
  );
  const hasWorkspace = createMemo(() =>
    Boolean(workspaceList()?.some((workspace) => workspace.id === activeWorkspace())),
  );
  const chatModels = createMemo(() =>
    (models() ?? []).filter((model) => model.model_type === "language"),
  );
  const serviceState = (name: string): ServiceState => health()?.services[name] ?? "offline";
  const vramPercent = createMemo(() => {
    const gpu = health()?.gpu;
    if (!gpu?.capacity_bytes) return 0;
    const ratio = gpu.reserved_bytes / gpu.capacity_bytes;
    return Math.min(100, Math.max(0, Math.round(ratio * 100)));
  });
  const vramLabel = createMemo(() => {
    const gpu = health()?.gpu;
    if (!gpu) return "Đang đo…";
    return `${formatBytes(gpu.reserved_bytes)} / ${formatBytes(gpu.capacity_bytes)}`;
  });
  const vramDetail = createMemo(() => {
    const count = health()?.gpu.leases?.length ?? 0;
    if (!count) return "Không có mô hình trong GPU";
    return `${count} mô hình đang dùng VRAM`;
  });

  const healthPoll = window.setInterval(() => {
    if (document.visibilityState === "visible") void refetchHealth();
  }, 5_000);
  const refreshVisibleHealth = () => {
    if (document.visibilityState === "visible") void refetchHealth();
  };
  document.addEventListener("visibilitychange", refreshVisibleHealth);
  onCleanup(() => {
    window.clearInterval(healthPoll);
    document.removeEventListener("visibilitychange", refreshVisibleHealth);
  });

  createEffect(() => {
    document.documentElement.dataset.theme = theme();
    document.documentElement.dataset.fontScale = fontScale();
    document.querySelector<HTMLMetaElement>("#theme-color")?.setAttribute(
      "content",
      theme() === "light" ? "#f3f6f4" : "#0b1412",
    );
    window.localStorage.setItem("private-ai-theme", theme());
    window.localStorage.setItem("private-ai-font-scale", fontScale());
  });

  createEffect(() => {
    const available = chatModels();
    const preferred = available.find((model) => model.default_for.includes("chat")) ?? available[0];
    if (preferred && !selectedModel()) setSelectedModel(preferred.name);
  });

  const chooseChatModel = (name: string) => {
    setSelectedModel(name);
    if (name) void api.setDefaultModel("chat", name).then(() => refetchModels());
  };

  const refresh = () => Promise.all([
    refetchHealth(),
    refetchModels(),
    refetchWorkspaces(),
    refetchConversations(),
    refetchDocuments(),
  ]);

  const openConversation = async (id: string) => {
    setActiveConversation(id);
    setConfirmConversationDelete(false);
    setChatError("");
    try {
      const detail = await api.conversation(id);
      setMessages(detail.messages);
      if (detail.model) setSelectedModel(detail.model);
    } catch (cause) {
      setChatError(cause instanceof Error ? cause.message : "Không thể mở cuộc trò chuyện");
    }
  };

  const chooseWorkspace = (id: string) => {
    setActiveWorkspace(id);
    setConfirmWorkspaceDelete("");
    refetchDocuments();
    setActiveConversation("");
    setView("chat");
    setMessages([]);
    setChatError("");
    setSidebarOpen(false);
  };

  const newConversation = async () => {
    if (!activeWorkspace()) {
      setChatError("Hãy tạo một không gian làm việc trước.");
      return;
    }
    setView("chat");
    setMessages([]);
    setDraft("");
    setChatError("");
    setSidebarOpen(false);
    try {
      const conversation = await api.createConversation(activeWorkspace(), selectedModel());
      setActiveConversation(conversation.id);
      refetchConversations();
      refetchWorkspaces();
    } catch (cause) {
      setChatError(cause instanceof Error ? cause.message : "Không thể tạo cuộc trò chuyện");
    }
  };

  const handleWorkspaceSaved = (workspace: WorkspaceRecord) => {
    setActiveWorkspace(workspace.id);
    setActiveConversation("");
    setMessages([]);
    setView("chat");
    refetchWorkspaces();
  };

  const handleWorkspaceDeleted = async (id: string) => {
    setConfirmWorkspaceDelete("");
    const remaining = await refetchWorkspaces();
    if (activeWorkspace() !== id) return;
    setActiveConversation("");
    setMessages([]);
    setActiveWorkspace(remaining?.find((workspace) => workspace.id !== id)?.id ?? "");
  };

  const deleteWorkspace = async (id: string) => {
    if (confirmWorkspaceDelete() !== id) {
      setConfirmWorkspaceDelete(id);
      return;
    }
    setChatError("");
    try {
      await api.deleteWorkspace(id);
      await handleWorkspaceDeleted(id);
    } catch (cause) {
      setConfirmWorkspaceDelete("");
      setChatError(
        cause instanceof Error ? cause.message : "Không thể xóa không gian làm việc",
      );
    }
  };

  const deleteCurrentConversation = async () => {
    const id = activeConversation();
    if (!id) return;
    if (!confirmConversationDelete()) {
      setConfirmConversationDelete(true);
      return;
    }
    await api.deleteConversation(id);
    setConfirmConversationDelete(false);
    setActiveConversation("");
    setMessages([]);
    refetchConversations();
    refetchWorkspaces();
  };

  createEffect(() => {
    const items = workspaceList();
    if (!items || items.some((workspace) => workspace.id === activeWorkspace())) return;
    setActiveWorkspace(items[0]?.id ?? "");
  });

  createEffect(() => {
    const items = conversations();
    if (!items || activeConversation() || items.length === 0) return;
    void openConversation(items[0].id);
  });

  const submitMessage = async (content = draft()) => {
    const text = content.trim();
    if (!text || sending()) return;
    if (!selectedModel()) {
      setChatError("Hãy cài hoặc chọn một mô hình trước khi gửi tin nhắn.");
      return;
    }
    if (!activeWorkspace()) {
      setChatError("Hãy tạo một không gian làm việc trước.");
      return;
    }
    let conversationId = activeConversation();
    if (!conversationId) {
      try {
        const conversation = await api.createConversation(activeWorkspace(), selectedModel());
        conversationId = conversation.id;
        setActiveConversation(conversation.id);
      } catch (cause) {
        setChatError(cause instanceof Error ? cause.message : "Không thể tạo cuộc trò chuyện");
        return;
      }
    }
    const nextMessages: ChatMessage[] = [...messages(), { role: "user", content: text }];
    let streamedAnswer = "";
    setMessages([...nextMessages, { role: "assistant", content: "" }]);
    setDraft("");
    setChatError("");
    setSending(true);
    const controller = new AbortController();
    let renderFrame: number | undefined;
    activeChatController = controller;
    queueMicrotask(() => messageList?.scrollTo({ top: messageList.scrollHeight, behavior: "smooth" }));
    try {
      const response: ConversationDetail = await api.streamConversation(
        conversationId,
        selectedModel(),
        text,
        (content) => {
          streamedAnswer += content;
          if (renderFrame === undefined) {
            renderFrame = window.requestAnimationFrame(() => {
              setMessages([...nextMessages, { role: "assistant", content: streamedAnswer }]);
              messageList?.scrollTo({ top: messageList.scrollHeight });
              renderFrame = undefined;
            });
          }
        },
        controller.signal,
      );
      if (renderFrame !== undefined) window.cancelAnimationFrame(renderFrame);
      renderFrame = undefined;
      setMessages(response.messages);
      refetchConversations();
      refetchWorkspaces();
      queueMicrotask(() => messageList?.scrollTo({ top: messageList.scrollHeight, behavior: "smooth" }));
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === "AbortError") {
        setMessages(streamedAnswer
          ? [...nextMessages, { role: "assistant", content: streamedAnswer }]
          : nextMessages);
      } else {
        setMessages(nextMessages);
        setChatError(cause instanceof Error ? cause.message : "Không thể gửi tin nhắn");
      }
    } finally {
      if (renderFrame !== undefined) window.cancelAnimationFrame(renderFrame);
      if (activeChatController === controller) activeChatController = undefined;
      setSending(false);
      refetchHealth();
      refetchConversations();
      refetchWorkspaces();
    }
  };

  const stopGeneration = () => activeChatController?.abort();

  const stopRecording = () => {
    if (voiceMode === "native" && audioWorklet) {
      setRecording(false);
      setTranscribing(true);
      audioWorklet.port.postMessage({ type: "flush" });
    } else if (mediaRecorder?.state === "recording") {
      mediaRecorder.stop();
    }
  };

  const finishVoiceSession = () => {
    setRecording(false);
    setTranscribing(false);
    audioSource?.disconnect();
    audioWorklet?.disconnect();
    audioMute?.disconnect();
    if (audioContext?.state !== "closed") void audioContext?.close();
    mediaStream?.getTracks().forEach((track) => track.stop());
    audioSource = undefined;
    audioWorklet = undefined;
    audioMute = undefined;
    audioContext = undefined;
    mediaStream = undefined;
    mediaRecorder = undefined;
    audioSocket = undefined;
    voiceMode = undefined;
    voiceDraftBase = "";
  };

  const voiceText = (text: string) =>
    `${voiceDraftBase}${voiceDraftBase && text ? " " : ""}${text}`;

  const startBatchCapture = () => {
    if (!mediaStream || !audioSocket || typeof MediaRecorder === "undefined") {
      throw new Error("Trình duyệt không hỗ trợ thu âm");
    }
    const preferred = ["audio/webm;codecs=opus", "audio/webm", "audio/mp4"].find(
      (type) => MediaRecorder.isTypeSupported(type),
    );
    mediaRecorder = new MediaRecorder(mediaStream, preferred ? { mimeType: preferred } : {});
    voiceMode = "batch";
    mediaRecorder.ondataavailable = (event) => {
      if (event.data.size && audioSocket?.readyState === WebSocket.OPEN) {
        audioSocket.send(event.data);
      }
    };
    mediaRecorder.onstop = () => {
      setRecording(false);
      setTranscribing(true);
      if (audioSocket?.readyState === WebSocket.OPEN) {
        audioSocket.send(JSON.stringify({ type: "commit" }));
      }
    };
    const mimeType = mediaRecorder.mimeType || "audio/webm";
    const extension = mimeType.includes("mp4") ? "m4a" : "webm";
    audioSocket.send(JSON.stringify({
      type: "config",
      language: "vi-VN",
      filename: `recording.${extension}`,
      format: "media",
    }));
    mediaRecorder.start(250);
    setRecording(true);
  };

  const startNativeCapture = async () => {
    if (!mediaStream || !audioSocket || !window.AudioContext) {
      throw new Error("AudioWorklet không được hỗ trợ");
    }
    const context = new AudioContext();
    try {
      await context.audioWorklet.addModule("/pcm-worklet.js");
      const source = context.createMediaStreamSource(mediaStream);
      const worklet = new AudioWorkletNode(context, "private-ai-pcm-16k", {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
      });
      const mute = context.createGain();
      mute.gain.value = 0;
      worklet.port.onmessage = (event: MessageEvent<ArrayBuffer | { type?: string }>) => {
        if (event.data instanceof ArrayBuffer) {
          if (event.data.byteLength && audioSocket?.readyState === WebSocket.OPEN) {
            audioSocket.send(event.data);
          }
        } else if (event.data?.type === "flushed") {
          audioSource?.disconnect();
          audioWorklet?.disconnect();
          audioMute?.disconnect();
          if (audioSocket?.readyState === WebSocket.OPEN) {
            audioSocket.send(JSON.stringify({ type: "commit" }));
          }
        }
      };
      audioSocket.send(JSON.stringify({
        type: "config",
        language: "vi-VN",
        format: "f32le",
        sample_rate: 16000,
      }));
      source.connect(worklet);
      worklet.connect(mute);
      mute.connect(context.destination);
      await context.resume();
      audioContext = context;
      audioSource = source;
      audioWorklet = worklet;
      audioMute = mute;
      voiceMode = "native";
      setRecording(true);
    } catch (cause) {
      await context.close();
      throw cause;
    }
  };

  const beginVoiceCapture = async (nativeStreaming: boolean) => {
    try {
      if (nativeStreaming && "AudioWorkletNode" in window) {
        try {
          await startNativeCapture();
          return;
        } catch {
          // Fall through to encoded MediaRecorder chunks for older webviews.
        }
      }
      startBatchCapture();
    } catch (cause) {
      setChatError(cause instanceof Error ? cause.message : "Không thể bắt đầu thu âm");
      audioSocket?.close();
    }
  };

  const toggleRecording = async () => {
    if (recording()) {
      stopRecording();
      return;
    }
    if (serviceState("asr") !== "online") {
      setChatError("ASR chưa sẵn sàng. Chạy private-ai-asr setup rồi khởi động lại API.");
      return;
    }
    try {
      voiceDraftBase = draft().trimEnd();
      mediaStream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      const websocketProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      audioSocket = new WebSocket(
        `${websocketProtocol}//${window.location.host}/api/v1/asr/stream`,
      );
      audioSocket.onmessage = (event) => {
        const result = JSON.parse(event.data) as {
          type: "ready" | "configured" | "progress" | "partial" | "final" | "error";
          text?: string;
          display?: string;
          message?: string;
          streaming?: boolean;
        };
        if (result.type === "ready") {
          void beginVoiceCapture(Boolean(result.streaming));
        } else if (result.type === "partial") {
          setDraft(voiceText(result.display ?? ""));
          setChatError("");
        } else if (result.type === "final" && result.text) {
          setDraft(voiceText(result.text));
          setChatError("");
        } else if (result.type === "error") {
          setChatError(result.message ?? "Không thể nhận dạng giọng nói");
        }
      };
      audioSocket.onerror = () => {
        setChatError("Không thể kết nối dịch vụ nhận dạng giọng nói");
        stopRecording();
      };
      audioSocket.onclose = finishVoiceSession;
      setChatError("");
    } catch (cause) {
      mediaStream?.getTracks().forEach((track) => track.stop());
      audioSocket?.close();
      setChatError(cause instanceof Error ? cause.message : "Không thể mở microphone");
    }
  };

  onCleanup(() => {
    activeChatController?.abort();
    if (audioSocket?.readyState === WebSocket.OPEN) {
      audioSocket.send(JSON.stringify({ type: "cancel" }));
    }
    audioSocket?.close();
    finishVoiceSession();
  });

  const handleComposerKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submitMessage();
    }
  };
  const handleUpload = async (event: Event) => {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const workspaceId = activeWorkspace();
    if (!workspaceId) {
      setUploadError("Hãy tạo một không gian làm việc trước khi thêm tài liệu.");
      input.value = "";
      return;
    }
    setUploading(true);
    setUploadError("");
    try {
      // Stay on the current view; progress shows in the context rail instead.
      await api.uploadDocument(file, workspaceId);
      refetchDocuments();
      window.setTimeout(() => refetchDocuments(), 800);
    } catch (cause) {
      setUploadError(cause instanceof Error ? cause.message : "Không thể thêm tài liệu");
    } finally {
      setUploading(false);
      input.value = "";
    }
  };

  return (
    <>
      <a class="skip-link" href="#main-content">Bỏ qua thanh điều hướng</a>
      <div class="app-shell">
        <aside classList={{ sidebar: true, open: sidebarOpen() }} aria-label="Không gian làm việc">
          <div class="sidebar-header">
            <button class="brand" onClick={() => setView("chat")} aria-label="Private AI">
              <span class="brand-mark"><i /><i /><i /></span>
              <span><strong>PRIVATE</strong><em>AI</em></span>
            </button>
            <button class="icon-button mobile-close" onClick={() => setSidebarOpen(false)} aria-label="Đóng menu"><X size={21} /></button>
          </div>
          <button class="new-chat-button" onClick={newConversation}><Plus size={20} /> Cuộc trò chuyện mới</button>
          <nav class="primary-nav" aria-label="Điều hướng chính">
            <For each={navigation}>{(item) => (
              <button classList={{ "nav-item": true, active: view() === item.id }} onClick={() => { setView(item.id); setSidebarOpen(false); }} aria-current={view() === item.id ? "page" : undefined}>
                <item.icon size={20} stroke-width={1.8} /><span>{item.label}</span>
              </button>
            )}</For>
          </nav>
          <div class="workspace-section">
            <div class="section-label">
              <span>Không gian của bạn</span>
              <WorkspaceDialog trigger="add" onSaved={handleWorkspaceSaved} />
            </div>
            <div class="workspace-list">
              <Show when={!workspaceList.loading && (workspaceList()?.length ?? 0) === 0}>
                <p class="empty-conversations">Chưa có không gian làm việc</p>
              </Show>
              <For each={workspaceList()}>{(workspace) => (
                <div classList={{ "workspace-row": true, active: activeWorkspace() === workspace.id }}>
                  <button class="workspace-item" onClick={() => chooseWorkspace(workspace.id)}>
                    <span class="workspace-dot" aria-hidden="true" />
                    <span class="workspace-copy"><strong>{workspace.name}</strong><small>{workspace.description}</small></span>
                    <time>{formatRelativeTime(workspace.updated_at)}</time>
                  </button>
                  <button classList={{ "workspace-delete": true, danger: confirmWorkspaceDelete() === workspace.id }} onClick={() => void deleteWorkspace(workspace.id)} aria-label={confirmWorkspaceDelete() === workspace.id ? `Bấm lại để xác nhận xóa ${workspace.name} và toàn bộ cuộc trò chuyện bên trong` : `Xóa không gian làm việc ${workspace.name}`}><Trash2 size={15} /></button>
                </div>
              )}</For>
            </div>
            <div class="section-label conversation-label"><span>Cuộc trò chuyện</span></div>
            <div class="conversation-list">
              <Show when={!conversations.loading && (conversations()?.length ?? 0) === 0}>
                <p class="empty-conversations">Chưa có cuộc trò chuyện</p>
              </Show>
              <For each={conversations()}>{(conversation) => (
                <div classList={{ "conversation-item": true, active: activeConversation() === conversation.id }}>
                  <button onClick={() => void openConversation(conversation.id)}>
                    <MessageSquareText size={16} />
                    <span><strong>{conversation.title}</strong><small>{conversation.message_count} tin nhắn · {formatRelativeTime(conversation.updated_at)}</small></span>
                  </button>
                  <Show when={activeConversation() === conversation.id}>
                    <button classList={{ "conversation-delete": true, danger: confirmConversationDelete() }} onClick={() => void deleteCurrentConversation()} aria-label={confirmConversationDelete() ? "Bấm lại để xác nhận xóa" : "Xóa cuộc trò chuyện"}><X size={15} /></button>
                  </Show>
                </div>
              )}</For>
            </div>
          </div>
          <div class="sidebar-preferences">
            <div class="preference-row">
              <span>Giao diện</span>
              <div class="segmented-control" aria-label="Chọn giao diện">
                <button classList={{ active: theme() === "light" }} onClick={() => setTheme("light")}>Sáng</button>
                <button classList={{ active: theme() === "dark" }} onClick={() => setTheme("dark")}>Tối</button>
              </div>
            </div>
            <button classList={{ "font-control": true, active: fontScale() === "large" }} onClick={() => setFontScale(fontScale() === "large" ? "normal" : "large")} aria-pressed={fontScale() === "large"}>
              <Type size={19} /> Chữ lớn <span>{fontScale() === "large" ? "Bật" : "Tắt"}</span>
            </button>
            <button classList={{ "settings-link": true, active: view() === "settings" }} onClick={() => setView("settings")}><Settings2 size={19} /> Cài đặt</button>
          </div>
        </aside>

        <main class="main-stage" id="main-content">
          <header class="topbar">
            <div class="topbar-title">
              <button class="icon-button mobile-menu" onClick={() => setSidebarOpen(true)} aria-label="Mở menu"><MoreHorizontal size={22} /></button>
              <div><strong>{view() === "chat" ? (hasWorkspace() ? currentWorkspace().name : "Chưa có không gian") : view() === "settings" ? "Cài đặt" : navigation.find((item) => item.id === view())?.label}</strong><span><StatusPip state={health()?.status === "ok" ? "online" : "offline"} /> Dữ liệu được xử lý trên máy</span></div>
            </div>
            <div class="topbar-actions">
              <label class="model-select-label">
                <span>Mô hình</span>
                <select value={selectedModel()} onChange={(event) => chooseChatModel(event.currentTarget.value)}>
                  <Show when={!models.loading && chatModels().length === 0}><option value="">Chưa có mô hình</option></Show>
                  <For each={chatModels()}>{(model) => <option value={model.name}>{model.name}</option>}</For>
                </select>
                <ChevronDown size={16} aria-hidden="true" />
              </label>
              <button class="icon-button" onClick={refresh} aria-label="Làm mới trạng thái"><RotateCw size={19} /></button>
              <div class="avatar" aria-label="Tài khoản của Vinh">VP</div>
            </div>
          </header>

          <Switch>
            <Match when={view() === "chat"}>
              <div class="chat-workspace">
                <section class="conversation" aria-label="Cuộc trò chuyện">
                  <div class="message-list" ref={messageList} aria-live="polite">
                    <Show when={messages().length > 0} fallback={
                      <div class="chat-welcome">
                        <div class="welcome-mark"><Sparkles size={28} /></div>
                        <p>Chào bạn, Vinh</p>
                        <h1>Hôm nay bạn muốn làm gì?</h1>
                        <span>Hỏi bằng ngôn ngữ tự nhiên. Private AI sẽ dùng mô hình và tài liệu trên máy để trả lời.</span>
                        <div class="starter-prompts"><For each={starterPrompts}>{(prompt) => <button onClick={() => void submitMessage(prompt)}>{prompt}<Send size={17} /></button>}</For></div>
                      </div>
                    }>
                      <div class="message-stream">
                        <Index each={messages()}>{(message) => (
                          <article class={`message message-${message().role}`}>
                            <div class="message-author"><span>{message().role === "user" ? "VP" : "AI"}</span><strong>{message().role === "user" ? "Bạn" : "Private AI"}</strong></div>
                            <Show
                              when={message().content}
                              fallback={<div class="thinking"><i /><i /><i /><span>Đang suy nghĩ</span></div>}
                            >
                              <Markdown content={message().content} />
                            </Show>
                          </article>
                        )}</Index>
                      </div>
                    </Show>
                  </div>
                  <div class="composer-wrap">
                    <Show when={chatError()}><div class="inline-error" role="alert">{chatError()}</div></Show>
                    <form class="composer" onSubmit={(event) => { event.preventDefault(); void submitMessage(); }}>
                      <textarea value={draft()} onInput={(event) => setDraft(event.currentTarget.value)} onKeyDown={handleComposerKeyDown} placeholder="Nhập câu hỏi cho Private AI…" aria-label="Tin nhắn" rows={2} />
                      <div class="composer-tools">
                        <div>
                          <button type="button" onClick={() => fileInput.click()} aria-label="Đính kèm tài liệu"><Paperclip size={20} /><span>Đính kèm</span></button>
                          <button
                            type="button"
                            classList={{ "recording-button": recording() }}
                            disabled={sending() || transcribing()}
                            onClick={() => void toggleRecording()}
                            aria-label={recording() ? "Dừng ghi âm" : "Nhập bằng giọng nói"}
                            aria-pressed={recording()}
                          >
                            {recording() ? <Square size={17} fill="currentColor" /> : <Mic2 size={20} />}
                            <span>{recording() ? "Dừng ghi" : transcribing() ? "Đang nhận dạng" : "Giọng nói"}</span>
                          </button>
                        </div>
                        <Show
                          when={sending()}
                          fallback={<button class="send-button" type="submit" disabled={!draft().trim()} aria-label="Gửi tin nhắn"><Send size={21} /></button>}
                        >
                          <button class="send-button stop-button" type="button" onClick={stopGeneration} aria-label="Dừng trả lời"><Square size={17} fill="currentColor" /></button>
                        </Show>
                      </div>
                    </form>
                    <p>Enter để gửi · Shift + Enter để xuống dòng</p>
                  </div>
                </section>

                <aside class="context-rail" aria-label="Ngữ cảnh không gian làm việc">
                  <div class="context-heading">
                    <div><span>Ngữ cảnh</span><strong>{hasWorkspace() ? currentWorkspace().name : "Chưa có không gian"}</strong></div>
                    <Show when={hasWorkspace()}>
                      <WorkspaceDialog workspace={currentWorkspace()} trigger="edit" onSaved={handleWorkspaceSaved} onDeleted={handleWorkspaceDeleted} />
                    </Show>
                  </div>
                  <section class="context-block">
                    <h2>Tài liệu trong thư viện</h2>
                    <Show when={uploadError() || documents.error}><div class="inline-error" role="alert">{uploadError() || (documents.error as Error)?.message}</div></Show>
                    <Show when={uploading()}><div class="context-loading"><i />Đang nhập tài liệu…</div></Show>
                    <Show when={(documents()?.length ?? 0) > 0} fallback={<Show when={!uploading()}><button class="empty-context" onClick={() => fileInput.click()}><FileUp size={22} /><span><strong>Thêm tài liệu</strong><small>PDF, Office hoặc Markdown</small></span></button></Show>}>
                      <div class="context-documents"><For each={documents()?.slice(0, 3)}>{(document) => (
                        <button onClick={() => setView("library")}><BookOpenText size={17} /><span><strong>{document.filename}</strong><small>{document.status === "ready" ? "Sẵn sàng" : document.status}</small></span></button>
                      )}</For></div>
                    </Show>
                  </section>
                  <section class="context-block"><h2>Trạng thái hệ thống</h2><dl class="service-list">
                    <div><dt><StatusPip state={serviceState("ollama")} /> Ollama</dt><dd>{serviceState("ollama") === "online" ? "Sẵn sàng" : "Ngoại tuyến"}</dd></div>
                    <div><dt><StatusPip state={serviceState("neo4j")} /> Kho tri thức</dt><dd>{serviceState("neo4j") === "online" ? "Sẵn sàng" : "Chưa cấu hình"}</dd></div>
                    <div><dt><StatusPip state={serviceState("asr")} /> Giọng nói</dt><dd>{serviceState("asr") === "online" ? "Sẵn sàng" : "Chưa cấu hình"}</dd></div>
                  </dl></section>
                  <section class="context-block resource-block">
                    <div><h2>VRAM đang dùng</h2><span>{vramLabel()}</span></div>
                    <div
                      class="resource-track"
                      role="progressbar"
                      aria-label="Mức sử dụng VRAM"
                      aria-valuemin="0"
                      aria-valuemax="100"
                      aria-valuenow={vramPercent()}
                    ><i style={{ transform: `scaleX(${vramPercent() / 100})` }} /></div>
                    <small>{vramDetail()} · tự cập nhật mỗi 5 giây</small>
                  </section>
                  <div class="local-note"><ShieldCheck size={22} /><div><strong>Riêng tư trên thiết bị</strong><span>Nội dung trò chuyện không rời khỏi máy này.</span></div></div>
                </aside>
              </div>
            </Match>

            <Match when={view() === "library"}>
              <LibraryView documents={documents()} uploadError={uploadError()} workspaceName={hasWorkspace() ? currentWorkspace().name : "Chưa có không gian"} loading={documents.loading} uploading={uploading()} onUpload={() => fileInput.click()} onRefresh={refetchDocuments} />
            </Match>
            <Match when={view() === "models"}>
              <section class="page-view"><div class="page-heading page-heading-row"><div><span>Mô hình cục bộ</span><h1>Quản lý mô hình</h1><p>Quản lý Ollama và ASR: trạng thái tải, VRAM, mặc định tác vụ và kiểm tra SHA-256.</p></div><AddModelDialog onCompleted={() => { refetchModels(); refetchHealth(); }} /></div><div class="model-list"><Switch><Match when={models.loading}><div class="loading-row"><i />Đang đọc thư viện mô hình…</div></Match><Match when={models.error || (models()?.length ?? 0) === 0}><div class="empty-models"><HardDrive size={28} /><strong>Chưa tìm thấy mô hình</strong><span>Khởi động Ollama rồi thêm mô hình đầu tiên.</span></div></Match><Match when={(models()?.length ?? 0) > 0}><For each={models()}>{(model) => <ModelRow model={model} onRefresh={() => { refetchModels(); refetchHealth(); }} />}</For></Match></Switch></div></section>
            </Match>
            <Match when={view() === "memory"}>
              <MemoryView />
            </Match>
            <Match when={view() === "settings"}>
              <section class="page-view settings-page">
                <div class="page-heading"><span>Cài đặt thiết bị</span><h1>Hiển thị và trải nghiệm</h1><p>Các lựa chọn này chỉ được lưu trên máy hiện tại.</p></div>
                <div class="settings-sections">
                  <section><div><strong>Giao diện</strong><span>Chọn nền sáng dễ đọc hoặc nền tối.</span></div><div class="segmented-control settings-control"><button classList={{ active: theme() === "light" }} onClick={() => setTheme("light")}>Sáng</button><button classList={{ active: theme() === "dark" }} onClick={() => setTheme("dark")}>Tối</button></div></section>
                  <section><div><strong>Cỡ chữ</strong><span>Tăng toàn bộ chữ và vùng điều khiển.</span></div><button classList={{ "button": true, "button-secondary": true, active: fontScale() === "large" }} onClick={() => setFontScale(fontScale() === "large" ? "normal" : "large")}><Type size={18} />{fontScale() === "large" ? "Đang dùng chữ lớn" : "Bật chữ lớn"}</button></section>
                </div>
              </section>
            </Match>
          </Switch>
        </main>
        <Show when={sidebarOpen()}><button class="sidebar-scrim" onClick={() => setSidebarOpen(false)} aria-label="Đóng menu" /></Show>
      </div>
      <input ref={fileInput} class="sr-only" type="file" onChange={handleUpload} />
    </>
  );
}

export default App;
