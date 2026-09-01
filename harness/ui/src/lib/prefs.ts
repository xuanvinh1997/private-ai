import { createSignal, type Accessor } from "solid-js";
import type { ToolScope } from "./protocol";

/**
 * Tuỳ chọn hiển thị, nhớ giữa các lần mở.
 *
 * Chung một khuôn với `theme.ts` và cùng một lý do: `localStorage` ném ở cửa sổ riêng tư
 * và khi trình duyệt chặn dữ liệu trang, nên mọi lần đọc/ghi đều phải chịu được việc
 * không có kho nào cả. Không đọc được thì rơi về mặc định — mất một tuỳ chọn nhẹ hơn
 * nhiều so với một màn hình trắng.
 */

/** `bubble`: bong bóng hai bên như chat. `document`: toàn chiều rộng như một tài liệu. */
export type DisplayMode = "bubble" | "document";

function persisted<T extends string>(
  key: string,
  fallback: T,
  isValid: (raw: string) => raw is T,
): [Accessor<T>, (value: T) => void] {
  let initial = fallback;
  try {
    const raw = localStorage.getItem(key);
    if (raw !== null && isValid(raw)) initial = raw;
  } catch {
    /* bỏ qua */
  }
  const [get, set] = createSignal<T>(initial);
  return [
    get,
    (value: T) => {
      set(() => value);
      try {
        localStorage.setItem(key, value);
      } catch {
        /* không ghi được thì lựa chọn chỉ sống trong phiên này */
      }
    },
  ];
}

const isDisplayMode = (raw: string): raw is DisplayMode =>
  raw === "bubble" || raw === "document";

export const [displayMode, setDisplayMode] = persisted<DisplayMode>(
  "pai-display-mode",
  "bubble",
  isDisplayMode,
);

const isFlag = (raw: string): raw is "on" | "off" => raw === "on" || raw === "off";

function flag(key: string, fallback: boolean): [Accessor<boolean>, (value: boolean) => void] {
  const [get, set] = persisted<"on" | "off">(key, fallback ? "on" : "off", isFlag);
  return [() => get() === "on", (value: boolean) => set(value ? "on" : "off")];
}

/**
 * Thanh bên trái. Khoá lưu đổi tên theo cột nó mô tả: cột cũ chỉ chứa danh sách phiên,
 * cột mới chứa mọi lối đi, nên một người từng thu gọn danh sách phiên năm ngoái không nên
 * mở ứng dụng lên và thấy mất luôn cả đường vào cài đặt.
 */
export const [sidebarOpen, setSidebarOpen] = flag("pai-sidebar", true);
export const [changesPanelOpen, setChangesPanelOpen] = flag("pai-changes-panel", false);

const isToolScope = (raw: string): raw is ToolScope =>
  raw === "read" || raw === "write" || raw === "shell";

/**
 * Phạm vi tool mà **một lượt mới** bắt đầu ở đó.
 *
 * Đây là một thiết lập, không phải trạng thái của lượt: bộ chọn trong ô soạn tin vẫn là
 * của từng lượt và vẫn đổi được ở đó bất cứ lúc nào. Thứ được nhớ lại chỉ là điểm xuất
 * phát — chính vì thế nó nằm ở trang Quyền chứ không nằm trong ô soạn tin.
 *
 * Mặc định vẫn là `write` như bản cũ chốt cứng trong `App.tsx`: đổi mặc định trong cùng
 * một lần thay giao diện là đổi hai thứ rồi không biết thứ nào gây ra khác biệt. Ai muốn
 * mở app lên ở mức chỉ-đọc thì chọn lấy, và lựa chọn đó mới là thứ được nhớ.
 *
 * `shell` **được phép** lưu. Nghe nguy hiểm, nhưng nó là một lựa chọn người dùng đã cố ý
 * bấm ở trang Quyền, cạnh một câu nói thẳng rằng mức đó cho thi hành lệnh trên máy này —
 * khác hẳn một mức quyền tự leo lên mà không ai chọn. Còn giao diện thì luôn hiện mức
 * đang có ngay trong ô soạn tin, nên nó không bao giờ là một quyền mở lén.
 */
export const [defaultToolScope, setDefaultToolScope] = persisted<ToolScope>(
  "pai-tool-scope-mac-dinh",
  "write",
  isToolScope,
);
