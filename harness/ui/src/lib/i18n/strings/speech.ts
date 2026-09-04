import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `speech` area: the model that turns recordings and the microphone into text.
 * See lib/i18n/README.md. */
export const speech = {
  section: {
    title: { en: "Speech", vi: "Tiếng nói" },
    desc: {
      en: "Turn recordings into text.",
      vi: "Đọc bản ghi âm thành chữ, và đọc chính tả bằng micro.",
    },
    more: {
      en: "One local model does both: audio files a document project reads, and the microphone in the composer. It runs on this machine, so nothing you say leaves it.",
      vi: "Một mô hình chạy tại chỗ lo cả hai việc: tệp âm thanh trong dự án tài liệu, và micro ở ô soạn tin. Nó chạy trên máy này, nên không lời nào rời khỏi máy.",
    },
  },

  model: {
    label: common.model,
    desc: { en: "A GGUF speech model.", vi: "Một tệp mô hình .gguf" },
    more: {
      en: "Any GGUF that transcribe.cpp reads. A streaming model shows words as you speak; the others produce their text when you stop.",
      vi: "Bất kỳ tệp GGUF nào transcribe.cpp đọc được. Mô hình chạy theo dòng hiện chữ ngay khi bạn nói; loại còn lại chỉ trả chữ khi bạn dừng.",
    },
    pick: { en: "Choose file", vi: "Chọn tệp" },
    none: { en: "No model chosen", vi: "Chưa chọn mô hình" },
  },

  language: {
    label: { en: "Language", vi: "Ngôn ngữ" },
    desc: { en: "Empty lets the model decide.", vi: "Để trống thì mô hình tự nhận ra" },
    more: {
      en: "A hint, not a filter: a wrong one is worse than none, so leave it empty unless the model keeps guessing wrong.",
      vi: "Chỉ là gợi ý: đặt sai còn tệ hơn bỏ trống, nên chỉ điền khi mô hình đoán nhầm nhiều lần.",
    },
    placeholder: { en: "vi-VN", vi: "vi-VN" },
  },

  library: {
    label: { en: "Read audio files", vi: "Đọc tệp âm thanh" },
    desc: { en: "Off skips them entirely.", vi: "Tắt thì bỏ qua, không báo hỏng" },
    more: {
      en: "With this on, a recording in a document project becomes a transcript with a heading every five minutes, so a citation points at a moment you can find.",
      vi: "Bật thì bản ghi âm trong dự án tài liệu thành bản chép có tiêu đề mỗi năm phút, nên trích dẫn chỉ đúng đoạn bạn tìm lại được.",
    },
    toggleLabel: { en: "Read audio files", vi: "Đọc tệp âm thanh" },
  },

  probe: {
    action: common.test,
    running: { en: "Loading…", vi: "Đang nạp mô hình…" },
    failed: { en: "Could not load", vi: "Không nạp được mô hình" },
    arch: { en: "Family", vi: "Họ mô hình" },
    backend: { en: "Backend", vi: "Chạy trên" },
    streaming: { en: "Live dictation", vi: "Đọc theo dòng" },
    languagesLabel: { en: "Languages", vi: "Ngôn ngữ" },
    languages: { en: "{n} languages", vi: "{n} ngôn ngữ" },
  },

  saveFailed: { en: "Could not save", vi: "Không lưu được cấu hình" },

  // The composer's microphone button and what it says while listening.
  dictation: {
    start: { en: "Dictate", vi: "Đọc chính tả" },
    stop: { en: "Stop dictating", vi: "Dừng đọc" },
    cancel: { en: "Discard dictation", vi: "Bỏ đoạn vừa đọc" },
    listening: { en: "Listening", vi: "Đang nghe" },
    buffering: {
      en: "Recording — text appears when you stop",
      vi: "Đang ghi — chữ hiện ra khi bạn dừng",
    },
    failed: { en: "Dictation stopped", vi: "Đọc chính tả dừng lại" },
    needsModel: {
      en: "Choose a speech model in Settings first.",
      vi: "Chọn mô hình tiếng nói trong Cài đặt trước đã.",
    },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
