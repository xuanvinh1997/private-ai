import { For, type JSX } from "solid-js";
import { displayMode, setDisplayMode, type DisplayMode } from "../lib/prefs";
import { setTheme, theme, type ThemeChoice } from "../lib/theme";
import Icon, { type IconName } from "./Icon";

/**
 * Trang cài đặt.
 *
 * Chỉ chứa thứ đổi được ngay và thấy được ngay. Mọi lựa chọn ở đây là một `radiogroup`
 * chứ không phải một dãy nút rời: đó là những lựa chọn loại trừ nhau, và trình đọc màn
 * hình cần biết điều đó để đọc "2 trên 3" thay vì đọc ba cái nút không liên quan.
 */
export default function SettingsView() {
  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-(--reading-measure) flex-col gap-2xl">
        <Section title="Giao diện" desc="Áp dụng ngay, nhớ lại ở lần mở sau.">
          <Choice<ThemeChoice>
            label="Bảng màu"
            value={theme()}
            onPick={setTheme}
            options={[
              { id: "light", label: "Sáng", icon: "sun" },
              { id: "dark", label: "Tối", icon: "moon" },
              { id: "system", label: "Theo hệ thống", icon: "monitor" },
            ]}
          />
          <Choice<DisplayMode>
            label="Cách hiển thị hội thoại"
            value={displayMode()}
            onPick={setDisplayMode}
            options={[
              { id: "bubble", label: "Bong bóng", icon: "bubble" },
              { id: "document", label: "Tài liệu", icon: "document" },
            ]}
            hint="Chế độ tài liệu bỏ bong bóng và trải hết bề rộng — dễ đọc hơn với diff dài."
          />
        </Section>

        <Section title="Phím tắt" desc="Chạy được kể cả khi tiêu điểm đang ở ô soạn tin.">
          <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-lg gap-y-xs text-sm">
            <Shortcut keys="⌘K / Ctrl+K" what="Tìm phiên" />
            <Shortcut keys="Enter" what="Gửi tin nhắn" />
            <Shortcut keys="Shift+Enter" what="Xuống dòng" />
            <Shortcut keys="Esc" what="Đóng hộp thoại đang mở" />
          </dl>
        </Section>
      </div>
    </div>
  );
}

function Section(props: { title: string; desc: string; children: JSX.Element }) {
  return (
    <section class="flex flex-col gap-md">
      <div class="flex flex-col gap-3xs">
        <h2 class="m-0 text-md font-semibold text-ink">{props.title}</h2>
        <p class="m-0 text-xs text-muted">{props.desc}</p>
      </div>
      {props.children}
    </section>
  );
}

function Choice<T extends string>(props: {
  label: string;
  value: T;
  hint?: string;
  options: { id: T; label: string; icon: IconName }[];
  onPick: (value: T) => void;
}) {
  return (
    <div class="flex flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
      <span class="text-xs font-medium text-ink">{props.label}</span>
      <div role="radiogroup" aria-label={props.label} class="flex flex-wrap gap-2xs">
        <For each={props.options}>
          {(option) => (
            <button
              type="button"
              role="radio"
              aria-checked={props.value === option.id}
              onClick={() => props.onPick(option.id)}
              class="flex items-center gap-2xs rounded-pill border px-md py-2xs text-xs transition-colors duration-[var(--dur-fast)]"
              classList={{
                "border-line text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
                  props.value !== option.id,
                "border-accent bg-accent-soft text-accent-ink": props.value === option.id,
              }}
            >
              <Icon name={option.icon} size={14} />
              {option.label}
            </button>
          )}
        </For>
      </div>
      {props.hint && <p class="m-0 text-2xs text-faint">{props.hint}</p>}
    </div>
  );
}

function Shortcut(props: { keys: string; what: string }) {
  return (
    <>
      <dt class="m-0">
        <kbd class="rounded-btn border border-line bg-surface px-2xs py-3xs font-mono text-2xs text-text">
          {props.keys}
        </kbd>
      </dt>
      <dd class="m-0 text-muted">{props.what}</dd>
    </>
  );
}
