import type {
  EmbeddingProbe,
  EmbeddingSetting,
  ModelChoice,
  Provider,
  ProviderInput,
  ProviderPreset,
  ProviderProbe,
} from "../protocol";

/** Fake providers for `?demo=1`, chosen by hard state: remote-with-key on chat, local on embedding, one disabled,
 * one remote without a key, and a model list containing `tools: false`. The cross-wired pairing is the default,
 * and the store is a mutable array so the click-save-reload loop actually happens. */

let store: Provider[] | null = null;

function seed(): Provider[] {
  return [
    {
      id: "pv-ollama",
      name: "Ollama",
      kind: "ollama",
      baseUrl: "http://127.0.0.1:11434",
      hasKey: false,
      enabled: true,
      onDevice: true,
      activeChat: false,
      activeEmbedding: true,
      activeVision: true,
      model: "qwen2.5-coder:14b",
      embeddingModel: "nomic-embed-text",
      visionModel: "gemma3:12b",
    },
    {
      id: "pv-openai",
      name: "OpenAI",
      kind: "openai",
      baseUrl: "https://api.openai.com/v1",
      hasKey: true,
      enabled: true,
      onDevice: false,
      activeChat: true,
      activeEmbedding: false,
      activeVision: false,
      model: "gpt-4o-mini",
      // A stored embedding model without the embedding role: the field means "what to use *if* given the role".
      embeddingModel: "text-embedding-3-small",
      visionModel: null,
    },
    {
      id: "pv-lmstudio",
      name: "LM Studio",
      kind: "openai",
      baseUrl: "http://127.0.0.1:1234/v1",
      hasKey: false,
      enabled: false,
      onDevice: true,
      activeChat: false,
      activeEmbedding: false,
      activeVision: false,
      model: null,
      embeddingModel: null,
      visionModel: null,
    },
    // Remote without a key: the only row where the form opens with an *empty* key field rather than "set".
    {
      id: "pv-vllm",
      name: "vLLM nội bộ",
      kind: "openai",
      baseUrl: "http://10.0.4.12:8000/v1",
      hasKey: false,
      enabled: true,
      onDevice: false,
      activeChat: false,
      activeEmbedding: false,
      activeVision: false,
      model: null,
      embeddingModel: null,
      visionModel: null,
    },
  ];
}

function all(): Provider[] {
  store ??= seed();
  return store;
}

export function demoProviders(): Provider[] {
  return all().map((entry) => ({ ...entry }));
}

const OLLAMA_MODELS: ModelChoice[] = [
  { id: "qwen2.5-coder:14b", tools: true, chat: true, embedding: false, contextWindow: 32768 },
  { id: "qwen2.5-coder:32b", tools: true, chat: true, embedding: false, contextWindow: 32768 },
  // A model that cannot call tools: the *only* state the model picker must warn about.
  { id: "gemma3:12b", tools: false, chat: true, embedding: false, contextWindow: 8192 },
  { id: "llama3.2:3b", tools: false, chat: true, embedding: false, contextWindow: 131072 },
  // Embedding-only, so the chat picker must hide it; a real Ollama server returns both roles in one list.
  { id: "embeddinggemma:latest", tools: false, chat: false, embedding: true, contextWindow: 2048 },
  // Both embedding and chat: not filtered, and the reason the rule is `embedding && !chat` rather than `chat`.
  { id: "nomic-embed-text", tools: false, chat: true, embedding: true, contextWindow: 8192 },
];

const OPENAI_MODELS: ModelChoice[] = [
  { id: "gpt-4o-mini", tools: true, chat: true, embedding: false, contextWindow: 128000 },
  { id: "gpt-4o", tools: true, chat: true, embedding: false, contextWindow: 128000 },
  { id: "o3-mini", tools: true, chat: true, embedding: false, contextWindow: 200000 },
  { id: "text-embedding-3-small", tools: false, chat: false, embedding: true, contextWindow: 8191 },
];

/** The same list as `probe_provider` sees it: every `tools` flag cleared. */
function probed(models: ModelChoice[]): ModelChoice[] {
  return models.map((entry) => ({ ...entry, tools: false }));
}

/** Models of the active provider, the fixture for `list_models` and the only source here with true `tools` flags. */
export function demoActiveModels(): ModelChoice[] {
  const entry = all().find((provider) => provider.activeChat) ?? null;
  if (entry === null || !entry.enabled) return [];
  return entry.kind === "ollama" ? [...OLLAMA_MODELS] : [...OPENAI_MODELS];
}

/** Models of any provider, the fixture for `provider_models`; the two empty rows mean "unreachable", not "none". */
export function demoProviderModels(providerId: string): ModelChoice[] {
  const entry = all().find((provider) => provider.id === providerId) ?? null;
  if (entry === null) return [];
  const url = entry.baseUrl;
  if (url.includes(":8000") || url.includes(":1234")) return [];
  return entry.kind === "ollama" ? [...OLLAMA_MODELS] : [...OPENAI_MODELS];
}

