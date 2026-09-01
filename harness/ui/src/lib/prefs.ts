import { createSignal, type Accessor } from "solid-js";

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
