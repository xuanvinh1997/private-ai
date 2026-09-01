import { For } from "solid-js";
import Icon, { type IconName } from "./Icon";
import { Tip } from "./primitives";
import { setTheme, theme, type ThemeChoice } from "../lib/theme";
import type { ProjectKind } from "../lib/protocol";

export type TabId =
  | "chat"
  | "projects"
  | "diff"
  | "code"
  | "graph"
  | "library"
  | "terminal"
  | "settings";

/**
 * Tab, và **loại dự án nào thấy được nó**.
 *
 * `kinds: null` là "luôn hiện". Việc lọc nằm ở đây chứ không nằm rải trong `App`, vì đây
 * là chỗ duy nhất biết danh sách đầy đủ — một tab mới thêm vào bảng này mà quên khai loại
 * sẽ hiện ở mọi nơi, và đó là kiểu quên rẻ nhất để phát hiện.
 *
 * Cắt tab **của loại kia** đi chứ không làm mờ nó: một nút mờ nói rằng có thứ gì đó đang
 * bị khoá và người dùng phải tìm cách mở, trong khi sự thật là nó không áp dụng ở đây.
 */
const TABS: { id: TabId; label: string; icon: IconName; kinds: ProjectKind[] | null }[] = [
  { id: "chat", label: "Hội thoại", icon: "chat", kinds: null },
  { id: "projects", label: "Dự án", icon: "folder-open", kinds: null },
  { id: "diff", label: "Thay đổi", icon: "diff", kinds: ["code"] },
  { id: "code", label: "Mã nguồn", icon: "code", kinds: ["code"] },
  { id: "graph", label: "Đồ thị mã nguồn", icon: "graph", kinds: ["code"] },
  { id: "library", label: "Thư viện tài liệu", icon: "library", kinds: ["docs"] },
  { id: "terminal", label: "Terminal", icon: "terminal", kinds: ["code"] },
  { id: "settings", label: "Cài đặt", icon: "settings", kinds: null },
];

/** Tab hiện được cho một loại dự án. `App` dùng nó để sửa tab khi đổi dự án. */
export function tabsFor(kind: ProjectKind | undefined): TabId[] {
  return TABS.filter((tab) => tab.kinds === null || (kind !== undefined && tab.kinds.includes(kind))).map(
    (tab) => tab.id,
  );
}

const NEXT_THEME: Record<ThemeChoice, ThemeChoice> = {
  light: "dark",
  dark: "system",
  system: "light",
};

const THEME_ICON: Record<ThemeChoice, IconName> = {
  light: "sun",
  dark: "moon",
  system: "monitor",
};

const THEME_LABEL: Record<ThemeChoice, string> = {
  light: "Giao diện sáng",
  dark: "Giao diện tối",
  system: "Theo hệ thống",
};

/**
 * Rail biểu tượng — cột hẹp nhất, và là thứ duy nhất không bao giờ biến mất.
 *
 * Chỉ có biểu tượng: nhãn chữ ở đây sẽ ép cột rộng gấp ba, và người ta học vị trí của
 * bốn cái nút nhanh hơn học chữ trên chúng. Nhãn vẫn tồn tại đầy đủ trong `aria-label`
 * và trong chú giải, nên "chỉ có biểu tượng" là chuyện của mắt, không phải của ngữ nghĩa.
 */
export default function Rail(props: {
  active: TabId;
  onSelect: (tab: TabId) => void;
  /** Loại dự án đang mở. Vắng mặt (lõi chưa trả lời) thì chỉ hiện tab luôn có. */
  kind?: ProjectKind;
  /** Đang chuyển dự án: mọi tab đều đang nói về một cây tệp sắp không còn đúng nữa. */
  disabled?: boolean;
}) {
  const visible = () => TABS.filter((tab) => tab.kinds === null || (props.kind !== undefined && tab.kinds.includes(props.kind)));
  return (
    <nav
      aria-label="Khu vực làm việc"
      class="flex w-(--icon-rail-w) shrink-0 flex-col items-center gap-2xs border-r border-line bg-sidebar pb-md"
    >
      {/* Chỗ trống cho ba nút giao thông của macOS. Nó cũng là vùng kéo cửa sổ: rail là
          dải dọc duy nhất chắc chắn không có nội dung cuộn được đè lên. */}
      <div class="h-(--titlebar-h) w-full shrink-0" data-tauri-drag-region />

      <div
        class="mb-2xs grid size-8 shrink-0 place-items-center rounded-panel bg-accent text-on-accent"
        title="Private AI"
      >
        <Icon name="sparkle" size={17} />
      </div>

      <For each={visible()}>
        {(tab) => (
          <span class="group/tip relative">
            <button
              type="button"
              onClick={() => props.onSelect(tab.id)}
              disabled={props.disabled}
              aria-label={tab.label}
              aria-current={props.active === tab.id ? "page" : undefined}
              class="grid size-10 place-items-center rounded-panel text-muted transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40 enabled:hover:bg-[var(--overlay-hover)] enabled:hover:text-ink aria-[current=page]:bg-accent-soft aria-[current=page]:text-accent-ink"
            >
              <Icon name={tab.icon} size={19} />
            </button>
            <Tip side="right">{tab.label}</Tip>
          </span>
        )}
      </For>

      <div class="flex-1" />

      <span class="group/tip relative">
        <button
          type="button"
          onClick={() => setTheme(NEXT_THEME[theme()])}
          aria-label={`Giao diện: ${THEME_LABEL[theme()]}. Bấm để đổi.`}
          class="grid size-10 place-items-center rounded-panel text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
        >
          <Icon name={THEME_ICON[theme()]} size={18} />
        </button>
        <Tip side="right">{THEME_LABEL[theme()]}</Tip>
      </span>
    </nav>
  );
}
