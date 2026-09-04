import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `libs` area. See lib/i18n/README.md for the wording rules. */
export const libs = {
  /** `sessions.ts`: relative time under each session row. */
  time: {
    justNow: { en: "just now", vi: "vừa xong" },
    minutes: { en: "{n}m ago", vi: "{n} phút trước" },
    hours: { en: "{n}h ago", vi: "{n} giờ trước" },
    days: { en: "{n}d ago", vi: "{n} ngày trước" },
  },

  /** `sessions.ts`: group headings in the left column. */
  sessionGroup: {
    today: { en: "Today", vi: "Hôm nay" },
    week: { en: "Past week", vi: "7 ngày qua" },
    older: { en: "Older", vi: "Cũ hơn" },
  },

  /** `complete.ts`: the hint beside each `/` command. Hints only, since command names are typed and matched on. */
  command: {
    newSession: common.newSession,
    findSession: { en: "Find a session (⌘K)", vi: "Tìm phiên đã có (⌘K)" },
    projects: { en: "Open projects", vi: "Mở màn hình dự án" },
    changes: { en: "Changes this turn", vi: "Bảng thay đổi của lượt này" },
    docs: { en: "Document library", vi: "Thư viện tài liệu" },
    models: { en: "Switch provider and model", vi: "Đổi nhà cung cấp và mô hình" },
    mcp: { en: "Manage MCP servers", vi: "Quản lý server MCP" },
    permissions: { en: "Permissions and tool scope", vi: "Trang quyền và phạm vi tool" },
    shortcuts: { en: "Keyboard shortcuts", vi: "Danh sách phím tắt" },
    settings: { en: "General settings", vi: "Cài đặt chung" },
  },

  /** `diff.ts`: the separator row shown when a diff block is folded. */
  diff: {
    gap: common.linesHidden,
  },

  /** `katex.ts`: text shown in place of a formula KaTeX could not render. */
  math: {
    parseFailed: {
      en: "KaTeX could not read this formula.",
      vi: "KaTeX không đọc được công thức này.",
    },
  },

  /** `mermaid.ts`: render errors plus diagram kind names for `aria-label`; the lookup keys are mermaid syntax. */
  diagram: {
    loadFailed: {
      en: "Could not load the diagram renderer.",
      vi: "Không nạp được bộ vẽ sơ đồ.",
    },
    parseFailed: {
      en: "Mermaid could not read this diagram.",
      vi: "Mermaid không đọc được sơ đồ này.",
    },
    generic: { en: "diagram", vi: "sơ đồ" },
    flowchart: { en: "flowchart", vi: "lưu đồ" },
    sequence: { en: "sequence diagram", vi: "sơ đồ tuần tự" },
    class: { en: "class diagram", vi: "sơ đồ lớp" },
    state: { en: "state diagram", vi: "sơ đồ trạng thái" },
    entity: { en: "entity diagram", vi: "sơ đồ thực thể" },
    journey: { en: "user journey", vi: "hành trình người dùng" },
    gantt: { en: "gantt chart", vi: "biểu đồ gantt" },
    pie: { en: "pie chart", vi: "biểu đồ tròn" },
    mindmap: { en: "mind map", vi: "sơ đồ tư duy" },
    timeline: { en: "timeline", vi: "dòng thời gian" },
    gitgraph: { en: "git graph", vi: "đồ thị git" },
    quadrant: { en: "quadrant chart", vi: "biểu đồ bốn góc" },
    requirement: { en: "requirement diagram", vi: "sơ đồ yêu cầu" },
    block: { en: "block diagram", vi: "sơ đồ khối" },
    sankey: { en: "sankey diagram", vi: "sơ đồ dòng chảy" },
    xy: { en: "xy chart", vi: "biểu đồ toạ độ" },
    architecture: { en: "architecture diagram", vi: "sơ đồ kiến trúc" },
    packet: { en: "packet diagram", vi: "sơ đồ gói tin" },
    c4: { en: "C4 diagram", vi: "sơ đồ C4" },
  },

  /** `attach.ts`: title of the OS file dialog. */
  attach: {
    pickTitle: { en: "Attach files", vi: "Đính kèm tệp" },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
