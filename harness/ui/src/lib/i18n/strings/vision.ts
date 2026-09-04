import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `vision` area: the model that reads images for OCR. See lib/i18n/README.md. */
export const vision = {
  section: {
    title: common.vision,
    desc: {
      en: "Read text off images.",
      vi: "Đọc chữ trong ảnh và trang PDF đã quét.",
    },
    more: {
      en: "The model that reads scanned pages and images. It is separate from the chat model, because not every chat model can see, and a scanned page sent to a remote model is the whole page.",
      vi: "Mô hình đọc chữ trong ảnh và trang PDF đã quét. Nó tách khỏi mô hình hội thoại: không phải mô hình nào cũng nhìn được ảnh, và gửi một trang quét đi là gửi nguyên trang đó.",
    },
    failed: common.actionFailed,
    setFailed: {
      en: "Could not set the vision model: {detail}",
      vi: "Không đặt được mô hình đọc ảnh: {detail}",
    },
  },

  empty: {
    text: {
      en: "No provider to read images.",
      vi: "Chưa có nhà cung cấp nào để đọc ảnh.",
    },
    infoLabel: { en: "Where to add a provider", vi: "Thêm nhà cung cấp ở đâu" },
    infoText: {
      en: "Add a provider in the Chat tab first — one that runs on this machine is enough, and scanned pages then stay here.",
      vi: "Thêm một nhà cung cấp ở tab Hội thoại trước — một cái chạy tại chỗ là đủ, và trang quét sẽ không rời khỏi máy.",
    },
  },

  // The OCR switch itself: the setting that decides whether images are read at all.
  ocr: {
    label: { en: "Read scans", vi: "Đọc ảnh và bản quét" },
    desc: {
      en: "Off skips images entirely.",
      vi: "Tắt thì bỏ qua ảnh, không báo lỗi.",
    },
    more: {
      en: "On, pages without a text layer are sent to the vision model. Off, images and scanned pages are skipped: documents that already have text still index normally, and nothing is reported as broken. Turning it back on picks those files up on the next sync.",
      vi: "Bật thì trang không có lớp chữ được gửi cho mô hình đọc ảnh. Tắt thì ảnh và trang quét bị bỏ qua: tài liệu đã có chữ vẫn nạp bình thường và không có gì bị báo hỏng. Bật lại thì lần đồng bộ sau sẽ nạp những tệp đó.",
    },
    toggleLabel: { en: "Read scans", vi: "Đọc ảnh và bản quét" },
    saveFailed: {
      en: "Could not save the OCR switch: {detail}",
      vi: "Không lưu được công tắc OCR: {detail}",
    },
    offTitle: { en: "Scans skipped", vi: "Đang bỏ qua ảnh" },
    offBody: {
      en: "Images and scanned pages are left out of the library. Everything with a text layer still indexes.",
      vi: "Ảnh và trang quét không được đưa vào thư viện. Mọi thứ có sẵn lớp chữ vẫn nạp bình thường.",
    },
  },

  current: {
    unsetTitle: { en: "Not configured", vi: "Chưa cấu hình đọc ảnh" },
    unsetInfoLabel: { en: "About the unconfigured state", vi: "Về trạng thái chưa cấu hình" },
    unsetInfoText: {
      en: "The library still works: PDFs with a text layer, Word files and Markdown all index as usual. Only images and scanned pages are left out, and they come in as soon as a vision model is picked here.",
      vi: "Thư viện vẫn dùng được: PDF có lớp chữ, tệp Word và Markdown vẫn nạp như thường. Chỉ ảnh và trang quét bị bỏ qua, và chúng sẽ được nạp ngay khi bạn chọn mô hình đọc ảnh ở đây.",
    },
    unsetBody: {
      en: "Scans and images are skipped.",
      vi: "Ảnh và trang quét đang bị bỏ qua.",
    },
    brokenTitle: { en: "Not working", vi: "Cấu hình đọc ảnh chưa dùng được" },
    brokenMore: {
      en: "Until this is fixed, images and scanned pages are skipped rather than indexed.",
      vi: "Chưa sửa xong thì ảnh và trang quét bị bỏ qua chứ không được nạp.",
    },
    okTitle: { en: "In use", vi: "Đang đọc ảnh bằng mô hình này" },
    okMoreOnDevice: {
      en: "Page images stay on this machine.",
      vi: "Ảnh trang không rời khỏi máy này.",
    },
    okMoreRemote: {
      en: "Page images are uploaded to {provider} to be read.",
      vi: "Ảnh trang được gửi lên {provider} để đọc.",
    },
    okBodyOnDevice: { en: "reads scans on this machine.", vi: "đang đọc bản quét ngay trên máy này." },
    okBodyRemote: { en: "reads scans at {provider}.", vi: "đang đọc bản quét ở {provider}." },
  },

  provider: {
    desc: { en: "Which server reads images.", vi: "Máy chủ nào đọc ảnh." },
    more: {
      en: "It can be a different server from the chat one. Reading a scan uploads the whole page, so a provider on this machine is the private choice.",
      vi: "Có thể là máy chủ khác với máy chủ hội thoại. Đọc bản quét là gửi nguyên trang đi, nên chọn nhà cung cấp chạy tại chỗ là lựa chọn kín đáo nhất.",
    },
    selectLabel: { en: "Vision provider", vi: "Máy chủ đọc ảnh" },
    unset: common.unset,
    optionOff: { en: "{name} (off)", vi: "{name} (đang tắt)" },
    optionOnDevice: { en: "{name} · on this machine", vi: "{name} · chạy tại chỗ" },
  },

  model: {
    label: { en: "Vision model", vi: "Mô hình đọc ảnh" },
    desc: { en: "Must be able to see.", vi: "Phải là mô hình nhìn được ảnh." },
    more: {
      en: "A chat-only model returns an error for every page. Models the server itself calls image-capable come first; the rest stay listed and labelled, since a server that declares nothing still runs one. Test to be certain.",
      vi: "Mô hình chỉ biết trò chuyện sẽ báo lỗi ở từng trang. Mô hình mà chính máy chủ khai là nhìn được ảnh sẽ đứng trước; số còn lại vẫn nằm trong danh sách và có ghi chú, vì máy chủ không khai gì vẫn có thể đang chạy một cái. Bấm Thử để biết chắc.",
    },
    fieldLabel: { en: "Vision model", vi: "Mô hình đọc ảnh" },
  },

  source: {
    loading: common.loadingModels,
    unavailable: {
      en: "The server did not answer with a model list; type the name instead.",
      vi: "Máy chủ không trả về danh sách mô hình; gõ thẳng tên vào cũng được.",
    },
    someSeeing: {
      en: "{n} models on this server, {k} of them can see images.",
      vi: "Máy chủ có {n} mô hình, {k} cái trong đó nhìn được ảnh.",
    },
    noneSeeing: {
      en: "{n} models on this server, none of which declares image support. Type a name if you know better.",
      vi: "Máy chủ có {n} mô hình, không cái nào khai là nhìn được ảnh. Biết chắc thì cứ gõ thẳng tên vào.",
    },
  },

  probe: {
    label: { en: "Test", vi: "Thử đọc một ảnh" },
    desc: { en: "Read one test image.", vi: "Đọc thử một ảnh có sẵn chữ." },
    more: {
      en: "Sends a small image with one line of text and checks that the model reads it back. This is the same call the library makes for every scanned page, so a pass here means OCR works.",
      vi: "Gửi một ảnh nhỏ có đúng một dòng chữ rồi xem mô hình có đọc lại đúng không. Đây chính là lệnh mà thư viện gọi cho từng trang quét, nên thử được nghĩa là OCR chạy được.",
    },
    run: common.test,
    running: common.testing,
    busy: {
      en: "Reading the test image… a local vision model can take a minute to load.",
      vi: "Đang đọc ảnh thử… mô hình đọc ảnh chạy tại chỗ có thể mất một phút để nạp.",
    },
    failedTitle: common.testFailed,
    okTitle: { en: "Reads images", vi: "Đọc được ảnh" },
    notOkTitle: { en: "Did not read it", vi: "Chưa đọc được ảnh" },
    answer: { en: "Model answered:", vi: "Mô hình trả lời:" },
  },

  privacy: {
    remoteTitle: { en: "Pages leave this machine", vi: "Trang tài liệu sẽ rời khỏi máy" },
    remoteMore: {
      en: "{name} is at {url}. Every scanned page is uploaded there as an image to be read.",
      vi: "{name} nằm ở {url}. Mỗi trang quét sẽ được gửi lên đó dưới dạng ảnh để đọc.",
    },
    remoteBody: {
      en: "Whole page images are uploaded to {url}, not just a search query.",
      vi: "Ảnh nguyên trang được gửi tới {url}, không chỉ là câu truy vấn.",
    },
    localTitle: { en: "Pages stay here", vi: "Trang tài liệu ở lại máy này" },
    localMore: {
      en: "This provider only answers on this machine, so page images never reach the network.",
      vi: "Nhà cung cấp này chỉ trả lời ngay trên máy, nên ảnh trang không ra mạng.",
    },
    localBody: {
      en: "Scanned pages are read on this machine.",
      vi: "Bản quét được đọc ngay trên máy này.",
    },
  },

  apply: {
    save: common.save,
    unsaved: { en: "Not saved yet", vi: "Chưa lưu" },
    note: {
      en: "Changing this affects the next scan only; documents already read stay as they are.",
      vi: "Đổi ở đây chỉ ảnh hưởng lần đọc sau; tài liệu đã đọc xong vẫn giữ nguyên.",
    },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
