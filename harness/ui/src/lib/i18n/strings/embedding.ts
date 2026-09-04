import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `embedding` area (embedding plus reranking). See lib/i18n/README.md. */
export const embedding = {
  // "Embedding" section header
  section: {
    title: common.embedding,
    desc: {
      en: "Turn docs into vectors.",
      vi: "Biến tài liệu thành vector để tìm theo ý nghĩa.",
    },
    more: {
      en: "The model that turns documents into vectors for meaning-based search. It is separate from the chat model and chosen here.",
      vi: "Mô hình biến tài liệu thành vector để tìm theo ý nghĩa. Nó tách hẳn khỏi mô hình trò chuyện, và chọn riêng ở đây.",
    },
    failed: common.actionFailed,
    setFailed: {
      en: "Could not set the embedding model: {detail}",
      vi: "Không đặt được mô hình nhúng: {detail}",
    },
  },

  // No provider yet
  empty: {
    text: {
      en: "No provider to embed with.",
      vi: "Chưa có nhà cung cấp nào để nhúng tài liệu.",
    },
    infoLabel: { en: "Where to add a provider", vi: "Thêm nhà cung cấp ở đâu" },
    infoText: {
      en: "Add a provider in the section above first — one that runs on this machine is enough, and it keeps documents here.",
      vi: "Thêm một nhà cung cấp ở mục trên trước — một cái chạy tại chỗ là đủ, và nó giữ tài liệu trong máy này.",
    },
  },

  // The effective configuration
  current: {
    unsetTitle: { en: "Not configured", vi: "Chưa cấu hình nhúng" },
    unsetInfoLabel: {
      en: "About the unconfigured state",
      vi: "Về trạng thái chưa cấu hình nhúng",
    },
    unsetInfoText: {
      en: "The document library still works, it just searches by keyword: you have to type words that appear in the document rather than ask by meaning. This is a normal state, not an error. Pick an embedding model below if you want to ask by meaning, and pick a provider that runs on this machine if the documents must not leave it.",
      vi: "Thư viện tài liệu vẫn dùng được, chỉ là nó tìm theo từ khoá: bạn phải nhập đúng chữ có trong tài liệu chứ chưa hỏi được theo ý. Đây là trạng thái bình thường, không phải lỗi. Chọn mô hình nhúng bên dưới nếu bạn muốn hỏi theo ý, và chọn một nhà cung cấp chạy tại chỗ nếu tài liệu không được rời khỏi máy.",
    },
    unsetBody: {
      en: "Docs still search by keyword.",
      vi: "Thư viện tài liệu vẫn tìm được bằng từ khoá.",
    },
    brokenTitle: { en: "Not working", vi: "Cấu hình nhúng chưa dùng được" },
    brokenMore: {
      en: "Until this is fixed, the document library only searches by keyword.",
      vi: "Chưa sửa xong thì thư viện tài liệu chỉ tìm được theo từ khoá.",
    },
    okTitle: { en: "In use", vi: "Đang nhúng bằng mô hình này" },
    okMoreOnDevice: {
      en: "Documents stay on this machine.",
      vi: "Tài liệu không rời khỏi máy này.",
    },
    okMoreRemote: {
      en: "Every document is sent in full to {provider} to be embedded.",
      vi: "Toàn văn mỗi tài liệu được gửi tới {provider} để nhúng.",
    },
    okBodyOnDevice: {
      en: "on {provider} — stays on this machine.",
      vi: "trên {provider} — không rời khỏi máy này.",
    },
    okBodyRemote: {
      en: "on {provider} — full text goes there.",
      vi: "trên {provider} — toàn văn gửi tới đó.",
    },
  },

  // Privacy line for the selected provider
  privacy: {
    remoteTitle: { en: "Leaves device", vi: "Tài liệu được gửi ra khỏi máy" },
    remoteMore: {
      en: "Embedding with {name} means every document you load is sent in full to {url}. Re-embedding the library sends all of them again.",
      vi: "Nhúng bằng {name} nghĩa là toàn văn mỗi tài liệu bạn nạp vào được gửi tới {url}. Nhúng lại cả thư viện thì gửi lại tất cả một lần nữa.",
    },
    remoteBody: {
      // The address sits *inside* the sentence; in other languages it does not come last.
      en: "*Every document* is sent in full to `{url}`.",
      vi: "*Toàn văn* mỗi tài liệu được gửi tới `{url}`.",
    },
    localTitle: { en: "On device", vi: "Chạy trên máy này" },
    localMore: {
      en: "The documents you load — contracts, records, private notes — are embedded right here and never leave this machine. No network request carries their contents away.",
      vi: "Tài liệu bạn nạp vào — hợp đồng, hồ sơ, ghi chú riêng — được nhúng ngay tại đây và không rời khỏi máy này. Không có yêu cầu mạng nào mang nội dung của chúng đi.",
    },
    localBody: {
      en: "Documents *never leave this machine*.",
      vi: "Tài liệu *không rời khỏi máy này*.",
    },
  },

  // Provider row
  provider: {
    desc: { en: "Where full text goes.", vi: "Nơi toàn văn tài liệu được gửi tới." },
    more: {
      en: "Where the full text of each document is sent to become a vector.",
      vi: "Nơi toàn văn tài liệu được gửi tới để biến thành vector.",
    },
    selectLabel: {
      en: "Provider used to embed documents",
      vi: "Nhà cung cấp dùng để nhúng tài liệu",
    },
    unset: common.unset,
    optionOff: { en: "{name} (off)", vi: "{name} (đang tắt)" },
    optionOnDevice: { en: "{name} · on device", vi: "{name} · trên máy này" },
  },

  // Model row
  model: {
    label: common.embeddingModel,
    desc: { en: "From the chosen provider.", vi: "Lấy thẳng từ máy chủ đã chọn." },
    more: {
      en: "The list comes straight from the chosen provider, with embedding models first. If the provider does not answer, this becomes a plain text box — still usable, just without suggestions.",
      vi: "Danh sách lấy thẳng từ máy chủ đã chọn, mô hình nhúng xếp lên trước. Máy chủ không trả lời thì ô này thành ô nhập tay — vẫn đặt được, chỉ là không còn gợi ý.",
    },
    fieldLabel: { en: "Model used to embed", vi: "Mô hình dùng để nhúng" },
  },

  // Where the model list came from
  source: {
    loading: common.loadingModels,
    unavailable: {
      en: "No model list — type it.",
      vi: "Không lấy được danh sách mô hình từ máy chủ — nhập tên mô hình nhúng vào ô trên.",
    },
    noneEmbeddable: {
      en: "{n} models, none embed.",
      vi: "Máy chủ có {n} mô hình, không mô hình nào nhúng được. Tải một mô hình nhúng về, hoặc nhập tên nếu bạn biết máy chủ có.",
    },
    someEmbeddable: {
      en: "{n} models, {k} embed.",
      vi: "Máy chủ có {n} mô hình, trong đó {k} mô hình nhúng được.",
    },
  },

  // Embed one sentence as a probe
  probe: {
    label: { en: "Test embed", vi: "Thử nhúng một câu" },
    desc: {
      en: "Send a sentence, measure vector.",
      vi: "Gửi thật một câu đi và đo vector nhận về.",
    },
    more: {
      en: "This test really sends a sentence and reports the number of dimensions in the vector that comes back. The list in the box above is only what the provider says it has — only a sentence going out and a vector coming back proves this model can embed.",
      vi: "Phép thử này gửi thật một câu đi và báo lại số chiều của vector nhận về. Danh sách ở ô trên mới chỉ là những gì máy chủ trả về — chỉ khi một câu đi qua và một vector quay về thì mới chắc mô hình này nhúng được.",
    },
    run: { en: "Test now", vi: "Thử ngay" },
    running: common.testing,
    busy: {
      en: "Embedding a test sentence…",
      vi: "Đang gửi một câu đi để nhúng thử…",
    },
    failedTitle: common.testFailed,
    okTitle: { en: "Works", vi: "Nhúng được" },
    notOkTitle: { en: "Failed", vi: "Không nhúng được" },
    dimsMore: {
      en: "This number is measured from a real vector, not read off a model list.",
      vi: "Đây là con số đo từ một vector thật, không phải từ một danh sách mô hình.",
    },
    dims: { en: "The vector has {n} dimensions.", vi: "Vector nhận về có {n} chiều." },
  },

  // Save
  apply: {
    willReembed: {
      en: "Saving re-embeds the whole library.",
      vi: "Lưu thay đổi sẽ nhúng lại toàn bộ thư viện.",
    },
    noReembed: {
      en: "No vectors yet, so nothing is re-embedded.",
      vi: "Chưa có vector nào nên không phải nhúng lại.",
    },
    save: { en: "Save model", vi: "Lưu mô hình nhúng" },
  },

  // Re-embed confirmation dialog
  confirm: {
    title: { en: "Re-embed library?", vi: "Nhúng lại toàn bộ thư viện?" },
    body: {
      en: "Every old vector is dropped and each document is embedded again.",
      vi: "Ứng dụng bỏ vector cũ và nhúng lại từng tài liệu.",
    },
    more: {
      en: "Changing the embedding model drops every old vector and embeds each document again with the new one. This is required: vectors from two models live in two different spaces, and comparing them gives a meaningless number that looks exactly like a meaningful one — that is, wrong search results with nothing to flag them. While re-embedding runs, the library still searches by keyword; only meaning-based search is missing for a while.",
      vi: "Đổi mô hình nhúng thì mọi vector cũ bị bỏ và từng tài liệu được nhúng lại bằng mô hình mới. Bắt buộc phải thế: vector của hai mô hình nằm ở hai không gian khác nhau, và đem so với nhau thì ra một con số vô nghĩa trông y hệt một con số có nghĩa — tức là kết quả tìm kiếm sai mà không có gì báo sai. Trong lúc nhúng lại, thư viện vẫn tìm được bằng từ khoá; chỉ phần tìm theo ý nghĩa là tạm thiếu.",
    },
    detailNow: { en: "Now:  {provider} · {model}", vi: "Đang dùng:  {provider} · {model}" },
    detailNext: { en: "Next: {provider} · {model}", vi: "Sẽ dùng:    {provider} · {model}" },
    confirmLabel: { en: "Re-embed", vi: "Đổi và nhúng lại" },
  },

  // Rerank section
  rerank: {
    title: common.rerank,
    desc: {
      en: "Reorder results: better, slower.",
      vi: "Sắp lại thứ tự đoạn tìm được: đúng hơn, đổi lại chậm hơn.",
    },
    more: {
      en: "An optional HTTP rerank server reads the question and each chunk together, then returns a better order than vector similarity alone. Changing this does not re-embed the library.",
      vi: "Một máy chủ rerank HTTP tùy chọn đọc cả câu hỏi lẫn từng đoạn rồi trả về thứ tự tốt hơn so vector đơn thuần. Đổi ở đây không nhúng lại thư viện.",
    },
    saveFailed: { en: "Save failed", vi: "Không lưu được" },

    enableLabel: { en: "Enable", vi: "Bật" },
    enableDesc: {
      en: "Off is faster, less accurate.",
      vi: "Tắt thì tìm nhanh hơn, thứ tự kém chính xác hơn.",
    },
    enableMore: {
      en: "With this off, retrieval still runs by merging keywords with vectors; only the final scoring pass is missing.",
      vi: "Tắt đi thì truy hồi vẫn chạy bằng cách hợp nhất từ khoá với vector; chỉ mất bước chấm lại ở cuối.",
    },
    enableToggleLabel: { en: "Enable reranking", vi: "Bật xếp hạng lại" },

    candidatesLabel: { en: "Candidates", vi: "Số đoạn chấm lại" },
    candidatesDesc: {
      en: "More chunks, better order, slower.",
      vi: "Càng nhiều đoạn thì thứ tự càng đúng và càng chờ lâu.",
    },
    candidatesMore: {
      en: "Each candidate is sent to the configured HTTP rerank endpoint. Lowering this number reduces network and scoring latency.",
      vi: "Mỗi ứng viên được gửi tới endpoint rerank HTTP đã cấu hình. Hạ số này để giảm độ trễ mạng và chấm điểm.",
    },

    topNLabel: { en: "Kept", vi: "Số đoạn giữ lại" },
    topNDesc: {
      en: "Top chunks sent to model.",
      vi: "Mấy đoạn đứng đầu được đưa cho mô hình trả lời.",
    },

    urlLabel: { en: "Server", vi: "Máy chủ" },
    urlDesc: {
      en: "Base URL or full /v1/rerank endpoint.",
      vi: "URL gốc hoặc endpoint /v1/rerank đầy đủ.",
    },
    urlFieldLabel: { en: "Rerank server URL", vi: "URL máy chủ rerank" },

    remoteModelLabel: { en: "Model name", vi: "Tên mô hình" },
    remoteModelDesc: {
      en: "Model your server serves.",
      vi: "Tên mô hình mà máy chủ của bạn phục vụ.",
    },
    modelFieldLabel: { en: "Rerank model", vi: "Mô hình xếp hạng lại" },

    reasonMore: {
      en: "Turning this back on is not a re-embed — this step only reorders chunks that were already found, so the next question already follows the new setting.",
      vi: "Bật lại không phải nhúng lại thư viện — bước này chỉ sắp xếp lại những đoạn đã tìm được, nên câu hỏi kế tiếp đã theo cấu hình mới.",
    },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
