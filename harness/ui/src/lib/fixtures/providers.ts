import type { ModelChoice, Provider, ProviderInput, ProviderPreset, ProviderProbe } from "../protocol";

/**
 * Provider giả cho `?demo=1`.
 *
 * Bộ mẫu được chọn theo *trạng thái khó*, không theo "trông giống thật": một provider
 * chạy tại chỗ đang hoạt động, một provider từ xa đã có khoá, một provider đang tắt, một
 * provider từ xa **chưa** có khoá, và một danh sách mô hình có mục `tools: false`. Trạng
 * thái không nằm trong bộ mẫu là trạng thái chưa ai nhìn thấy bao giờ — nó chỉ xuất hiện
 * lần đầu trên máy người dùng, và ở đó thì không ai đang nhìn.
 *
 * Kho là một mảng **có thể sửa**, không phải một hằng số trả về bản sao mới mỗi lần. Một
 * trang demo mà bấm bật/tắt xong không có gì đổi thì không dựng lại được cái vòng thật:
 * bấm → lưu → nạp lại danh sách.
 */

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
      active: true,
      model: "qwen2.5-coder:14b",
    },
    {
      id: "pv-openai",
      name: "OpenAI",
      kind: "openai",
      baseUrl: "https://api.openai.com/v1",
      hasKey: true,
      enabled: true,
      onDevice: false,
      active: false,
      model: "gpt-4o-mini",
    },
    {
      id: "pv-lmstudio",
      name: "LM Studio",
      kind: "openai",
      baseUrl: "http://127.0.0.1:1234/v1",
      hasKey: false,
      enabled: false,
      onDevice: true,
      active: false,
      model: null,
    },
    // Từ xa mà chưa có khoá: hàng duy nhất mà biểu mẫu phải mở ra với ô khoá *trống*
    // thay vì mở ra với chữ "đã đặt". Không có nó thì nhánh đó không bao giờ được nhìn.
    {
      id: "pv-vllm",
      name: "vLLM nội bộ",
      kind: "openai",
      baseUrl: "http://10.0.4.12:8000/v1",
      hasKey: false,
      enabled: true,
      onDevice: false,
      active: false,
      model: null,
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
  { id: "qwen2.5-coder:14b", tools: true, contextWindow: 32768 },
  { id: "qwen2.5-coder:32b", tools: true, contextWindow: 32768 },
  // Mô hình không gọi được tool. Đây là trạng thái *duy nhất* mà bộ chọn mô hình phải
  // cảnh báo, nên nó phải có mặt ở đây, cạnh những mô hình bình thường.
  { id: "gemma3:12b", tools: false, contextWindow: 8192 },
  { id: "llama3.2:3b", tools: false, contextWindow: 131072 },
];

const OPENAI_MODELS: ModelChoice[] = [
  { id: "gpt-4o-mini", tools: true, contextWindow: 128000 },
  { id: "gpt-4o", tools: true, contextWindow: 128000 },
  { id: "o3-mini", tools: true, contextWindow: 200000 },
];

/** Cùng một danh sách, nhưng qua con mắt của `probe_provider`: cờ `tools` bị gạt sạch. */
function probed(models: ModelChoice[]): ModelChoice[] {
  return models.map((entry) => ({ ...entry, tools: false }));
}

/**
 * Mô hình của provider đang hoạt động — bản mẫu của `list_models`.
 *
 * Đây là **nguồn duy nhất** trong bộ mẫu mang cờ `tools` đúng, y như phía lõi: chỉ
 * `list_models` mới thật sự hỏi từng mô hình. Bộ chọn mô hình đọc từ đây, nên `tools:
 * false` phải có mặt ở đây chứ không phải ở kết quả thử.
 */
export function demoActiveModels(): ModelChoice[] {
  const entry = all().find((provider) => provider.active) ?? null;
  if (entry === null || !entry.enabled) return [];
  return entry.kind === "ollama" ? [...OLLAMA_MODELS] : [...OPENAI_MODELS];
}

