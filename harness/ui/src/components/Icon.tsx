import { For } from "solid-js";

/**
 * Bộ biểu tượng vẽ tay, không phải một thư viện.
 *
 * Một gói icon kéo theo vài trăm KB và một lớp component nữa, đổi lại thứ ta cần là hai
 * mươi hình vẽ. Chúng nằm chung một lưới 24×24, cùng độ dày nét và cùng kiểu bo đầu nét
 * — đó mới là thứ làm bộ biểu tượng trông thuộc về nhau, chứ không phải nguồn gốc của nó.
 *
 * Mọi icon là `aria-hidden`: ý nghĩa đi qua `aria-label` của nút bọc ngoài. Một biểu
 * tượng tự xưng tên sẽ bị trình đọc màn hình đọc hai lần.
 */

const PATHS = {
  chat: ["M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"],
  diff: [
    "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z",
    "M14 2v5h5",
    "M12 11v6",
    "M9 14h6",
  ],
  terminal: ["m4 17 6-6-6-6", "M12 19h8"],
  settings: [
    "M21 4h-7", "M10 4H3", "M21 12h-9", "M8 12H3", "M21 20h-5", "M12 20H3",
    "M12 2v4", "M6 10v4", "M14 18v4",
  ],
  plus: ["M5 12h14", "M12 5v14"],
  search: ["M11 3a8 8 0 1 0 0 16 8 8 0 0 0 0-16z", "m21 21-4.35-4.35"],
  "panel-left": ["M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z", "M9 3v18"],
  "panel-right": ["M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z", "M15 3v18"],
  more: ["M12 12h.01", "M19 12h.01", "M5 12h.01"],
  trash: ["M3 6h18", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"],
  copy: ["M11 9h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-9a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2z", "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"],
  retry: ["M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8", "M3 3v5h5", "M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16", "M21 21v-5h-5"],
  "chevron-down": ["m6 9 6 6 6-6"],
  "chevron-right": ["m9 18 6-6-6-6"],
  "arrow-down": ["M12 5v13", "m6 12 6 6 6-6"],
  paperclip: ["m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"],
  model: ["M5 5h14a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z", "M9 9h6v6H9z", "M9 2v2", "M15 2v2", "M9 20v2", "M15 20v2", "M2 9h2", "M2 15h2", "M20 9h2", "M20 15h2"],
  tools: ["M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94z"],
  stop: ["M7 7h10a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1z"],
  send: ["m5 12 7-7 7 7", "M12 19V5"],
  enter: ["m9 10-5 5 5 5", "M20 4v7a4 4 0 0 1-4 4H4"],
  sun: ["M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z", "M12 2v2", "M12 20v2", "m4.9 4.9 1.4 1.4", "m17.7 17.7 1.4 1.4", "M2 12h2", "M20 12h2", "m4.9 19.1 1.4-1.4", "m17.7 6.3 1.4-1.4"],
  moon: ["M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z"],
  monitor: ["M3 4h18a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z", "M8 21h8", "M12 17v4"],
  x: ["M18 6 6 18", "m6 6 12 12"],
  check: ["M20 6 9 17l-5-5"],
  clock: ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z", "M12 6v6l4 2"],
  bubble: ["M8 3h8a5 5 0 0 1 5 5v3a5 5 0 0 1-5 5h-4l-5 4v-4a5 5 0 0 1-3-4.58V8a5 5 0 0 1 4-4.9z"],
  document: ["M6 3h9l5 5v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z", "M14 3v6h6", "M9 13h6", "M9 17h6"],
  sparkle: ["M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z", "M19 16l.8 2.2L22 19l-2.2.8L19 22l-.8-2.2L16 19l2.2-.8z"],
  // Tam giác cảnh báo. Dùng cho điều kiện đang tồn tại, không cho lỗi vừa xảy ra.
  warn: ["M12 4 2.5 20h19L12 4Z", "M12 10v4", "M12 17.5v.5"],
  folder: ["M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"],
  "folder-open": [
    "M3 8V6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v1",
    "M2.6 19.3 5 10.5h17.2l-2.4 8.8a1 1 0 0 1-1 .7H3.6a1 1 0 0 1-1-.7z",
  ],
  code: ["m9 7-5 5 5 5", "m15 7 5 5-5 5"],
  file: ["M6 3h8l5 5v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z", "M14 3v5h5"],
  "file-code": [
    "M6 3h8l5 5v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z",
    "M14 3v5h5",
    "m10 13-2 2 2 2",
    "m14 13 2 2-2 2",
  ],
  image: [
    "M4 4h16a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z",
    "m3 16 5-5 4 4 2.5-2.5L21 17",
    "M9 9h.01",
  ],
};

// Không khai `Record<string, string[]>`: khai vậy thì `IconName` chỉ là `string`, và một
// tên gõ sai đi qua trình biên dịch rồi render ra ô trống trên màn hình người dùng.
export type IconName = keyof typeof PATHS;

export default function Icon(props: { name: IconName; size?: number; class?: string }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      width={props.size ?? 16}
      height={props.size ?? 16}
      fill="none"
      stroke="currentColor"
      stroke-width="1.75"
      stroke-linecap="round"
      stroke-linejoin="round"
      class={props.class}
    >
      <For each={PATHS[props.name]}>{(d) => <path d={d} />}</For>
    </svg>
  );
}