/** Three kinds of failure with three different messages, each implying a different fix; `tools` is always `false`,
 * exactly as the core reports it, so the UI is not built around a flag a probe never provides. */
export function demoProbeProvider(input: ProviderInput): ProviderProbe {
  const url = input.baseUrl.trim();
  if (url === "") {
    return { ok: false, message: "Base URL đang trống — chưa biết gọi tới đâu.", models: [] };
  }
  if (url.includes(":8000")) {
    return {
      ok: false,
      message: `Không nối được tới ${url}: connection refused sau 5,0 giây. Máy chủ chưa chạy, hoặc cổng bị tường lửa chặn.`,
      models: [],
    };
  }
  if (url.includes(":1234")) {
    return {
      ok: true,
      message: "Nối được, nhưng máy chủ chưa nạp mô hình nào. Mở LM Studio và tải một mô hình về trước.",
      models: [],
    };
  }
  if (url.includes("api.openai.com")) {
    // `apiKey === ""` clears the key and `null` keeps it; only the first yields a 401.
    if (input.apiKey !== null && input.apiKey.trim() === "") {
      return {
        ok: false,
        message: "Máy chủ trả 401: khoá API bị từ chối. Kiểm tra lại khoá ở trang API keys của OpenAI.",
        models: [],
      };
    }
    return { ok: true, message: "Nối được. Tìm thấy 3 mô hình.", models: probed(OPENAI_MODELS) };
  }
  return {
    ok: true,
    message: `Nối được. Tìm thấy ${OLLAMA_MODELS.length} mô hình.`,
    models: probed(OLLAMA_MODELS),
  };
}

export function demoSaveProvider(input: ProviderInput): Provider {
  const list = all();
  // The fixture stands in for the *core*, so it may compute `onDevice`; the UI may not, since that badge is a promise.
  const onDevice = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1])/i.test(input.baseUrl.trim());
  const at = input.id === null ? -1 : list.findIndex((entry) => entry.id === input.id);
  const previous = at < 0 ? null : list[at]!;

  const saved: Provider = {
    id: previous?.id ?? `pv-${Date.now().toString(36)}`,
    name: input.name,
    kind: input.kind,
    baseUrl: input.baseUrl,
    // The contract's key rule, reproduced exactly: `null` keeps, `""` clears, anything else sets.
    hasKey: input.apiKey === null ? (previous?.hasKey ?? false) : input.apiKey.trim() !== "",
    enabled: input.enabled,
    onDevice,
    // The two roles are set by `set_active_provider` and `set_embedding`, never by the form.
    activeChat: previous?.activeChat ?? false,
    activeEmbedding: previous?.activeEmbedding ?? false,
    activeVision: previous?.activeVision ?? (input.visionModel !== null && !list.some((entry) => entry.activeVision)),
    model: input.model,
    embeddingModel: input.embeddingModel,
    visionModel: input.visionModel,
  };

  if (at < 0) list.push(saved);
  else list[at] = saved;
  return { ...saved };
}

export function demoRemoveProvider(id: string): void {
  store = all().filter((entry) => entry.id !== id);
}

/** Sets the *chat* role only; the embedding role goes through `demoSetEmbedding`. */
export function demoSetActiveProvider(id: string): void {
  for (const entry of all()) entry.activeChat = entry.id === id;
}

export function demoSetProviderModel(id: string, model: string): void {
  const hit = all().find((entry) => entry.id === id);
  if (hit) hit.model = model;
}

/** Real dimensions of common embedding models, so the fixture never invents a round number as evidence. */
const EMBEDDING_DIMS: Record<string, number> = {
  "nomic-embed-text": 768,
  "mxbai-embed-large": 1024,
  "bge-m3": 1024,
  "all-minilm": 384,
  "text-embedding-3-small": 1536,
  "text-embedding-3-large": 3072,
};

export function demoEmbeddingSetting(): EmbeddingSetting {
  const entry = all().find((provider) => provider.activeEmbedding) ?? null;
  if (entry === null) {
    return {
      providerId: null,
      providerName: null,
      model: null,
      onDevice: false,
      reason: "Chưa giao vai nhúng cho provider nào.",
    };
  }
  const base = {
    providerId: entry.id,
    providerName: entry.name,
    model: entry.embeddingModel,
    onDevice: entry.onDevice,
  };
  // `reason` says *why the config is unusable*, not a general note; all three branches name a config that cannot embed.
  if (!entry.enabled) return { ...base, reason: `${entry.name} đang bị tắt.` };
  if (entry.embeddingModel === null) {
    return { ...base, reason: `Chưa chọn mô hình nhúng cho ${entry.name}.` };
  }
  return { ...base, reason: null };
}

export function demoSetEmbedding(providerId: string, model: string): void {
  for (const entry of all()) {
    entry.activeEmbedding = entry.id === providerId;
    if (entry.id === providerId) entry.embeddingModel = model;
  }
}

