import { Dialog } from "@kobalte/core/dialog";
import {
  BookOpenText,
  Boxes,
  BrainCircuit,
  ChevronDown,
  Clipboard,
  FileUp,
  Globe,
  HardDrive,
  LayoutGrid,
  Menu,
  MessageSquareText,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Paperclip,
  Plus,
  RefreshCw,
  Send,
  Settings2,
  ShieldCheck,
  Sparkles,
  Square,
  Trash2,
  Type,
  Waypoints,
  X,
} from "lucide-solid";
import {
  For,
  Index,
  Match,
  Show,
  Suspense,
  Switch,
  createEffect,
  createMemo,
  createResource,
  createSignal,
  lazy,
  onCleanup,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { api } from "./api";
import { formatBytes, formatRelativeTime } from "./format";
import { DocumentViewer, LibraryView, MemoryView, WorkspaceDialog } from "./components/DataViews";
import { WorkspacesView } from "./components/WorkspacesView";
import { ProfileNameDialog, ProfileSwitcher, initialsOf } from "./components/Profiles";
import { UploadDialog } from "./components/UploadDialog";
import { notify, ToastViewport } from "./components/AppToast";
import { ProviderSettings } from "./components/Providers";
import { Markdown } from "./components/Markdown";
import { ModelPicker } from "./components/ModelPicker";
import { NotificationsMenu } from "./components/Notifications";
import type { Notice } from "./components/Notifications";
import type {
  ChatMessage,
  ConversationDetail,
  DocumentRecord,
  ModelInfo,
  Preferences,
  PreferencesUpdate,
  RagMode,
  ServiceState,
  WebSearchBackend,
  WebSearchProbeResult,
  WorkspaceRecord,
} from "./types";

// Cytoscape chỉ cần cho màn hình Tri thức, nên nó nằm ngoài bundle khởi động.
const GraphView = lazy(() => import("./components/GraphView"));

type View = "chat" | "workspaces" | "library" | "graph" | "settings";
type SettingsTab = "general" | "models" | "memory" | "providers";
const DOCUMENTS_PER_PAGE = 20;

type Theme = "light" | "dark";
type FontScale = "normal" | "large";

const navigation = [
  { id: "chat" as const, label: "Trò chuyện", icon: MessageSquareText },
  { id: "workspaces" as const, label: "Không gian", icon: LayoutGrid },
  { id: "library" as const, label: "Tài liệu", icon: BookOpenText },
  { id: "graph" as const, label: "Tri thức", icon: Waypoints },
];

// Cấu hình nâng cao gom hết vào trang Cài đặt thay vì chiếm chỗ ở thanh bên.
const settingsTabs = [
  { id: "general" as const, label: "Chung", icon: Settings2 },
  { id: "models" as const, label: "Mô hình", icon: Boxes },
  { id: "memory" as const, label: "Bộ nhớ", icon: BrainCircuit },
  { id: "providers" as const, label: "Nhà cung cấp", icon: HardDrive },
];

// Nguồn tìm kiếm xếp từ riêng tư nhất tới ít riêng tư nhất.
const webSearchBackends = [
  {
    id: "searxng" as const,
    label: "SearXNG",
    hint: "Máy chủ meta-search bạn tự dựng. Không cần API key, câu hỏi chỉ đi tới máy chủ của bạn.",
  },
  {
    id: "duckduckgo" as const,
    label: "DuckDuckGo",
    hint: "Chạy được ngay, không cần cấu hình. DuckDuckGo không có API chính thức nên có thể bị chặn tạm thời khi hỏi quá nhiều.",
  },
  {
    id: "openai" as const,
    label: "OpenAI web search",
    hint: "Chất lượng cao nhất, có trả phí theo lượt tìm. Câu hỏi và API key được gửi tới api.openai.com.",
  },
];

function webSearchBackendLabel(backend: WebSearchBackend) {
  return webSearchBackends.find((item) => item.id === backend)?.label ?? backend;
}

const starterPrompts = [
  "Tóm tắt các tài liệu mới trong thư viện",
  "Giúp tôi lên kế hoạch công việc hôm nay",
  "Tìm lại thông tin tôi đã lưu về dự án",
];

function getStoredFlag(key: string) {
  if (typeof window === "undefined") return false;
  return window.localStorage.getItem(key) === "1";
}

function getStoredPreference<T extends string>(key: string, allowed: T[], fallback: T): T {
  if (typeof window === "undefined") return fallback;
  const value = window.localStorage.getItem(key) as T | null;
  return value && allowed.includes(value) ? value : fallback;
}

const MODEL_ACTIONS: Record<string, string> = {
  pull: "Đã tải mô hình",
  load: "Đã nạp mô hình",
  unload: "Đã dỡ mô hình",
  update: "Đã cập nhật mô hình",
  delete: "Đã xoá mô hình",
};

const modelStateLabel = (state: ModelInfo["state"]) => {
  switch (state) {
    case "loaded": return "Đang nằm trong bộ nhớ";
    case "installed": return "Đã cài đặt";
    case "unloaded": return "Chưa nạp vào bộ nhớ";
    case "downloading": return "Đang tải";
    case "failed": return "Lỗi";
  }
};

function modelActionLabel(action: string) {
  if (action.startsWith("select_default:")) return "Đã đổi mô hình mặc định";
  return MODEL_ACTIONS[action] ?? action;
}

const isDocumentBusy = (document: DocumentRecord) =>
  document.status === "queued" ||
  document.status === "processing" ||
  document.ingestion?.status === "processing";

const documentStatusLabel = (document: DocumentRecord) => {
  if (document.ingestion?.status === "processing") {
    return `${document.ingestion.detail} · ${Math.round(document.ingestion.progress * 100)}%`;
  }
  switch (document.status) {
    case "queued": return "Đang chờ xử lý";
    case "processing": return "Đang OCR";
    case "ready": return "Sẵn sàng";
    case "needs_ocr": return "OCR chưa đọc được";
    case "failed": return "Xử lý lỗi";
  }
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
    setStatusText("Đang dỡ khỏi bộ nhớ…");
    try {
      await api.unloadModel(props.model.name);
      props.onRefresh();
      setStatusText("");
    } catch (cause) {
      setStatusText(cause instanceof Error ? cause.message : "Không thể dỡ mô hình");
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
    setStatusText("Đang xóa mô hình…");
    try {
      await api.deleteModel(props.model.name);
      props.onRefresh();
    } catch (cause) {
      setStatusText(cause instanceof Error ? cause.message : "Không thể xóa mô hình");
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
      <div class="model-state"><span class={`status-pip model-status-${props.model.state}`} aria-hidden="true" />{modelStateLabel(props.model.state)}</div>
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
  let composerInput: HTMLTextAreaElement | undefined;
  const [view, setView] = createSignal<View>("chat");
  const [settingsTab, setSettingsTab] = createSignal<SettingsTab>("general");
  const [theme, setTheme] = createSignal<Theme>(getStoredPreference("private-ai-theme", ["light", "dark"], "light"));
  const [fontScale, setFontScale] = createSignal<FontScale>(getStoredPreference("private-ai-font-scale", ["normal", "large"], "normal"));
  const [activeWorkspace, setActiveWorkspace] = createSignal("");
  const [activeConversation, setActiveConversation] = createSignal("");
  const [confirmConversationDelete, setConfirmConversationDelete] = createSignal(false);
  const [confirmWorkspaceDelete, setConfirmWorkspaceDelete] = createSignal("");
  const [sidebarOpen, setSidebarOpen] = createSignal(false);
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(getStoredFlag("private-ai-sidebar-collapsed"));
  const [railCollapsed, setRailCollapsed] = createSignal(getStoredFlag("private-ai-rail-collapsed"));
  const [contextOpen, setContextOpen] = createSignal(
    typeof window === "undefined" ? true : window.innerWidth > 1180 && !getStoredFlag("private-ai-rail-collapsed"),
  );
  const [compactLayout, setCompactLayout] = createSignal(
    typeof window !== "undefined" && window.innerWidth <= 1180,
  );
  const [messages, setMessages] = createSignal<ChatMessage[]>([]);
  const [draft, setDraft] = createSignal("");
  const [selectedModel, setSelectedModel] = createSignal("");
  const [sending, setSending] = createSignal(false);
  const [activeTool, setActiveTool] = createSignal("");
  const [chatError, setChatError] = createSignal("");
  // dragenter/dragleave also fire for children, so depth decides when the overlay drops.
  const [chatDragDepth, setChatDragDepth] = createSignal(0);
  const [uploadOpen, setUploadOpen] = createSignal(false);
  const [stagedFiles, setStagedFiles] = createSignal<File[]>([]);
  const [viewingDocument, setViewingDocument] = createSignal("");
  // The file picker is a separate element, so the choice has to survive the round trip.
  const [recording, setRecording] = createSignal(false);
  const [transcribing, setTranscribing] = createSignal(false);
  const [showScrollToBottom, setShowScrollToBottom] = createSignal(false);
  const [onboardingDismissed, setOnboardingDismissed] = createSignal(
    getStoredFlag("private-ai-onboarding-dismissed"),
  );
  const [health, { refetch: refetchHealth }] = createResource(api.health);
  const [models, { refetch: refetchModels }] = createResource(api.models);
  const [modelEvents, { refetch: refetchModelEvents }] = createResource(api.modelEvents);
  const [workspaceList, { refetch: refetchWorkspaces }] = createResource(api.workspaces);
  const [preferences, { mutate: mutatePreferences }] = createResource(api.preferences);
  const [profiles, { refetch: refetchProfiles }] = createResource(api.profiles);
  const ragMode = createMemo<RagMode>(() => preferences()?.rag_mode ?? "simple");
  const graphModel = createMemo(() => preferences()?.graph_model ?? "");
  const webSearchEnabled = createMemo(() => preferences()?.web_search_enabled ?? false);
  const webSearchBackend = createMemo<WebSearchBackend>(
    () => preferences()?.web_search_backend ?? "duckduckgo",
  );
  const [webSearchUrlDraft, setWebSearchUrlDraft] = createSignal("");
  const [webSearchKeyDraft, setWebSearchKeyDraft] = createSignal("");
  const [webSearchModelDraft, setWebSearchModelDraft] = createSignal("");
  const [webSearchProbing, setWebSearchProbing] = createSignal(false);
  const [webSearchProbe, setWebSearchProbe] = createSignal<WebSearchProbeResult>();
  const [preferencesSaving, setPreferencesSaving] = createSignal(false);
  const [preferencesError, setPreferencesError] = createSignal("");
  const [preferencesNotice, setPreferencesNotice] = createSignal("");
  const [embeddingBatchDraft, setEmbeddingBatchDraft] = createSignal("32");
  const [embeddingConcurrencyDraft, setEmbeddingConcurrencyDraft] = createSignal("4");

  createEffect(() => {
    const current = preferences();
    if (!current) return;
    setEmbeddingBatchDraft(String(current.embedding_batch_size));
    setEmbeddingConcurrencyDraft(String(current.embedding_concurrency));
    setWebSearchUrlDraft(current.web_search_base_url);
    setWebSearchModelDraft(current.web_search_model);
  });

  const activeProfile = createMemo(() => profiles()?.find((profile) => profile.active));
  const profileName = createMemo(() => activeProfile()?.display_name?.trim() ?? "");
  // An empty name means nobody has introduced themselves yet, which is the onboarding cue.
  const needsOnboarding = createMemo(() =>
    Boolean(!onboardingDismissed() && !profiles.loading && activeProfile() && !profileName()),
  );

  const openUpload = (files: File[] = []) => {
    setStagedFiles(files);
    setUploadOpen(true);
  };
  const savePreferences = async (changes: PreferencesUpdate, notice: string) => {
    const previous = preferences();
    // The raw key is write-only, so the optimistic copy carries only the flag the UI reads.
    const { web_search_api_key: apiKey, ...visible } = changes;
    const optimistic: Preferences = {
      ocr_enabled: true,
      rag_mode: "simple",
      graph_model: "",
      embedding_batch_size: 32,
      embedding_concurrency: 4,
      web_search_enabled: false,
      web_search_backend: "duckduckgo",
      web_search_base_url: "",
      web_search_model: "",
      web_search_max_results: 5,
      web_search_has_api_key: false,
      ...previous,
      ...visible,
      ...(apiKey === undefined ? {} : { web_search_has_api_key: Boolean(apiKey.trim()) }),
    };
    setPreferencesError("");
    setPreferencesNotice("");
    setPreferencesSaving(true);
    mutatePreferences(optimistic);
    try {
      mutatePreferences(await api.updatePreferences(changes));
      setPreferencesNotice(notice);
    } catch (cause) {
      mutatePreferences(previous);
      setPreferencesError(cause instanceof Error ? cause.message : "Không lưu được cài đặt");
    } finally {
      setPreferencesSaving(false);
    }
  };

  const toggleOcr = (enabled: boolean) =>
    savePreferences({ ocr_enabled: enabled }, "Đã lưu lựa chọn OCR");

  const selectRagMode = (mode: RagMode) =>
    savePreferences(
      { rag_mode: mode },
      mode === "simple" ? "Đã chọn RAG nhanh" : "Đã chọn Graph RAG",
    );

  const selectGraphModel = (model: string) =>
    savePreferences(
      { graph_model: model },
      model ? `Graph RAG sẽ dùng ${model}` : "Graph RAG sẽ dùng mô hình chat mặc định",
    );

  const toggleWebSearch = (enabled: boolean) => {
    setWebSearchProbe(undefined);
    return savePreferences(
      { web_search_enabled: enabled },
      enabled
        ? "Đã bật tìm kiếm web: câu hỏi sẽ được gửi ra ngoài máy"
        : "Đã tắt tìm kiếm web",
    );
  };

  const selectWebSearchBackend = (backend: WebSearchBackend) => {
    setWebSearchProbe(undefined);
    return savePreferences({ web_search_backend: backend }, "Đã chọn nguồn tìm kiếm");
  };

  const saveWebSearchUrl = () =>
    savePreferences({ web_search_base_url: webSearchUrlDraft().trim() }, "Đã lưu địa chỉ SearXNG");

  const saveWebSearchModel = () =>
    savePreferences({ web_search_model: webSearchModelDraft().trim() }, "Đã lưu mô hình tìm kiếm");

  const saveWebSearchKey = async () => {
    const key = webSearchKeyDraft().trim();
    if (!key) return;
    await savePreferences({ web_search_api_key: key }, "Đã lưu API key");
    setWebSearchKeyDraft("");
  };

  const clearWebSearchKey = () =>
    savePreferences({ web_search_api_key: "" }, "Đã xóa API key đã lưu");

  // The probe runs against the drafts on screen, so a wrong host shows up here and not
  // in the middle of a conversation.
  const runWebSearchProbe = async () => {
    setWebSearchProbing(true);
    setWebSearchProbe(undefined);
    try {
      setWebSearchProbe(await api.probeWebSearch({
        backend: webSearchBackend(),
        base_url: webSearchUrlDraft().trim(),
        api_key: webSearchKeyDraft().trim(),
        model: webSearchModelDraft().trim(),
      }));
    } catch (cause) {
      setWebSearchProbe({
        reachable: false,
        result_count: 0,
        host: "",
        on_device: false,
        detail: cause instanceof Error ? cause.message : "Không kiểm tra được kết nối",
      });
    } finally {
      setWebSearchProbing(false);
    }
  };

  const commitEmbeddingSetting = (
    key: "embedding_batch_size" | "embedding_concurrency",
    rawValue: string,
  ) => {
    const value = Number(rawValue);
    const [minimum, maximum] = key === "embedding_batch_size" ? [1, 256] : [1, 32];
    if (!Number.isInteger(value) || value < minimum || value > maximum) {
      const current = preferences();
      setEmbeddingBatchDraft(String(current?.embedding_batch_size ?? 32));
      setEmbeddingConcurrencyDraft(String(current?.embedding_concurrency ?? 4));
      setPreferencesError(`Giá trị phải là số nguyên từ ${minimum} đến ${maximum}`);
      setPreferencesNotice("");
      return;
    }
    void savePreferences(
      { [key]: value },
      "Đã áp dụng cấu hình embedding cho lần lập chỉ mục tiếp theo",
    );
  };
  // createResource only skips a fetch for false/null/undefined, so an empty id would be
  // sent as a real request and leave both resources stuck in an error state.
  const workspaceSource = createMemo(() => activeWorkspace() || undefined);
  const [conversations, { refetch: refetchConversations }] = createResource(
    workspaceSource,
    api.conversations,
  );
  const [documentPage, setDocumentPage] = createSignal(0);
  const [documentSearch, setDocumentSearch] = createSignal("");
  const [documentStatus, setDocumentStatus] = createSignal("");
  const documentQuery = createMemo(() => {
    const workspaceId = activeWorkspace();
    if (!workspaceId) return undefined;
    return {
      workspaceId,
      offset: documentPage() * DOCUMENTS_PER_PAGE,
      search: documentSearch(),
      status: documentStatus(),
    };
  });
  const [documents, { refetch: refetchDocuments }] = createResource(documentQuery, (query) =>
    api.documents(
      query.workspaceId,
      DOCUMENTS_PER_PAGE,
      query.offset,
      query.search,
      query.status,
    ),
  );
  // Keep the previous page on screen while the next one loads instead of blanking the list,
  // and reconcile by id so a background poll patches the changed fields in place rather than
  // tearing down and rebuilding every row.
  const [documentRows, setDocumentRows] = createStore<{ items: DocumentRecord[] }>({ items: [] });
  createEffect(() => {
    const page = documents();
    if (page) setDocumentRows("items", reconcile(page.items, { key: "id" }));
  });
  createEffect(() => {
    activeWorkspace();
    setDocumentRows("items", reconcile([], { key: "id" }));
  });
  const documentItems = () => documentRows.items;
  // Only the first load of a page has nothing to show; the 1.2s progress poll must not swap
  // the list out for a spinner or dim it, or the panel blinks the whole time a file indexes.
  // "pending" is a real reload (workspace, page or filter changed); a poll only ever puts the
  // resource in "refreshing", which must stay invisible.
  const documentsReloading = createMemo(() => documents.state === "pending");
  const documentsFirstLoad = createMemo(() => documentsReloading() && documentItems().length === 0);
  const documentTotal = createMemo(() => documents()?.total ?? 0);
  const documentSummary = createMemo(
    () => documents()?.summary ?? { total: 0, byte_size: 0, pending: 0, indexing: 0, failed: 0 },
  );
  const documentPageCount = createMemo(() =>
    Math.max(1, Math.ceil(documentTotal() / DOCUMENTS_PER_PAGE)),
  );
  const changeDocumentFilter = (search: string, status: string) => {
    setDocumentSearch(search);
    setDocumentStatus(status);
    setDocumentPage(0);
  };
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
  let followLatestMessages = true;

  const isNearMessageBottom = () =>
    !messageList || messageList.scrollHeight - messageList.scrollTop - messageList.clientHeight < 80;

  const scrollToLatest = (behavior: ScrollBehavior = "smooth") => {
    followLatestMessages = true;
    setShowScrollToBottom(false);
    messageList?.scrollTo({ top: messageList.scrollHeight, behavior });
  };

  const toggleContext = () => {
    const next = !contextOpen();
    setContextOpen(next);
    setRailCollapsed(!next);
  };

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
  const usableChatModels = createMemo(() =>
    chatModels().filter((model) => model.state !== "failed" && model.state !== "downloading"),
  );
  const serviceState = (name: string): ServiceState => health()?.services[name] ?? "offline";
  const voiceModelLoaded = createMemo(() =>
    Boolean(models()?.some((model) => model.model_type === "asr" && model.state === "loaded")),
  );
  const voiceReady = createMemo(() =>
    serviceState("asr") === "online" && voiceModelLoaded(),
  );
  const voiceControlLabel = createMemo(() => {
    if (recording()) return "Dừng ghi âm";
    if (transcribing()) return "Đang nhận dạng giọng nói";
    if (voiceReady()) return "Giọng nói sẵn sàng — bắt đầu ghi âm";
    if (serviceState("asr") === "online") return "Nạp mô hình giọng nói để bắt đầu";
    return "Giọng nói chưa sẵn sàng";
  });
  const providerOnDevice = createMemo(() => Boolean(health()?.provider?.on_device));
  const providerStatus = createMemo(() => {
    const provider = health()?.provider;
    if (!provider) return "Chưa cấu hình";
    if (serviceState("provider") === "online") {
      return providerOnDevice() ? `${provider.name} · trên thiết bị` : `${provider.name} · từ xa`;
    }
    return `${provider.name} · không phản hồi`;
  });
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
  const vramTitle = createMemo(() =>
    health()?.gpu.unified_memory ? "Bộ nhớ hợp nhất cho GPU" : "VRAM đang dùng",
  );
  const vramDetail = createMemo(() => {
    const gpu = health()?.gpu;
    const count = gpu?.leases?.length ?? 0;
    const models = count ? `${count} mô hình đang dùng` : "Không có mô hình trong GPU";
    if (!gpu?.unified_memory || !gpu.total_memory_bytes) return models;
    return `${models} · dùng chung ${formatBytes(gpu.total_memory_bytes)} RAM của SoC`;
  });

  const openSettingsTab = (tab: SettingsTab) => {
    setView("settings");
    setSettingsTab(tab);
    setSidebarOpen(false);
  };

  const notices = createMemo<Notice[]>(() => {
    const list: Notice[] = [];
    const state = (name: string) => health()?.services[name];
    if (state("provider") === "not_configured") {
      list.push({ id: "provider-missing", tone: "warn", title: "Chưa chọn nhà cung cấp AI", detail: "Thêm Ollama hoặc một endpoint tương thích OpenAI trong Cài đặt.", actionLabel: "Mở nhà cung cấp", onAction: () => openSettingsTab("providers") });
    } else if (state("provider") === "offline") {
      list.push({ id: "provider-offline", tone: "alert", title: "Nhà cung cấp AI không phản hồi", detail: "Không gọi được endpoint đang chọn, cuộc trò chuyện sẽ lỗi.", actionLabel: "Kiểm tra nhà cung cấp", onAction: () => openSettingsTab("providers") });
    }
    if (providerOnDevice() && state("local_runtime") === "offline") {
      list.push({ id: "local-runtime-offline", tone: "alert", title: "Máy chủ mô hình cục bộ đã ngoại tuyến", detail: "Nhà cung cấp đang chọn chạy trên máy này nhưng runtime không phản hồi.", actionLabel: "Mở mô hình", onAction: () => openSettingsTab("models") });
    }
    if (state("knowledge_graph") === "not_configured") {
      list.push({ id: "graph-missing", tone: "warn", title: "Kho tri thức chưa dựng", detail: "Tải tài liệu lên để Private AI lập chỉ mục và trả lời theo ngữ cảnh.", actionLabel: "Mở tài liệu", onAction: () => setView("library") });
    }
    if (state("asr") === "offline") {
      list.push({ id: "asr-offline", tone: "warn", title: "Nhập bằng giọng nói chưa sẵn sàng", detail: "Cài mô hình nhận dạng giọng nói trong Cài đặt → Mô hình.", actionLabel: "Mở mô hình", onAction: () => openSettingsTab("models") });
    }
    const summary = documentSummary();
    if (summary.failed) {
      const firstFailure = documentItems().find((document) =>
        document.status === "failed" || document.status === "needs_ocr"
      );
      list.push({
        id: "documents-failed",
        tone: "alert",
        title: `${summary.failed} tài liệu xử lý lỗi`,
        detail: firstFailure
          ? `${firstFailure.filename}: ${firstFailure.error ?? documentStatusLabel(firstFailure)}`
          : "Mở Thư viện để xem nguyên nhân và thử xử lý lại.",
        actionLabel: "Mở tài liệu",
        onAction: () => setView("library"),
      });
    }
    if (summary.pending) {
      const firstPending = documentItems().find(isDocumentBusy);
      list.push({
        id: "documents-pending",
        tone: "info",
        title: `${summary.pending} tài liệu đang xử lý`,
        detail: firstPending
          ? `${firstPending.filename}: ${documentStatusLabel(firstPending)}`
          : "Nội dung sẽ vào kho tri thức khi trích xuất xong.",
        actionLabel: "Theo dõi tài liệu",
        onAction: () => setView("library"),
      });
    }
    if (summary.indexing) {
      const firstIndexing = documentItems().find((document) =>
        document.ingestion?.status === "processing"
      );
      list.push({
        id: "documents-indexing",
        tone: "info",
        title: `${summary.indexing} tài liệu đang vào kho tri thức`,
        detail: firstIndexing
          ? `${firstIndexing.filename}: ${documentStatusLabel(firstIndexing)}`
          : "Đang tạo embedding và graph memory.",
        actionLabel: "Theo dõi tài liệu",
        onAction: () => setView("library"),
      });
    }
    for (const event of (modelEvents() ?? []).filter((item) => item.status === "failed").slice(0, 6)) {
      const failed = event.status === "failed";
      list.push({
        id: event.id,
        tone: failed ? "alert" : "info",
        title: `${modelActionLabel(event.action)}${failed ? " thất bại" : ""}`,
        detail: failed && event.detail ? `${event.model_name} — ${event.detail}` : event.model_name,
        at: event.created_at,
        actionLabel: "Mở mô hình",
        onAction: () => openSettingsTab("models"),
      });
    }
    return list;
  });

  const healthPoll = window.setInterval(() => {
    if (document.visibilityState === "visible") void refetchHealth();
  }, 5_000);
  // One workspace-level poll updates every queued row and the notification menu without
  // issuing one request per file.
  // Simple RAG never builds a LightRAG instance, so the knowledge graph stays
  // "not_configured" while vectors are being embedded: gate the poll on the documents
  // themselves instead of on that service state.
  const documentsWorking = createMemo(() => {
    const summary = documentSummary();
    return summary.pending > 0 || summary.indexing > 0 || documentItems().some(isDocumentBusy);
  });
  const documentPoll = window.setInterval(() => {
    if (document.visibilityState === "visible" && documentsWorking()) {
      void refetchDocuments();
    }
  }, 1_200);
  const refreshVisibleHealth = () => {
    if (document.visibilityState === "visible") void refetchHealth();
  };
  const refreshCompactLayout = () => {
    const compact = window.innerWidth <= 1180;
    setCompactLayout(compact);
    setContextOpen(compact ? false : !railCollapsed());
  };
  window.addEventListener("resize", refreshCompactLayout);
  document.addEventListener("visibilitychange", refreshVisibleHealth);
  onCleanup(() => {
    window.clearInterval(healthPoll);
    window.clearInterval(documentPoll);
    window.removeEventListener("resize", refreshCompactLayout);
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
    window.localStorage.setItem("private-ai-sidebar-collapsed", sidebarCollapsed() ? "1" : "0");
    window.localStorage.setItem("private-ai-rail-collapsed", railCollapsed() ? "1" : "0");
  });

  createEffect(() => {
    const available = usableChatModels();
    // Switching provider replaces the whole inventory, so a stale pick has to be dropped.
    if (!available.length) {
      setSelectedModel("");
      return;
    }
    if (available.some((model) => model.name === selectedModel())) return;
    const preferred = available.find((model) => model.default_for.includes("chat")) ?? available[0];
    setSelectedModel(preferred.name);
  });

  const chooseChatModel = (name: string) => {
    setSelectedModel(name);
    if (name) void api.setDefaultModel("chat", name).then(() => refetchModels());
  };

  const refresh = () => Promise.all([
    refetchHealth(),
    refetchModels(),
    refetchModelEvents(),
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

  // Opening a workspace from the management page has to land somewhere visible: a collapsed
  // sidebar hides the workspace list entirely, and a long list can leave the selected row
  // scrolled out of sight, so the selection would be correct but invisible.
  const revealWorkspaceInSidebar = (id: string) => {
    if (sidebarCollapsed()) setSidebarCollapsed(false);
    window.requestAnimationFrame(() => {
      document
        .querySelector(`.workspace-list [data-workspace-id="${CSS.escape(id)}"]`)
        ?.scrollIntoView({ block: "nearest" });
    });
  };

  const chooseWorkspace = (id: string) => {
    setActiveWorkspace(id);
    setConfirmWorkspaceDelete("");
    changeDocumentFilter("", "");
    setActiveConversation("");
    setView("chat");
    setMessages([]);
    setChatError("");
    setSidebarOpen(false);
    revealWorkspaceInSidebar(id);
  };

  const focusComposer = () => {
    window.requestAnimationFrame(() => composerInput?.focus());
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
      focusComposer();
    } catch (cause) {
      setChatError(cause instanceof Error ? cause.message : "Không thể tạo cuộc trò chuyện");
    }
  };

  const activateCreatedWorkspace = async (workspace: WorkspaceRecord) => {
    // Start refreshing first so the workspace-selection effect does not compare against a
    // stale list and jump back to the previous workspace.
    const workspaceRefresh = refetchWorkspaces();
    setActiveWorkspace(workspace.id);
    setActiveConversation("");
    setMessages([]);
    setDraft("");
    setChatError("");
    setView("chat");
    setSidebarOpen(false);
    changeDocumentFilter("", "");
    try {
      const conversation = await api.createConversation(workspace.id, selectedModel());
      setActiveConversation(conversation.id);
      await Promise.allSettled([workspaceRefresh, refetchConversations()]);
    } catch (cause) {
      await Promise.allSettled([workspaceRefresh]);
      setChatError(
        cause instanceof Error ? cause.message : "Không thể tạo cuộc trò chuyện đầu tiên",
      );
    } finally {
      focusComposer();
    }
  };

  const handleWorkspaceSaved = (workspace: WorkspaceRecord, created: boolean) => {
    if (created) {
      void activateCreatedWorkspace(workspace);
      return;
    }
    void refetchWorkspaces();
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
    if (!documents()) return;
    if (documentPage() > documentPageCount() - 1) setDocumentPage(documentPageCount() - 1);
  });

  createEffect(() => {
    const items = workspaceList();
    if (
      !items ||
      workspaceList.loading ||
      items.some((workspace) => workspace.id === activeWorkspace())
    ) return;
    setActiveWorkspace(items[0]?.id ?? "");
  });

  createEffect(() => {
    const items = conversations();
    if (!items || activeConversation() || items.length === 0) return;
    void openConversation(items[0].id);
  });

  const copyMessage = async (content: string) => {
    try {
      await navigator.clipboard.writeText(content);
      notify({ tone: "success", title: "Đã sao chép", description: "Câu trả lời đã được chép vào bộ nhớ tạm." });
    } catch {
      notify({ tone: "error", title: "Không thể sao chép", description: "Trình duyệt không cho phép truy cập bộ nhớ tạm." });
    }
  };

  const regenerateMessage = async (index: number) => {
    if (sending()) return;
    const current = messages();
    let userIndex = -1;
    for (let position = index - 1; position >= 0; position -= 1) {
      if (current[position]?.role === "user") {
        userIndex = position;
        break;
      }
    }
    const prompt = userIndex >= 0 ? current[userIndex]?.content : undefined;
    if (!prompt) return;
    setMessages(current.slice(0, userIndex));
    await submitMessage(prompt);
  };

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
    followLatestMessages = isNearMessageBottom();
    setShowScrollToBottom(false);
    const nextMessages: ChatMessage[] = [...messages(), { role: "user", content: text }];
    let streamedAnswer = "";
    setMessages([...nextMessages, { role: "assistant", content: "" }]);
    setDraft("");
    setChatError("");
    setActiveTool("");
    setSending(true);
    const controller = new AbortController();
    let renderFrame: number | undefined;
    activeChatController = controller;
    if (followLatestMessages) queueMicrotask(() => scrollToLatest());
    try {
      const response: ConversationDetail = await api.streamConversation(
        conversationId,
        selectedModel(),
        text,
        ragMode(),
        webSearchEnabled(),
        (content) => {
          streamedAnswer += content;
          if (renderFrame === undefined) {
            renderFrame = window.requestAnimationFrame(() => {
              setMessages([...nextMessages, { role: "assistant", content: streamedAnswer }]);
              if (followLatestMessages) scrollToLatest("auto");
              else setShowScrollToBottom(true);
              renderFrame = undefined;
            });
          }
        },
        (message) => notify({
          tone: "info",
          title: "Tìm kiếm web không dùng được",
          description: message,
        }),
        (name) => setActiveTool(name),
        controller.signal,
      );
      if (renderFrame !== undefined) window.cancelAnimationFrame(renderFrame);
      renderFrame = undefined;
      setMessages(response.messages);
      refetchConversations();
      refetchWorkspaces();
      if (followLatestMessages) queueMicrotask(() => scrollToLatest());
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
      setActiveTool("");
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
          void refetchModels();
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
  // Without this, a file dropped anywhere else replaces the app with the file itself.
  const swallowStrayDrop = (event: DragEvent) => event.preventDefault();
  window.addEventListener("dragover", swallowStrayDrop);
  window.addEventListener("drop", swallowStrayDrop);
  onCleanup(() => {
    window.removeEventListener("dragover", swallowStrayDrop);
    window.removeEventListener("drop", swallowStrayDrop);
  });

  return (
    <>
      <a class="skip-link" href="#main-content">Bỏ qua thanh điều hướng</a>
      <div classList={{ "app-shell": true, "sidebar-collapsed": sidebarCollapsed(), "rail-collapsed": railCollapsed() }}>
        <aside classList={{ sidebar: true, open: sidebarOpen() }} aria-label="Không gian làm việc">
          <div class="sidebar-header">
            <button class="brand" onClick={() => setView("chat")} aria-label="Private AI">
              <span class="brand-mark"><i /><i /><i /></span>
              <span><strong>PRIVATE</strong><em>AI</em></span>
            </button>
            <button
              class="sidebar-toggle"
              onClick={() => setSidebarCollapsed(!sidebarCollapsed())}
              aria-label={sidebarCollapsed() ? "Mở rộng thanh bên" : "Thu gọn thanh bên"}
              aria-expanded={!sidebarCollapsed()}
              title={sidebarCollapsed() ? "Mở rộng thanh bên" : "Thu gọn thanh bên"}
            >
              {sidebarCollapsed() ? <PanelLeftOpen size={19} /> : <PanelLeftClose size={19} />}
            </button>
            <button class="icon-button mobile-close" onClick={() => setSidebarOpen(false)} aria-label="Đóng menu"><X size={21} /></button>
          </div>
          <button class="new-chat-button" onClick={newConversation} title="Cuộc trò chuyện mới"><Plus size={20} /><span>Cuộc trò chuyện mới</span></button>
          <nav class="primary-nav" aria-label="Điều hướng chính">
            <For each={navigation}>{(item) => (
              <button classList={{ "nav-item": true, active: view() === item.id }} onClick={() => { setView(item.id); setSidebarOpen(false); }} aria-current={view() === item.id ? "page" : undefined} title={item.label}>
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
                <div
                  data-workspace-id={workspace.id}
                  classList={{ "workspace-row": true, active: activeWorkspace() === workspace.id }}
                >
                  <button class="workspace-item" onClick={() => chooseWorkspace(workspace.id)} title={workspace.description || workspace.name}>
                    <span class="workspace-dot" aria-hidden="true" />
                    <span class="workspace-copy"><strong>{workspace.name}</strong></span>
                  </button>
                  <button classList={{ "workspace-delete": true, danger: confirmWorkspaceDelete() === workspace.id }} onClick={() => void deleteWorkspace(workspace.id)} aria-label={confirmWorkspaceDelete() === workspace.id ? `Bấm lại để xác nhận xóa ${workspace.name} và toàn bộ cuộc trò chuyện bên trong` : `Xóa không gian làm việc ${workspace.name}`}><Trash2 size={15} /></button>
                </div>
              )}</For>
            </div>
            <button class="workspace-manage" onClick={() => { setView("workspaces"); setSidebarOpen(false); }}>
              <LayoutGrid size={16} /> Quản lý không gian
            </button>
            <div class="section-label conversation-label"><span>Gần đây</span></div>
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
          <div class="sidebar-footer">
            <ProfileSwitcher
              profiles={profiles() ?? []}
              active={activeProfile()}
              online={health()?.status === "ok"}
              onChanged={() => refetchProfiles()}
              onOpenSettings={() => { setView("settings"); setSidebarOpen(false); }}
            />
          </div>
        </aside>

        <main class="main-stage" id="main-content">
          <header class="topbar">
            <div class="topbar-title">
              <button class="icon-button mobile-menu" onClick={() => setSidebarOpen(true)} aria-label="Mở menu" title="Mở menu"><Menu size={22} /></button>
              <div><strong>{view() === "chat" ? (hasWorkspace() ? currentWorkspace().name : "Chưa có không gian") : view() === "settings" ? "Cài đặt" : navigation.find((item) => item.id === view())?.label}</strong><span><StatusPip state={health()?.status === "ok" ? "online" : "offline"} /> Trên thiết bị</span></div>
            </div>
            <div class="topbar-actions">
              <Show when={view() === "chat"}>
                <button
                  class="icon-button context-toggle"
                  onClick={toggleContext}
                  aria-label={contextOpen() ? "Ẩn bảng ngữ cảnh" : "Hiện bảng ngữ cảnh"}
                  aria-expanded={contextOpen()}
                  title={contextOpen() ? "Ẩn bảng ngữ cảnh" : "Hiện bảng ngữ cảnh"}
                >
                  {contextOpen() ? <PanelRightClose size={19} /> : <PanelRightOpen size={19} />}
                </button>
              </Show>
              <NotificationsMenu notices={notices()} onOpen={() => void refresh()} />
            </div>
          </header>

          <Switch>
            <Match when={view() === "chat"}>
              <div
                classList={{ "chat-workspace": true, "context-open": contextOpen() }}
                onDragEnter={(event) => {
                  if (!event.dataTransfer?.types.includes("Files")) return;
                  event.preventDefault();
                  setChatDragDepth(chatDragDepth() + 1);
                }}
                onDragOver={(event) => {
                  if (event.dataTransfer?.types.includes("Files")) event.preventDefault();
                }}
                onDragLeave={() => setChatDragDepth(Math.max(0, chatDragDepth() - 1))}
                onDrop={(event) => {
                  const files = Array.from(event.dataTransfer?.files ?? []);
                  if (!files.length) return;
                  event.preventDefault();
                  setChatDragDepth(0);
                  openUpload(files);
                }}
              >
                <Show when={chatDragDepth() > 0}>
                  <div class="chat-dropzone" aria-hidden="true">
                    <div>
                      <FileUp size={30} />
                      <strong>Thả để thêm vào {hasWorkspace() ? currentWorkspace().name : "thư viện"}</strong>
                      <span>PDF, Office, ảnh, Markdown và văn bản · tối đa 100 MB mỗi tệp</span>
                    </div>
                  </div>
                </Show>
                <section class="conversation" aria-label="Cuộc trò chuyện" aria-busy={sending()}>
                  <div
                    class="message-list"
                    ref={messageList}
                    onScroll={() => {
                      followLatestMessages = isNearMessageBottom();
                      setShowScrollToBottom(!followLatestMessages);
                    }}
                  >
                    <Show when={showScrollToBottom()}>
                      <button class="scroll-latest-button" type="button" onClick={() => scrollToLatest()}>
                        <ChevronDown size={17} aria-hidden="true" /> Cuộn tới trả lời mới nhất
                      </button>
                    </Show>
                    <Show when={sending()}>
                      <div class="sr-only" aria-live="polite">Private AI đang tạo câu trả lời.</div>
                    </Show>
                    <Show when={messages().length > 0} fallback={
                      <div class="chat-welcome">
                        <div class="welcome-mark"><Sparkles size={28} /></div>
                        <p>{profileName() ? `Chào bạn, ${profileName()}` : "Chào bạn"}</p>
                        <h1>Hôm nay bạn muốn làm gì?</h1>
                        <span>Hỏi bằng ngôn ngữ tự nhiên. Private AI sẽ dùng mô hình và tài liệu trên máy để trả lời.</span>
                        <Show when={!workspaceList.loading && (!hasWorkspace() || !selectedModel())}>
                          <div class="chat-setup-card">
                            <strong>Thiết lập trước khi trò chuyện</strong>
                            <span>Tạo nơi lưu dữ liệu và chọn một mô hình có thể trả lời.</span>
                            <div>
                              <Show when={!hasWorkspace()}>
                                <button class="button button-secondary" type="button" onClick={() => { setView("workspaces"); setSidebarOpen(false); }}>
                                  <Plus size={17} aria-hidden="true" /> Tạo không gian
                                </button>
                              </Show>
                              <Show when={!selectedModel()}>
                                <button class="button button-secondary" type="button" onClick={() => { setView("settings"); setSettingsTab("models"); setSidebarOpen(false); }}>
                                  <Boxes size={17} aria-hidden="true" /> Chọn mô hình
                                </button>
                              </Show>
                            </div>
                          </div>
                        </Show>
                        <Show when={hasWorkspace() && Boolean(selectedModel())}>
                          <div class="starter-prompts"><For each={starterPrompts}>{(prompt) => <button type="button" onClick={() => void submitMessage(prompt)}>{prompt}<Send size={17} aria-hidden="true" /></button>}</For></div>
                        </Show>
                      </div>
                    }>
                      <div class="message-stream">
                        <Index each={messages()}>{(message, index) => (
                          <article class={`message message-${message().role}`}>
                            <div class="message-author"><span>{message().role === "user" ? initialsOf(profileName() || "Bạn") : "AI"}</span><strong>{message().role === "user" ? (profileName() || "Bạn") : "Private AI"}</strong></div>
                            <Show
                              when={message().content}
                              fallback={<div class="thinking"><i /><i /><i /><span>{activeTool() ? `Đang dùng ${activeTool()}` : "Đang suy nghĩ"}</span></div>}
                            >
                              <div class="message-content">
                                <Markdown content={message().content} />
                                <Show when={message().role === "assistant" && Boolean(message().content) && index === messages().length - 1}>
                                  <div class="message-actions" aria-label="Thao tác với câu trả lời">
                                    <button type="button" onClick={() => void copyMessage(message().content)} title="Sao chép câu trả lời">
                                      <Clipboard size={15} aria-hidden="true" /> <span>Sao chép</span>
                                    </button>
                                    <button type="button" disabled={sending()} onClick={() => void regenerateMessage(index)} title="Tạo lại câu trả lời">
                                      <RefreshCw size={15} aria-hidden="true" /> <span>Tạo lại</span>
                                    </button>
                                  </div>
                                </Show>
                              </div>
                            </Show>
                          </article>
                        )}</Index>
                      </div>
                    </Show>
                  </div>
                  <div class="composer-wrap">
                    <Show when={chatError()}><div class="inline-error" role="alert">{chatError()}</div></Show>
                    <form class="composer" onSubmit={(event) => { event.preventDefault(); void submitMessage(); }}>
                      <textarea ref={(element) => { composerInput = element; }} value={draft()} onInput={(event) => setDraft(event.currentTarget.value)} onKeyDown={handleComposerKeyDown} placeholder="Nhập câu hỏi cho Private AI…" aria-label="Tin nhắn" rows={2} />
                      <div class="composer-tools">
                        <div>
                          <ModelPicker
                            models={chatModels()}
                            selected={selectedModel()}
                            loading={models.loading}
                            onSelect={chooseChatModel}
                            onManage={() => { setView("settings"); setSettingsTab("models"); }}
                          />
                          <label
                            class="rag-mode-control"
                            title={ragMode() === "simple"
                              ? "Tìm trực tiếp theo các đoạn gần nghĩa; phản hồi nhanh hơn"
                              : "Kết hợp đoạn văn với thực thể và quan hệ trong knowledge graph"}
                          >
                            <Waypoints size={17} aria-hidden="true" />
                            <select
                              value={ragMode()}
                              disabled={sending()}
                              aria-label="Chế độ truy xuất tài liệu"
                              onChange={(event) => void selectRagMode(event.currentTarget.value as RagMode)}
                            >
                              <option value="simple">RAG nhanh</option>
                              <option value="graph">Graph RAG</option>
                            </select>
                          </label>
                          <button
                            type="button"
                            classList={{ "web-search-toggle": true, active: webSearchEnabled() }}
                            disabled={sending() || preferences.loading || preferencesSaving()}
                            aria-pressed={webSearchEnabled()}
                            aria-label={webSearchEnabled() ? "Tắt tìm kiếm web" : "Bật tìm kiếm web"}
                            title={webSearchEnabled()
                              ? "Câu hỏi sẽ được gửi tới nguồn tìm kiếm đã chọn trong Cài đặt"
                              : "Tra cứu trên web trước khi trả lời. Câu hỏi sẽ rời khỏi máy này."}
                            onClick={() => void toggleWebSearch(!webSearchEnabled())}
                          >
                            <Globe size={19} aria-hidden="true" />
                            {webSearchEnabled() ? <span>{webSearchBackendLabel(webSearchBackend())}</span>: <></>}
                          </button>
                          <button type="button" onClick={() => openUpload()} aria-label="Đính kèm tài liệu"><Paperclip size={20} /></button>
                          <button
                            type="button"
                            classList={{
                              "voice-button": true,
                              // "voice-ready": voiceReady() && !recording() && !transcribing(),
                              "voice-recording": recording(),
                              "voice-processing": transcribing(),
                              "voice-unavailable": serviceState("asr") !== "online",
                            }}
                            disabled={sending() || transcribing() || serviceState("asr") !== "online"}
                            onClick={() => void toggleRecording()}
                            aria-label={voiceControlLabel()}
                            aria-busy={transcribing()}
                            aria-pressed={recording()}
                            title={voiceControlLabel()}
                          >
                            <span class="voice-wave" aria-hidden="true">
                              <i /><i /><i /><i /><i />
                            </span>
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
                    <p>
                      Enter để gửi · Shift + Enter để xuống dòng
                    </p>
                  </div>
                </section>

                <Show when={compactLayout() && contextOpen()}>
                  <button class="context-scrim" type="button" onClick={() => { setContextOpen(false); setRailCollapsed(true); }} aria-label="Đóng bảng ngữ cảnh" />
                </Show>
                <aside class="context-rail" aria-label="Ngữ cảnh không gian làm việc">
                  <div class="context-heading">
                    <div><span>Ngữ cảnh</span><strong>{hasWorkspace() ? currentWorkspace().name : "Chưa có không gian"}</strong></div>
                    <Show when={hasWorkspace()}>
                      <WorkspaceDialog workspace={currentWorkspace()} trigger="edit" onSaved={handleWorkspaceSaved} onDeleted={handleWorkspaceDeleted} />
                    </Show>
                  </div>
                  <section class="context-block">
                    <div class="context-block-heading">
                      <h2>Tài liệu</h2>
                      <Show when={hasWorkspace() && !documentsFirstLoad()}>
                        <span>{documentSummary().total}</span>
                      </Show>
                    </div>
                    <Show when={documents.error}><div class="inline-error" role="alert">{(documents.error as Error)?.message}</div></Show>
                    <Show when={hasWorkspace() && documentsFirstLoad()}>
                      <div class="context-loading" role="status"><i />Đang đọc thư viện…</div>
                    </Show>
                    <Show when={hasWorkspace() && !documentsFirstLoad() && documentItems().length > 0}>
                      <div class="context-documents"><For each={documentItems().slice(0, 3)}>{(document) => (
                        <button
                          disabled={isDocumentBusy(document)}
                          aria-busy={isDocumentBusy(document)}
                          onClick={() => !isDocumentBusy(document) && setViewingDocument(document.id)}
                          title={isDocumentBusy(document)
                            ? `${document.filename} vẫn đang xử lý`
                            : `Xem nội dung ${document.filename}`}
                        >
                          <BookOpenText size={17} aria-hidden="true" />
                          <span>
                            <strong>{document.filename}</strong>
                            <small>{documentStatusLabel(document)}</small>
                            <Show when={isDocumentBusy(document)}>
                              <i
                                class="context-document-progress"
                                role="progressbar"
                                aria-label={`Tiến độ ${document.filename}`}
                                aria-valuemin="0"
                                aria-valuemax="100"
                                aria-valuenow={Math.round((document.ingestion?.progress ?? 0.08) * 100)}
                              >
                                <span style={{ transform: `scaleX(${document.ingestion?.progress ?? 0.08})` }} />
                              </i>
                            </Show>
                          </span>
                        </button>
                      )}</For></div>
                    </Show>
                    <Show when={hasWorkspace() && documentSummary().total > 3}>
                      <button class="context-view-all" type="button" onClick={() => setView("library")}>Xem toàn bộ {documentSummary().total} tài liệu</button>
                    </Show>
                    <Show
                      when={hasWorkspace()}
                      fallback={
                        <WorkspaceDialog
                          trigger="add"
                          triggerClass="context-add context-add-create"
                          triggerLabel="Tạo không gian làm việc để thêm tài liệu"
                          triggerContent={
                            <>
                              <Plus size={19} aria-hidden="true" />
                              <span>
                                <strong>Tạo không gian trước</strong>
                                <small>Tài liệu cần một nơi lưu riêng</small>
                              </span>
                            </>
                          }
                          onSaved={handleWorkspaceSaved}
                        />
                      }
                    >
                      <Show when={!documentsFirstLoad() && documentItems().length === 0}>
                        <p class="context-empty">Chưa có tài liệu trong không gian này.</p>
                      </Show>
                      <button class="context-add" onClick={() => openUpload()}>
                        <FileUp size={19} aria-hidden="true" />
                        <span>
                          <strong>Thêm tài liệu</strong>
                          <small>Chọn nhiều tệp hoặc kéo thả vào màn hình</small>
                        </span>
                      </button>
                    </Show>
                  </section>
                  <section class="context-block"><h2>Trạng thái hệ thống</h2><dl class="service-list">
                    <div><dt><StatusPip state={serviceState("provider")} /> Nhà cung cấp AI</dt><dd>{providerStatus()}</dd></div>
                    <div><dt><StatusPip state={serviceState("knowledge_graph")} /> Kho tri thức</dt><dd>{serviceState("knowledge_graph") === "online" ? "Sẵn sàng" : "Chưa dựng"}</dd></div>
                    <div><dt><StatusPip state={serviceState("asr")} /> Giọng nói</dt><dd>{serviceState("asr") === "online" ? "Sẵn sàng" : "Chưa cấu hình"}</dd></div>
                  </dl></section>
                  <section class="context-block resource-block">
                    <div><h2>{vramTitle()}</h2><span>{vramLabel()}</span></div>
                    <div
                      class="resource-track"
                      role="progressbar"
                      aria-label={vramTitle()}
                      aria-valuemin="0"
                      aria-valuemax="100"
                      aria-valuenow={vramPercent()}
                    ><i style={{ transform: `scaleX(${vramPercent() / 100})` }} /></div>
                    <small>{vramDetail()}</small>
                  </section>
                  <div classList={{ "local-note": true, "local-note-remote": Boolean(health()?.provider && !providerOnDevice()) }}>
                    <ShieldCheck size={22} aria-hidden="true" />
                    <div>
                      <strong>{providerOnDevice() ? "Đang chạy trên thiết bị" : "Đang dùng máy chủ đã chọn"}</strong>
                      <span>{providerOnDevice() ? "Nội dung được gửi tới runtime cục bộ." : "Nội dung trò chuyện và tài liệu liên quan có thể rời khỏi máy này."}</span>
                    </div>
                  </div>
                </aside>
              </div>
            </Match>

            <Match when={view() === "workspaces"}>
              <WorkspacesView
                workspaces={workspaceList() ?? []}
                activeId={activeWorkspace()}
                loading={workspaceList.loading}
                onOpen={(id) => { chooseWorkspace(id); setView("chat"); }}
                onSaved={handleWorkspaceSaved}
                onDeleted={(id) => void handleWorkspaceDeleted(id)}
              />
            </Match>
            <Match when={view() === "library"}>
              <LibraryView
                documents={documentItems()}
                total={documentTotal()}
                summary={documentSummary()}
                page={documentPage()}
                pageSize={DOCUMENTS_PER_PAGE}
                pageCount={documentPageCount()}
                onPageChange={setDocumentPage}
                search={documentSearch()}
                status={documentStatus()}
                onFilterChange={changeDocumentFilter}
                workspaceName={hasWorkspace() ? currentWorkspace().name : "Chưa có không gian"}
                loading={documentsReloading()}
                onUpload={() => openUpload()}
                onRefresh={refetchDocuments}
              />
            </Match>
            <Match when={view() === "graph"}>
              <Suspense fallback={<section class="page-view"><p class="graph-boot">Đang mở đồ thị…</p></section>}>
                <GraphView
                  workspaceId={activeWorkspace()}
                  workspaceName={hasWorkspace() ? currentWorkspace().name : "Chưa có không gian"}
                />
              </Suspense>
            </Match>
            <Match when={view() === "settings"}>
              <section class="page-view settings-page">
                <div class="page-heading"><span>Cài đặt thiết bị</span><h1>Cài đặt</h1><p>Hiển thị, xử lý tài liệu và các cấu hình nâng cao đều nằm ở đây. Mọi lựa chọn chỉ lưu trên máy hiện tại.</p></div>
                <div class="settings-layout">
                  <nav class="settings-tabs" aria-label="Nhóm cài đặt">
                    <For each={settingsTabs}>{(tab) => (
                      <button
                        classList={{ "settings-tab": true, active: settingsTab() === tab.id }}
                        onClick={() => setSettingsTab(tab.id)}
                        aria-current={settingsTab() === tab.id ? "page" : undefined}
                      ><tab.icon size={18} stroke-width={1.8} /><span>{tab.label}</span></button>
                    )}</For>
                  </nav>

                  <div class="settings-panels">
                    <Switch>
                      <Match when={settingsTab() === "general"}>
                        <div class="settings-sections">
                          <Show when={preferencesError() || preferencesNotice()}>
                            <div
                              classList={{
                                "settings-feedback": true,
                                error: Boolean(preferencesError()),
                              }}
                              role={preferencesError() ? "alert" : "status"}
                            >{preferencesError() || preferencesNotice()}</div>
                          </Show>
                          <section><div><strong>Giao diện</strong><span>Chọn nền sáng dễ đọc hoặc nền tối.</span></div><div class="segmented-control settings-control"><button classList={{ active: theme() === "light" }} onClick={() => setTheme("light")}>Sáng</button><button classList={{ active: theme() === "dark" }} onClick={() => setTheme("dark")}>Tối</button></div></section>
                          <section><div><strong>Cỡ chữ</strong><span>Tăng toàn bộ chữ và vùng điều khiển.</span></div><button classList={{ "button": true, "button-secondary": true, active: fontScale() === "large" }} onClick={() => setFontScale(fontScale() === "large" ? "normal" : "large")}><Type size={18} />{fontScale() === "large" ? "Đang dùng chữ lớn" : "Bật chữ lớn"}</button></section>
                          <section>
                            <div>
                              <strong>Đọc văn bản bằng OCR</strong>
                              <span>Bật plugin OCR, mô hình OCR và Tesseract khi tệp không có lớp văn bản. Tắt thì chỉ đọc văn bản có sẵn, nhanh hơn nhưng bỏ qua tài liệu scan.</span>
                            </div>
                            <label class="settings-checkbox">
                              <input
                                type="checkbox"
                                checked={preferences()?.ocr_enabled ?? true}
                                disabled={preferences.loading || preferencesSaving()}
                                onChange={(event) => void toggleOcr(event.currentTarget.checked)}
                              />
                              <span>{(preferences()?.ocr_enabled ?? true) ? "Đang bật" : "Đang tắt"}</span>
                            </label>
                          </section>
                          <section>
                            <div>
                              <strong>Chế độ RAG mặc định</strong>
                              <span>RAG nhanh chỉ tạo vector, không gọi LLM. Graph RAG thêm thực thể và quan hệ để trả lời câu hỏi nhiều bước.</span>
                            </div>
                            <div class="rag-settings-control">
                              <div class="segmented-control settings-control" role="group" aria-label="Chế độ RAG mặc định">
                                <button
                                  type="button"
                                  classList={{ active: ragMode() === "simple" }}
                                  disabled={preferences.loading || preferencesSaving()}
                                  aria-pressed={ragMode() === "simple"}
                                  onClick={() => void selectRagMode("simple")}
                                >RAG nhanh</button>
                                <button
                                  type="button"
                                  classList={{ active: ragMode() === "graph" }}
                                  disabled={preferences.loading || preferencesSaving()}
                                  aria-pressed={ragMode() === "graph"}
                                  onClick={() => void selectRagMode("graph")}
                                >Graph RAG</button>
                              </div>
                              <Show when={ragMode() === "graph"}>
                                <div class="rag-graph-config">
                                  <label class="field-label" for="graph-rag-model">LLM trích xuất graph</label>
                                  <select
                                    id="graph-rag-model"
                                    class="text-input"
                                    value={graphModel()}
                                    disabled={preferences.loading || preferencesSaving() || usableChatModels().length === 0}
                                    onChange={(event) => void selectGraphModel(event.currentTarget.value)}
                                  >
                                    <option value="">Mô hình chat mặc định</option>
                                    <Show when={graphModel() && !usableChatModels().some((model) => model.name === graphModel())}>
                                      <option value={graphModel()}>{graphModel()} · hiện không khả dụng</option>
                                    </Show>
                                    <For each={usableChatModels()}>{(model) => (
                                      <option value={model.name}>{model.name}</option>
                                    )}</For>
                                  </select>
                                  <p>Chọn model nhỏ cho bước trích xuất thực thể. Model trả lời chat không bị thay đổi.</p>
                                </div>
                              </Show>
                            </div>
                          </section>
                          <section class="web-search-section">
                            <div>
                              <strong>Tìm kiếm web</strong>
                              <span>
                                Đây là tính năng duy nhất khiến câu hỏi rời khỏi máy này: nội dung tin nhắn
                                được gửi tới nguồn tìm kiếm bên dưới. Tài liệu, bộ nhớ và tri thức vẫn ở lại máy.
                              </span>
                              <label class="settings-checkbox">
                                <input
                                  type="checkbox"
                                  checked={webSearchEnabled()}
                                  disabled={preferences.loading || preferencesSaving()}
                                  onChange={(event) => void toggleWebSearch(event.currentTarget.checked)}
                                />
                                <span>{webSearchEnabled() ? "Đang bật" : "Đang tắt"}</span>
                              </label>
                            </div>
                            <div class="web-search-config">
                              <div class="segmented-control settings-control" role="group" aria-label="Nguồn tìm kiếm web">
                                <For each={webSearchBackends}>{(backend) => (
                                  <button
                                    type="button"
                                    classList={{ active: webSearchBackend() === backend.id }}
                                    disabled={preferences.loading || preferencesSaving()}
                                    aria-pressed={webSearchBackend() === backend.id}
                                    onClick={() => void selectWebSearchBackend(backend.id)}
                                  >{backend.label}</button>
                                )}</For>
                              </div>
                              <p class="web-search-hint">
                                {webSearchBackends.find((item) => item.id === webSearchBackend())?.hint}
                              </p>
                              <Show when={webSearchBackend() === "searxng"}>
                                <label class="field-label" for="web-search-url">Địa chỉ SearXNG</label>
                                <input
                                  id="web-search-url"
                                  class="text-input"
                                  type="url"
                                  placeholder="http://127.0.0.1:8888"
                                  value={webSearchUrlDraft()}
                                  disabled={preferences.loading || preferencesSaving()}
                                  onInput={(event) => setWebSearchUrlDraft(event.currentTarget.value)}
                                  onChange={() => void saveWebSearchUrl()}
                                />
                                <p class="web-search-hint">
                                  SearXNG chỉ trả HTML cho tới khi bạn thêm <code>json</code> vào
                                  <code> search.formats</code> trong <code>settings.yml</code>.
                                </p>
                              </Show>
                              <Show when={webSearchBackend() === "openai"}>
                                <label class="field-label" for="web-search-key">OpenAI API key</label>
                                <div class="web-search-key-row">
                                  <input
                                    id="web-search-key"
                                    class="text-input"
                                    type="password"
                                    autocomplete="off"
                                    placeholder={preferences()?.web_search_has_api_key ? "Đã lưu một API key" : "sk-…"}
                                    value={webSearchKeyDraft()}
                                    disabled={preferences.loading || preferencesSaving()}
                                    onInput={(event) => setWebSearchKeyDraft(event.currentTarget.value)}
                                  />
                                  <button
                                    class="button button-secondary"
                                    type="button"
                                    disabled={!webSearchKeyDraft().trim() || preferencesSaving()}
                                    onClick={() => void saveWebSearchKey()}
                                  >Lưu key</button>
                                  <Show when={preferences()?.web_search_has_api_key}>
                                    <button
                                      class="button button-secondary"
                                      type="button"
                                      disabled={preferencesSaving()}
                                      onClick={() => void clearWebSearchKey()}
                                    >Xóa key</button>
                                  </Show>
                                </div>
                                <label class="field-label field-spaced" for="web-search-model">Mô hình chạy tìm kiếm</label>
                                <input
                                  id="web-search-model"
                                  class="text-input"
                                  placeholder="gpt-5"
                                  value={webSearchModelDraft()}
                                  disabled={preferences.loading || preferencesSaving()}
                                  onInput={(event) => setWebSearchModelDraft(event.currentTarget.value)}
                                  onChange={() => void saveWebSearchModel()}
                                />
                                <p class="web-search-hint">
                                  OpenAI tính khoảng 10 USD cho mỗi 1.000 lượt tìm, chưa kể token của nội dung trả về.
                                </p>
                              </Show>
                              <div class="web-search-actions">
                                <button
                                  class="button button-secondary"
                                  type="button"
                                  disabled={webSearchProbing() || preferences.loading}
                                  onClick={() => void runWebSearchProbe()}
                                >{webSearchProbing() ? "Đang kiểm tra…" : "Kiểm tra kết nối"}</button>
                                <Show when={webSearchProbe()}>
                                  <span
                                    classList={{ "field-status": Boolean(webSearchProbe()?.reachable), "field-error": !webSearchProbe()?.reachable }}
                                    role="status"
                                  >
                                    {webSearchProbe()?.reachable
                                      ? `${webSearchProbe()?.host} trả về ${webSearchProbe()?.result_count} kết quả` +
                                        (webSearchProbe()?.on_device ? " · chạy trên máy này" : " · dữ liệu rời khỏi máy")
                                      : webSearchProbe()?.detail ?? "Không kết nối được"}
                                  </span>
                                </Show>
                              </div>
                            </div>
                          </section>
                          <section>
                            <div>
                              <strong>Hiệu năng embedding</strong>
                              <span>Tăng dần nếu máy còn RAM/VRAM. Giá trị quá cao có thể làm mô hình chậm hoặc hết bộ nhớ.</span>
                            </div>
                            <div class="settings-number-grid">
                              <label for="embedding-batch-size">
                                <span>Kích thước lô</span>
                                <input
                                  id="embedding-batch-size"
                                  type="number"
                                  min="1"
                                  max="256"
                                  step="1"
                                  inputmode="numeric"
                                  value={embeddingBatchDraft()}
                                  disabled={preferences.loading || preferencesSaving()}
                                  aria-describedby="embedding-batch-help"
                                  onInput={(event) => setEmbeddingBatchDraft(event.currentTarget.value)}
                                  onChange={() => commitEmbeddingSetting("embedding_batch_size", embeddingBatchDraft())}
                                />
                                <small id="embedding-batch-help">1–256 đoạn mỗi lô</small>
                              </label>
                              <label for="embedding-concurrency">
                                <span>Tác vụ song song</span>
                                <input
                                  id="embedding-concurrency"
                                  type="number"
                                  min="1"
                                  max="32"
                                  step="1"
                                  inputmode="numeric"
                                  value={embeddingConcurrencyDraft()}
                                  disabled={preferences.loading || preferencesSaving()}
                                  aria-describedby="embedding-concurrency-help"
                                  onInput={(event) => setEmbeddingConcurrencyDraft(event.currentTarget.value)}
                                  onChange={() => commitEmbeddingSetting("embedding_concurrency", embeddingConcurrencyDraft())}
                                />
                                <small id="embedding-concurrency-help">1–32 yêu cầu đồng thời</small>
                              </label>
                            </div>
                          </section>
                          <section><div><strong>Nhà cung cấp đang dùng</strong><span>Nơi mô hình thực sự chạy cho phiên làm việc này.</span></div><div class="settings-control"><StatusPip state={health()?.services.provider ?? "offline"} /> {health.loading ? "Đang kiểm tra…" : (health()?.provider?.name ?? "Chưa cấu hình")}</div></section>
                        </div>
                      </Match>

                      <Match when={settingsTab() === "models"}>
                        <section class="settings-panel">
                          <div class="page-heading page-heading-row"><div><span>Mô hình cục bộ</span><h1>Quản lý mô hình</h1><p>Quản lý Ollama và ASR: trạng thái tải, VRAM, mặc định tác vụ và kiểm tra SHA-256.</p></div><AddModelDialog onCompleted={() => { refetchModels(); refetchHealth(); }} /></div>
                          <div class="model-list">
                            <Switch>
                              <Match when={models.loading}>
                                <div class="loading-row"><i />Đang đọc thư viện mô hình…</div>
                              </Match>
                              <Match when={models.error || (models()?.length ?? 0) === 0}>
                                <div class="empty-models">
                                  <HardDrive size={28} />
                                  <strong>Chưa tìm thấy mô hình</strong>
                                  <span>Khởi động Ollama rồi thêm mô hình đầu tiên.</span>
                                </div>
                              </Match>
                              <Match when={(models()?.length ?? 0) > 0}>
                                <For each={models()}>{(model) => (
                                  <ModelRow model={model} onRefresh={() => {
                                    refetchModels();
                                    refetchHealth();
                                  }} />
                                )}</For>
                              </Match>
                            </Switch>
                          </div>
                        </section>
                      </Match>

                      <Match when={settingsTab() === "memory"}>
                        <MemoryView embedded profileId={activeProfile()?.id} />
                      </Match>

                      <Match when={settingsTab() === "providers"}>
                        <ProviderSettings onChanged={() => { refetchModels(); refetchHealth(); }} />
                      </Match>
                    </Switch>
                  </div>
                </div>
              </section>
            </Match>
          </Switch>
        </main>
        <Show when={sidebarOpen()}><button class="sidebar-scrim" onClick={() => setSidebarOpen(false)} aria-label="Đóng menu" /></Show>
      </div>
      <Show when={viewingDocument()}>
        <DocumentViewer
          documentId={viewingDocument()}
          onClose={() => setViewingDocument("")}
          onChanged={() => refetchDocuments()}
        />
      </Show>
      <ProfileNameDialog
        open={needsOnboarding()}
        mode="onboarding"
        profile={activeProfile()}
        onClose={() => {
          setOnboardingDismissed(true);
          window.localStorage.setItem("private-ai-onboarding-dismissed", "1");
          refetchProfiles();
        }}
        onDone={() => refetchProfiles()}
      />
      <UploadDialog
        open={uploadOpen()}
        workspaceId={activeWorkspace()}
        workspaceName={hasWorkspace() ? currentWorkspace().name : ""}
        defaultOcr={false}
        initialFiles={stagedFiles()}
        onClose={() => setUploadOpen(false)}
        onCompleted={({ uploaded, ready, failed, pending }) => {
          if (uploaded || ready || failed || pending) {
            refetchDocuments();
          }
          if (failed) {
            notify({
              tone: "error",
              title: "Có tài liệu xử lý lỗi",
              description: `${ready} tệp đã sẵn sàng, ${failed} tệp cần xem lại.`,
              duration: 6_000,
            });
          } else if (pending) {
            notify({
              tone: "info",
              title: `${pending} tài liệu vẫn đang xử lý`,
              description: "Bạn có thể theo dõi tiếp trong Thư viện và thanh thông báo.",
              duration: 6_000,
            });
          } else {
            notify({
              tone: "success",
              title: `Đã xử lý ${ready} tài liệu`,
              description: "Nội dung đã sẵn sàng trong thư viện.",
            });
          }
        }}
      />
      <ToastViewport />
    </>
  );
}

export default App;
