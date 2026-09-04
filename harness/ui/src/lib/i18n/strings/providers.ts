import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `providers` area. See lib/i18n/README.md for the wording rules. */
export const providers = {
  // Page header
  title: { en: "Models", vi: "Mô hình" },
  desc: {
    en: "Chat, embedding, and reranking.",
    vi: "Cấu hình hội thoại, nhúng và xếp hạng lại.",
  },
  more: {
    en: "Each tab owns one model role. Switching tabs keeps unfinished fields intact until you return.",
    vi: "Mỗi tab quản lý một vai trò mô hình. Chuyển tab vẫn giữ nguyên dữ liệu đang nhập để bạn quay lại.",
  },
  actionFailed: common.actionFailed,

  tabs: {
    label: { en: "Model settings", vi: "Các nhóm cấu hình mô hình" },
    chat: common.chat,
    embedding: common.embedding,
    vision: common.vision,
    rerank: common.rerank,
  },

  /** API kinds. Ollama and LM Studio are proper nouns and stay untranslated. */
  kind: {
    openai: { en: "OpenAI-compatible", vi: "Tương thích OpenAI" },
  },

  /** Prefix for an error shown after a click. */
  err: {
    activate: { en: "Could not switch chat provider", vi: "Không đổi được nhà cung cấp đang dùng" },
    toggle: { en: "Could not change state", vi: "Không đổi được trạng thái" },
    pickModel: { en: "Could not select model", vi: "Không chọn được mô hình" },
    remove: { en: "Could not delete provider", vi: "Không xoá được nhà cung cấp" },
  },

  /** Delete dialog; not shortened, since the action is irreversible. */
  del: {
    title: { en: "Delete {name}?", vi: "Xoá {name}?" },
    body: {
      en: "Permanently removes the config and the API key from this device.",
      vi: "Xoá vĩnh viễn cấu hình và khoá API khỏi máy.",
    },
    more: {
      en: "The config and the API key of this provider are removed from this device. This cannot be undone.",
      vi: "Cấu hình và khoá API của nhà cung cấp này bị xoá khỏi máy. Thao tác không hoàn tác được.",
    },
    confirm: { en: "Delete provider", vi: "Xoá nhà cung cấp" },
  },

  /** The built-in provider catalogue. */
  catalog: {
    headingEmpty: { en: "Get started", vi: "Chọn một nhà cung cấp để bắt đầu" },
    heading: { en: "Add provider", vi: "Thêm nhà cung cấp" },
    aboutLabel: { en: "About the provider catalog", vi: "Về danh mục nhà cung cấp" },
    aboutText: {
      en: "The first entries run on this device: your code and your questions go nowhere. Remote services sit behind the show-more button — faster and stronger, but everything you send leaves this device. For a server that is not listed, use Other server.",
      vi: "Những mục đầu chạy ngay trên máy này: mã nguồn và câu hỏi của bạn không đi đâu cả. Các dịch vụ từ xa nằm sau nút xem thêm — chúng nhanh và mạnh hơn, nhưng mọi thứ bạn gửi đều rời khỏi máy. Máy chủ không có trong danh sách thì dùng mục Máy chủ khác.",
    },
    rowMore: { en: "{hint} Default address {url}.", vi: "{hint} Địa chỉ mặc định {url}." },
    rowMoreKey: {
      en: "{hint} Default address {url}. Needs an API key.",
      vi: "{hint} Địa chỉ mặc định {url}. Cần khoá API.",
    },
    added: { en: "Added", vi: "đã thêm" },
    connect: { en: "Connect", vi: "Kết nối" },
    otherLabel: { en: "Other server", vi: "Máy chủ khác" },
    otherMore: {
      en: "For servers not in the list: a self-hosted llama.cpp, an internal relay port, or another OpenAI-compatible service. You fill in the name, the API type and the address yourself.",
      vi: "Dùng cho máy chủ không có trong danh sách: llama.cpp tự dựng, một cổng trung chuyển nội bộ, hay một dịch vụ tương thích OpenAI khác. Bạn tự điền tên, loại API và địa chỉ.",
    },
    otherBadge: { en: "Custom", vi: "tuỳ chỉnh" },
    otherAction: { en: "Set up", vi: "Khai báo" },
    showRemote: { en: "Show {n} remote services", vi: "Xem thêm {n} dịch vụ từ xa" },
  },

  /** Warning banner above the list. */
  notice: {
    noProviderTitle: { en: "No chat provider", vi: "Chưa chọn nhà cung cấp để trò chuyện" },
    noProviderMore: {
      en: 'The assistant cannot call any model yet. Press "Use for chat" on a row below.',
      vi: 'Trợ lý chưa gọi được mô hình nào. Bấm "Dùng để trò chuyện" ở một hàng bên dưới.',
    },
    noProviderBody: {
      en: "Pick one in the list below.",
      vi: 'Bấm "Dùng để trò chuyện" ở một hàng bên dưới.',
    },
    disabledTitle: {
      en: "Provider off",
      vi: "Nhà cung cấp đang dùng để trò chuyện lại bị tắt",
    },
    disabledBody: {
      en: "Turn it on, or pick another.",
      vi: "Bật nó lên, hoặc giao vai cho provider khác.",
    },
    noModelTitle: { en: "No chat model", vi: "Chưa chọn mô hình hội thoại" },
    noModelBody: {
      en: "Pick a model below.",
      vi: "Chọn mô hình ở hàng provider đang giữ vai.",
    },
    noToolsTitle: { en: "No tools", vi: "Mô hình đang chọn không gọi được tool" },
    noToolsMore: {
      en: "{model} still answers, but it reads no files, edits no code and runs no commands — every answer is a guess from memory. Pick a model with tools if you need work done inside the project.",
      vi: "{model} vẫn trả lời được, nhưng nó không đọc tệp, không sửa mã và không chạy lệnh — mọi câu trả lời sẽ là phỏng đoán từ trí nhớ. Chọn một mô hình có tool nếu bạn cần nó làm việc trong dự án.",
    },
    noToolsFallback: { en: "This model", vi: "Mô hình này" },
    noToolsBody: {
      en: "reads no files, edits no code, runs no commands.",
      vi: "không đọc tệp, không sửa mã, không chạy lệnh.",
    },
  },

  /** One provider row. */
  row: {
    onDevice: {
      en: "Runs on this device — data stays here.",
      vi: "Chạy trên máy này — dữ liệu không rời khỏi đây.",
    },
    remote: {
      en: "Remote server — everything you send leaves this device.",
      vi: "Máy chủ từ xa — mọi thứ bạn gửi đều rời khỏi máy này.",
    },
    useForChat: { en: "Use for chat", vi: "Dùng để trò chuyện" },
    turnOn: { en: "Turn on {name}", vi: "Bật {name}" },
    turnOff: { en: "Turn off {name}", vi: "Tắt {name}" },
    edit: common.editName,
    remove: { en: "Delete {name}", vi: "Xoá {name}" },
    leaves: { en: "Leaves device", vi: "Gửi ra ngoài" },
    noModel: { en: "no model", vi: "chưa chọn mô hình" },
    keyTitle: {
      en: "An API key is saved for this provider",
      vi: "Đã lưu khoá API cho nhà cung cấp này",
    },
    keyLabel: { en: "API key saved", vi: "Đã lưu khoá API" },
    roleEmbedding: { en: "Embedding documents with {model}", vi: "Đang nhúng tài liệu bằng {model}" },
    roleEmbeddingNone: { en: "no model chosen", vi: "mô hình chưa chọn" },
    roleVision: { en: "Reading scans with {model}", vi: "Đang đọc bản quét bằng {model}" },
    noRole: { en: "No role", vi: "Chưa có" },
  },

  /** One-line labels in the model picker; each combination is a whole message, since word order differs by language. */
  opt: {
    ctx: { en: "{id} · {n} tokens", vi: "{id} · {n} token" },
    noTools: { en: "{id} — no tools", vi: "{id} — không gọi được tool" },
    noToolsCtx: { en: "{id} — no tools · {n} tokens", vi: "{id} — không gọi được tool · {n} token" },
    chatOnlyEmbed: { en: "{id} · embedding only", vi: "{id} · chỉ dùng để nhúng" },
    chatOnlyEmbedCtx: {
      en: "{id} · {n} tokens · embedding only",
      vi: "{id} · {n} token · chỉ dùng để nhúng",
    },
    notEmbed: { en: "{id} · not an embedding model", vi: "{id} · không phải mô hình nhúng" },
    notVision: { en: "{id} · cannot see images", vi: "{id} · không đọc được ảnh" },
    none: common.unset,
    custom: { en: "Type another name…", vi: "Nhập tên khác…" },
  },

  /** Chat model picker on the row that holds the role. */
  picker: {
    chatModel: { en: "Model used for chat", vi: "Mô hình dùng để trò chuyện" },
    loading: common.loadingModels,
    unreadable: {
      en: "Could not read the model list from this server.",
      vi: "Không đọc được danh sách mô hình từ máy chủ này.",
    },
    unreadableLabel: { en: "More about the model list", vi: "Xem thêm về danh sách mô hình" },
    unreadableText: {
      en: 'Open "Edit" — the dialog asks the server again and says what it answered.',
      vi: 'Mở "Sửa" — hộp thoại tự hỏi lại máy chủ và nói ra nó trả lời gì.',
    },
    reload: { en: "Reload the model list", vi: "Nạp lại danh sách mô hình" },
  },

  /** Model field shared by both roles. */
  field: {
    about: common.about,
    notListed: { en: "The server does not list this model.", vi: "Máy chủ không khai mô hình này." },
    notListedLabel: {
      en: "About names missing from the list",
      vi: "Về tên mô hình không có trong danh sách",
    },
    notListedText: {
      en: 'This list comes from the server. A name outside it can still work — a self-hosted server often serves exactly one model and lists none — so this is a reminder, not a block. To be sure, press "Test now" in the embedding section: it really sends a sentence and measures the vector that comes back.',
      vi: "Danh sách này do máy chủ trả về. Một cái tên không nằm trong đó vẫn có thể chạy — máy chủ tự dựng thường phục vụ đúng một mô hình và không liệt kê ra — nên đây là lời nhắc, không phải cái chặn. Muốn chắc thì bấm “Thử ngay” ở mục nhúng: nó gửi thật một câu đi và đo vector nhận về.",
    },
    notVision: {
      en: "This server says that model cannot read images.",
      vi: "Máy chủ nói mô hình này không đọc được ảnh.",
    },
    notVisionLabel: { en: "About this warning", vi: "Về cảnh báo này" },
    notVisionText: {
      en: "The server reported its own capability list and this model is not in the seeing group, so OCR would fail on every page. Two models from the same family can differ here: only the build that ships the image part can read a scan. Pick a model marked as seeing, or check `ollama show <model>` for `vision`.",
      vi: "Máy chủ tự khai danh sách khả năng và mô hình này không nằm trong nhóm nhìn được ảnh, nên OCR sẽ hỏng ở từng trang. Cùng một họ mô hình vẫn có bản khác nhau: chỉ bản kèm phần đọc ảnh mới đọc được bản quét. Chọn mô hình có đánh dấu nhìn được, hoặc xem `ollama show <model>` có dòng `vision` không.",
    },
  },

  /** Add/edit dialog. */
  form: {
    titleEdit: common.editName,
    titleConnect: { en: "Connect to {name}", vi: "Kết nối tới {name}" },
    titleManual: { en: "Other server", vi: "Khai báo máy chủ khác" },
    desc: { en: "Base URL sets the destination.", vi: "Base URL quyết định dữ liệu đi tới đâu." },
    probing: common.testing,
    name: common.name,
    namePlaceholder: { en: "Local Ollama", vi: "Ollama trên máy" },
    kindLabel: { en: "API type", vi: "Loại API" },
    kindHint: {
      en: "LM Studio reports loaded models.",
      vi: "Mục LM Studio đọc được mô hình nào đang nạp.",
    },
    kindMore: {
      en: "LM Studio has its own entry because its catalog says which model is loaded and which can call tools; picking “OpenAI-compatible” for it loses that. llama.cpp, vLLM and most other servers speak the OpenAI dialect.",
      vi: "LM Studio có mục riêng vì kho mô hình của nó nói được mô hình nào đang nạp và gọi được công cụ; chọn “Tương thích OpenAI” cho nó thì mất phần đó. llama.cpp, vLLM và phần lớn máy chủ còn lại thì nói giọng OpenAI.",
    },
    baseUrl: common.baseUrl,
    baseUrlHint: {
      en: "Loopback keeps data on this device.",
      vi: "Loopback thì dữ liệu không rời khỏi máy này.",
    },
    baseUrlMore: {
      en: "The base URL decides where the data goes. On loopback it never leaves this device.",
      vi: "Base URL quyết định dữ liệu đi tới đâu. Loopback thì nó không rời khỏi máy này.",
    },
    keySet: { en: "Key set", vi: "Đã có khoá" },
    keyFrom: { en: "Get a key at", vi: "Lấy khoá ở" },
    enable: { en: "Enable this provider", vi: "Bật nhà cung cấp này" },
    enableHint: {
      en: "Stays listed but never called.",
      vi: "Tắt thì vẫn trong danh sách nhưng không được gọi.",
    },
    chatModel: common.chatModel,
    chatModelPlaceholder: {
      en: "type a name, or test to list",
      vi: "nhập tên mô hình, hoặc thử kết nối để chọn",
    },
    chatModelMore: {
      en: "It saves blank too, but without a model there is no chat.",
      vi: "Lưu được cả khi để trống, nhưng chưa chọn mô hình thì chưa trò chuyện được.",
    },
    blankOk: {
      en: "Blank still saves, but cannot chat.",
      vi: "Để trống vẫn lưu được, nhưng chưa trò chuyện được.",
    },
    noModels: { en: "Server listed no models.", vi: "Máy chủ chưa khai mô hình nào để chọn." },
    noModelsLabel: { en: "What to do with no models", vi: "Không có mô hình nào thì làm gì" },
    noModelsText: {
      en: "The server returned no models, so there is nothing to pick. Type the model name straight into the field above if you know what this server accepts.",
      vi: "Máy chủ không trả về mô hình nào, nên không có gì để chọn. Nhập thẳng tên mô hình vào ô trên nếu bạn biết máy chủ này nhận tên gì.",
    },
    embedModel: { en: "Embedding model", vi: "Mô hình nhúng của nhà cung cấp này" },
    embedHint: {
      en: "Only when this provider embeds.",
      vi: "Chỉ dùng khi nhà cung cấp này nhúng tài liệu.",
    },
    embedMore: {
      en: "It only matters if this provider is the one chosen to embed documents in the section below. Leaving it blank is fine.",
      vi: "Chỉ có tác dụng nếu nhà cung cấp này được chọn để nhúng tài liệu ở mục bên dưới. Để trống cũng được.",
    },
    visionModel: { en: "Vision model for OCR", vi: "Mô hình vision cho OCR" },
    visionPlaceholder: { en: "e.g. gemma3:12b", vi: "Ví dụ: gemma3:12b" },
    visionHint: {
      en: "Who actually reads scans is chosen in the Vision tab.",
      vi: "Chọn ai thật sự đọc bản quét ở tab Đọc ảnh.",
    },
    visionMore: {
      en: "Scanned PDF pages and image files are sent to this model for transcription. Leave blank when this provider cannot read images; the Vision tab is where the reader is chosen and tested.",
      vi: "Trang PDF quét và tệp ảnh được gửi tới mô hình này để chép lại chữ. Để trống nếu nhà cung cấp này không đọc được ảnh; tab Đọc ảnh là nơi chọn và thử mô hình đọc.",
    },
  },

  /** API key field: three states, each saying plainly what Save will do. */
  key: {
    title: common.apiKey,
    optional: {
      en: "— local servers rarely need one",
      vi: "— máy chủ chạy tại chỗ thường không cần",
    },
    isSet: { en: "Set", vi: "Đã đặt" },
    keepNote: {
      en: "Saving this form *keeps* the stored key.",
      vi: "Lưu biểu mẫu này sẽ *giữ nguyên* khoá đã lưu.",
    },
    whereLabel: { en: "Where the key is kept", vi: "Khoá được giữ ở đâu" },
    whereText: {
      en: "The key is stored on this device and never shown on screen again. Saving this form keeps it as it is.",
      vi: "Khoá được lưu trong máy và không hiện lại ra màn hình. Lưu biểu mẫu này sẽ giữ nguyên nó.",
    },
    replace: { en: "Replace key", vi: "Thay khoá" },
    clear: { en: "Delete key", vi: "Xoá khoá" },
    clearTitle: {
      en: "Saving will delete the stored key",
      vi: "Bấm Lưu sẽ xoá khoá đã lưu",
    },
    clearBody: {
      en: "*Nothing can be called* until a new key is set.",
      vi: "*Không gọi được* cho tới khi có khoá mới.",
    },
    undo: { en: "Undo", vi: "Hoàn tác" },
    newLabel: { en: "New key", vi: "Khoá mới" },
    label: { en: "Key", vi: "Khoá" },
    placeholderOptional: {
      en: "leave blank if the server does not ask",
      vi: "để trống nếu máy chủ không yêu cầu",
    },
    hintHad: {
      en: "Blank keeps the old key, it does not delete it.",
      vi: "Để trống là giữ khoá cũ, không phải xoá khoá.",
    },
    hintNew: {
      en: "Stored on this device, never read back.",
      vi: "Khoá được lưu trong máy, không đọc ngược ra được.",
    },
    more: {
      en: "A stored key is never shown on screen again, so this field only takes a new one. Leaving it blank and saving keeps the old key — this is not how a key is deleted.",
      vi: "Khoá đã lưu không bao giờ hiện lại ra màn hình, nên ô này chỉ nhận khoá mới. Để trống rồi bấm Lưu thì khoá cũ được giữ nguyên — đây không phải cách xoá khoá.",
    },
    keepOld: { en: "Keep key", vi: "Giữ khoá cũ" },
  },

  /** Connection probe results. */
  probe: {
    busy: { en: "Calling the server…", vi: "Đang thử gọi tới máy chủ…" },
    failedTitle: common.testFailed,
    okTitle: { en: "Server replied", vi: "Máy chủ trả lời" },
    badTitle: { en: "Not usable", vi: "Không dùng được" },
  },

  /** Preset hints keyed by preset `id`; the core's own `hint` is the fallback, so a new Rust preset still shows text. */
  presetHint: {
    ollama: {
      en: "Runs entirely on your machine: nothing leaves it. Install Ollama and pull a model first.",
      vi: "Chạy hoàn toàn trên máy bạn: không có gì rời khỏi đây. Cần cài Ollama và kéo mô hình về trước.",
    },
    lmstudio: {
      en: "Runs on your machine. Start the local server in LM Studio's Developer tab or nothing answers here.",
      vi: "Chạy trên máy bạn. Phải bật máy chủ cục bộ trong tab Developer của LM Studio thì địa chỉ này mới có ai trả lời.",
    },
    llamacpp: {
      en: "Runs on your machine. `llama-server` serves exactly one model — the one you passed at startup — so the model name here barely matters.",
      vi: "Chạy trên máy bạn. `llama-server` phục vụ đúng một mô hình — cái bạn truyền cho nó lúc khởi động — nên tên mô hình ở đây gần như không quan trọng.",
    },
    vllm: {
      en: "A server you run yourself, usually on a GPU machine. The model name must match what vLLM loaded at startup.",
      vi: "Máy chủ tự vận hành, thường trên một máy có GPU. Tên mô hình phải trùng đúng cái đã nạp lúc khởi động vLLM.",
    },
    openai: {
      en: "Paid, billed by usage. Everything you send leaves this machine.",
      vi: "Dịch vụ trả tiền theo lượng dùng. Mọi thứ bạn gửi đi đều rời khỏi máy này.",
    },
    anthropic: {
      en: "Goes through Anthropic's own OpenAI-compatible layer, so it takes most but not all of the native API. Paid.",
      vi: "Đi qua tầng tương thích OpenAI của chính Anthropic, nên nó nhận phần lớn nhưng không phải mọi tính năng của API gốc. Dịch vụ trả tiền.",
    },
    openrouter: {
      en: "One key for many providers. Model names need a vendor prefix, e.g. `anthropic/claude-sonnet-4.5`.",
      vi: "Một khoá dùng được nhiều nhà cung cấp. Tên mô hình phải có tiền tố hãng, ví dụ `anthropic/claude-sonnet-4.5`.",
    },
    deepseek: {
      en: "Paid, cheap. The servers sit outside your jurisdiction — weigh that against internal code.",
      vi: "Dịch vụ trả tiền, giá thấp. Máy chủ đặt ngoài lãnh thổ bạn — cân nhắc với mã nguồn nội bộ.",
    },
    groq: {
      en: "Very fast, but serves open models only and caps you by the minute.",
      vi: "Rất nhanh, nhưng chỉ phục vụ mô hình mở và có hạn mức theo phút.",
    },
    xai: {
      en: "Paid, sharing a key with the xAI console.",
      vi: "Dịch vụ trả tiền, dùng chung khoá với bảng điều khiển xAI.",
    },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