/**
 * Ba kiểu hỏng, ba câu khác nhau.
 *
 * "Không nối được", "nối được nhưng khoá bị từ chối" và "nối được nhưng chưa có mô hình
 * nào" dẫn tới ba việc phải làm khác hẳn nhau. Lõi thật phân biệt ba cái đó trong
 * `message`; bộ mẫu phải dựng lại đủ cả ba, nếu không thì màn hình chỉ được kiểm với
 * đúng một câu.
 *
 * `tools` trả về **luôn `false`**, đúng như lõi: một lần thử không trả tiền hỏi năng lực
 * gọi tool của từng mô hình. Bản mẫu nói dối chỗ này thì giao diện sẽ được dựng quanh
 * một cờ nó không thật sự có.
 */
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
    // `apiKey === ""` là *xoá khoá*, `null` là giữ nguyên khoá đã lưu — và chỉ trường hợp
    // đầu mới cho ra 401. Đúng phân biệt đó là thứ biểu mẫu dễ làm sai nhất.
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
  // Bộ mẫu đứng thay **lõi**, nên nó được phép tính `onDevice`. Giao diện thì không: huy
  // hiệu "chạy trên máy này" là một lời hứa về quyền riêng tư, và hai chỗ cùng suy ra nó
  // là hai chỗ sẽ lệch nhau sau lần sửa lõi đầu tiên.
  const onDevice = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1])/i.test(input.baseUrl.trim());
  const at = input.id === null ? -1 : list.findIndex((entry) => entry.id === input.id);
  const previous = at < 0 ? null : list[at]!;

  const saved: Provider = {
    id: previous?.id ?? `pv-${Date.now().toString(36)}`,
    name: input.name,
    kind: input.kind,
    baseUrl: input.baseUrl,
    // Luật khoá của hợp đồng, dựng lại nguyên vẹn: `null` giữ nguyên, `""` xoá, chuỗi
    // khác là đặt mới. Bản mẫu làm sai chỗ này thì biểu mẫu "chạy đúng" trong demo và
    // làm mất khoá của người dùng ở lần chạy thật đầu tiên.
    hasKey: input.apiKey === null ? (previous?.hasKey ?? false) : input.apiKey.trim() !== "",
    enabled: input.enabled,
    onDevice,
    active: previous?.active ?? false,
    model: input.model,
  };

  if (at < 0) list.push(saved);
  else list[at] = saved;
  return { ...saved };
}

export function demoRemoveProvider(id: string): void {
  store = all().filter((entry) => entry.id !== id);
}

export function demoSetActiveProvider(id: string): void {
  for (const entry of all()) entry.active = entry.id === id;
}

export function demoSetProviderModel(id: string, model: string): void {
  const hit = all().find((entry) => entry.id === id);
  if (hit) hit.model = model;
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
      hint: "Chạy mô hình ngay trên máy. Cài xong thì `ollama pull qwen2.5-coder:14b`.",
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
      hint: "Giao diện tải mô hình về máy, có sẵn máy chủ tương thích OpenAI.",
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
      hint: "`llama-server` nhẹ nhất trong nhóm chạy tại chỗ, nhưng phải tự nạp tệp GGUF.",
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
      hint: "Máy chủ suy luận cho GPU. Thường đặt trên một máy khác trong mạng nội bộ.",
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
      hint: "Mã nguồn và câu hỏi của bạn được gửi tới máy chủ OpenAI.",
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
      hint: "Mã nguồn và câu hỏi của bạn được gửi tới máy chủ Anthropic.",
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
      hint: "Một khoá đi tới nhiều nhà cung cấp. Yêu cầu của bạn đi qua máy chủ OpenRouter.",
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
      hint: "Mã nguồn và câu hỏi của bạn được gửi tới máy chủ DeepSeek.",
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
      hint: "Suy luận rất nhanh, nhưng danh sách mô hình gọi được tool khá hẹp.",
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
      hint: "Mã nguồn và câu hỏi của bạn được gửi tới máy chủ xAI.",
    },
  ];
}
