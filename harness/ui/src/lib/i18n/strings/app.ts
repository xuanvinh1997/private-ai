import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `app` area. See lib/i18n/README.md for the wording rules. */
export const app = {
  // Screen titles in the top bar; keys match `TabId`.
  tab: {
    chat: common.chat,
    diff: { en: "Changes", vi: "Thay đổi trong phiên" },
    library: common.library,
    projects: common.projects,
    settings: common.settings,
  },

  // Sessions
  sessionTitle: { en: "Session", vi: "Phiên làm việc" },
  sessionNew: common.newSession,
  /** Secondary line of a session row when the last message was the user's. */
  previewYou: { en: "You: {text}", vi: "Bạn: {text}" },

  // Status
  switchingProject: common.switchingProject,
  loadingTranscript: { en: "Loading transcript…", vi: "Đang nạp bản ghi…" },

  // Line under the model picker
  modelUnknown: { en: "No model", vi: "(chưa hỏi được máy chủ)" },
  modelNoServer: { en: "No model server.", vi: "Không hỏi được máy chủ mô hình." },
  modelEmbedOnly: { en: "Embedding models only.", vi: "Máy chủ chỉ có mô hình nhúng." },
  modelNoTools: { en: "This model has no tools.", vi: "Mô hình này không gọi được công cụ." },

  // Error messages: they must name what broke, so they are not shortened.
  error: {
    switchProject: {
      en: 'Could not switch to "{name}": {err}',
      vi: 'Không chuyển được sang "{name}": {err}',
    },
    swapKind: {
      en: "Could not change the project type: {err}",
      vi: "Không đổi được loại dự án: {err}",
    },
    closeProject: { en: "Could not close the project: {err}", vi: "Không đóng được dự án: {err}" },
    openFolder: {
      en: 'Could not open the folder "{path}": {err}',
      vi: 'Không mở được thư mục "{path}": {err}',
    },
    forgetProject: {
      en: 'Could not remove "{name}" from the list: {err}',
      vi: 'Không bỏ được "{name}" khỏi danh sách: {err}',
    },
    deleteSession: { en: "Could not delete the session: {err}", vi: "Không xoá được phiên: {err}" },
    deleteProject: {
      en: 'Could not delete "{name}": {err}',
      vi: 'Không xoá được "{name}": {err}',
    },
  },

  // Remove-project-from-list dialog
  forget: {
    title: { en: 'Remove "{name}" from the list?', vi: 'Bỏ "{name}" khỏi danh sách?' },
    body: { en: "Files on disk stay untouched.", vi: "Thư mục trên đĩa vẫn nguyên, không tệp nào mất." },
    bodyWithDelete: {
      en: 'Files on disk stay untouched either way. "Delete its data" also drops this project\'s conversations and its indexed library.',
      vi: 'Thư mục trên đĩa vẫn nguyên, dù chọn cách nào. "Xoá cả dữ liệu" bỏ thêm phiên trò chuyện và thư viện đã lập chỉ mục của dự án này.',
    },
    delete: { en: "Delete its data", vi: "Xoá cả dữ liệu" },
    more: {
      en: "Only the recent projects list changes. The folder and every file inside stay on disk — open it again any time and the project comes back.",
      vi: "Chỉ danh sách dự án gần đây bị đổi. Thư mục và toàn bộ tệp bên trong vẫn nguyên trên đĩa — mở lại thư mục này bất cứ lúc nào là dự án trở lại.",
    },
    confirm: { en: "Remove", vi: "Bỏ khỏi danh sách" },
  },

  // Rename-session dialog
  rename: {
    title: { en: "Rename session", vi: "Đổi tên phiên" },
    label: common.sessionName,
  },

  // Delete-session dialog: an irreversible action, so the warning keeps its full length.
  remove: {
    title: { en: 'Delete session "{title}"?', vi: 'Xoá phiên "{title}"?' },
    body: {
      en: "This transcript is deleted for good.",
      vi: "Xoá hẳn bản ghi phiên này, không lấy lại được.",
    },
    more: {
      en: "The transcript of this session is removed from the log and cannot be recovered. Other sessions and every file in the project are left untouched.",
      vi: "Bản ghi của phiên này bị xoá khỏi sổ và không lấy lại được. Các phiên khác cùng mọi tệp trong dự án đều không bị đụng tới.",
    },
    confirm: common.deleteSession,
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
