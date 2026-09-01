import { For } from "solid-js";
import Icon, { type IconName } from "./Icon";
import { Tip } from "./primitives";
import { setTheme, theme, type ThemeChoice } from "../lib/theme";

export type TabId = "chat" | "diff" | "code" | "terminal" | "settings";

const TABS: { id: TabId; label: string; icon: IconName }[] = [
  { id: "chat", label: "Hội thoại", icon: "chat" },
  { id: "diff", label: "Thay đổi", icon: "diff" },
  { id: "code", label: "Mã nguồn", icon: "code" },
  { id: "terminal", label: "Terminal", icon: "terminal" },
  { id: "settings", label: "Cài đặt", icon: "settings" },
];

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
  /** Đang chuyển dự án: mọi tab đều đang nói về một cây tệp sắp không còn đúng nữa. */
  disabled?: boolean;
}) {
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

      <For each={TABS}>
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
