import { createSignal } from "solid-js";

export type ThemeChoice = "light" | "dark" | "system";

const STORAGE_KEY = "pai-theme";

function read(): ThemeChoice {
  // localStorage ném ở cửa sổ riêng tư và khi trình duyệt chặn dữ liệu trang. Không
  // đọc được thì rơi về "system" — đó cũng là lựa chọn ít gây bất ngờ nhất.
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
  } catch {
    /* bỏ qua */
  }
  return "system";
}

const [theme, setThemeSignal] = createSignal<ThemeChoice>(read());

/**
 * Đóng dấu lựa chọn lên `<html>`.
 *
 * Chế độ "system" **không stamp gì cả** — đó là điều kiện để khối
 * `@media (prefers-color-scheme: dark)` trong tokens.css có tác dụng. Stamp
 * `data-theme="system"` sẽ vô hại về mặt CSS nhưng khiến người sau tưởng có ba nhánh
 * theme trong khi chỉ có hai; giữ đúng luật của theme.py:239.
 */
function stamp(choice: ThemeChoice): void {
  const root = document.documentElement;
  if (choice === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", choice);
}

export function setTheme(choice: ThemeChoice): void {
  setThemeSignal(choice);
  stamp(choice);
  try {
    localStorage.setItem(STORAGE_KEY, choice);
  } catch {
    /* không ghi được thì lựa chọn chỉ sống trong phiên này — vẫn hơn là nổ */
  }
}

/** Gọi một lần lúc khởi động, trước khi render, để tránh nháy theme sai. */
export function initTheme(): void {
  stamp(theme());
}

export { theme };