/** Actually embed one sentence; the best case here is the last branch, a chat model typed into the embedding field. */
export function demoProbeEmbedding(providerId: string, model: string): EmbeddingProbe {
  const entry = all().find((provider) => provider.id === providerId) ?? null;
  if (entry === null) {
    return { ok: false, message: "Không tìm thấy provider này nữa.", dimensions: null };
  }
  if (!entry.enabled) {
    return {
      ok: false,
      message: `${entry.name} đang bị tắt, nên không gọi thử được.`,
      dimensions: null,
    };
  }
  const name = model.trim();
  if (name === "") {
    return { ok: false, message: "Chưa điền tên mô hình nhúng.", dimensions: null };
  }
  if (entry.baseUrl.includes(":8000")) {
    return {
      ok: false,
      message: `Không nối được tới ${entry.baseUrl}: connection refused sau 5,0 giây.`,
      dimensions: null,
    };
  }
  const dims = EMBEDDING_DIMS[name];
  if (dims === undefined) {
    return {
      ok: false,
      message: `Máy chủ nhận ra "${name}" nhưng không trả về vector nào — đây không phải mô hình nhúng. Danh sách mô hình vẫn liệt kê nó, vì danh sách đó không nói mô hình nào nhúng được.`,
      dimensions: null,
    };
  }
  return {
    ok: true,
    message: `Đã nhúng thử một câu bằng "${name}" và nhận về một vector.`,
    dimensions: dims,
  };
}

export function demoProviderPresets(): ProviderPreset[] {
  return [
    {
      id: "ollama",
      name: "Ollama",
      kind: "ollama",
      baseUrl: "http://127.0.0.1:11434",
      needsKey: false,
      onDevice: true,
      defaultModel: "qwen2.5-coder:14b",
      homepage: "https://ollama.com",
      hint: "Chạy mô hình ngay trên máy của bạn.",
    },
    {
      id: "lmstudio",
      name: "LM Studio",
      kind: "openai",
      baseUrl: "http://127.0.0.1:1234/v1",
      needsKey: false,
      onDevice: true,
      defaultModel: null,
      homepage: "https://lmstudio.ai",
      hint: "Giao diện tải mô hình về máy, kèm máy chủ OpenAI.",
    },
    {
      id: "llamacpp",
      name: "llama.cpp",
      kind: "openai",
      baseUrl: "http://127.0.0.1:8080/v1",
      needsKey: false,
      onDevice: true,
      defaultModel: null,
      homepage: "https://github.com/ggml-org/llama.cpp",
      hint: "Nhẹ nhất nhóm tại chỗ, nhưng phải tự nạp GGUF.",
    },
    {
      id: "vllm",
      name: "vLLM",
      kind: "openai",
      baseUrl: "http://localhost:8000/v1",
      needsKey: false,
      onDevice: false,
      defaultModel: null,
      homepage: "https://docs.vllm.ai",
      hint: "Máy chủ GPU, thường đặt trên máy khác trong mạng.",
    },
    {
      id: "openai",
      name: "OpenAI",
      kind: "openai",
      baseUrl: "https://api.openai.com/v1",
      needsKey: true,
      onDevice: false,
      defaultModel: "gpt-4o-mini",
      homepage: "https://platform.openai.com/api-keys",
      hint: "Mã nguồn và câu hỏi được gửi tới máy chủ OpenAI.",
    },
    {
      id: "anthropic",
      name: "Anthropic",
      kind: "openai",
      baseUrl: "https://api.anthropic.com/v1",
      needsKey: true,
      onDevice: false,
      defaultModel: "claude-sonnet-4-5",
      homepage: "https://console.anthropic.com/settings/keys",
      hint: "Mã nguồn và câu hỏi được gửi tới máy chủ Anthropic.",
    },
    {
      id: "openrouter",
      name: "OpenRouter",
      kind: "openai",
      baseUrl: "https://openrouter.ai/api/v1",
      needsKey: true,
      onDevice: false,
      defaultModel: "anthropic/claude-sonnet-4.5",
      homepage: "https://openrouter.ai/keys",
      hint: "Một khoá cho nhiều provider, đi qua máy chủ OpenRouter.",
    },
    {
      id: "deepseek",
      name: "DeepSeek",
      kind: "openai",
      baseUrl: "https://api.deepseek.com/v1",
      needsKey: true,
      onDevice: false,
      defaultModel: "deepseek-chat",
      homepage: "https://platform.deepseek.com/api_keys",
      hint: "Mã nguồn và câu hỏi được gửi tới máy chủ DeepSeek.",
    },
    {
      id: "groq",
      name: "Groq",
      kind: "openai",
      baseUrl: "https://api.groq.com/openai/v1",
      needsKey: true,
      onDevice: false,
      defaultModel: "llama-3.3-70b-versatile",
      homepage: "https://console.groq.com/keys",
      hint: "Rất nhanh, nhưng ít mô hình gọi được tool.",
    },
    {
      id: "xai",
      name: "xAI",
      kind: "openai",
      baseUrl: "https://api.x.ai/v1",
      needsKey: true,
      onDevice: false,
      defaultModel: "grok-4",
      homepage: "https://console.x.ai",
      hint: "Mã nguồn và câu hỏi được gửi tới máy chủ xAI.",
    },
  ];
}
