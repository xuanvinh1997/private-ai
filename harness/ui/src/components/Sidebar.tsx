import { Key } from "@solid-primitives/keyed";
import { createMemo, createSignal, For, Show, type JSX } from "solid-js";
import type { ProjectKind, SessionSummary } from "../lib/protocol";
import { groupSessions, relativeTime } from "../lib/sessions";
import { setTheme, theme, type ThemeChoice } from "../lib/theme";
import Icon, { type IconName } from "./Icon";
import Menu from "./Menu";
import { IconButton } from "./primitives";

/**
 * Màn hình đang mở. Danh sách này ngắn là kết quả của một quyết định, không phải của việc
 * chưa làm xong: ứng dụng lấy ChatGPT và Codex làm khung, mà cả hai đều không có trình
 * duyệt mã nguồn hay terminal riêng — người dùng đã có editor của họ rồi.
 */
export type TabId = "chat" | "diff" | "library" | "projects" | "settings";

/**
 * Mục điều hướng, và **loại dự án nào thấy được nó**.
 *
 * `kinds: null` là "luôn hiện". Việc lọc nằm ở đây chứ không rải trong `App`, vì đây là
 * chỗ duy nhất biết danh sách đầy đủ — một mục mới thêm vào bảng mà quên khai loại sẽ hiện
 * ở mọi nơi, và đó là kiểu quên rẻ nhất để phát hiện.
 *
 * Cắt mục **của loại kia** đi chứ không làm mờ nó: một hàng mờ nói rằng có thứ gì đó đang
 * bị khoá và người dùng phải tìm cách mở, trong khi sự thật là nó không áp dụng ở đây.
 */
const NAV: { id: TabId; label: string; icon: IconName; kinds: ProjectKind[] | null }[] = [
  { id: "diff", label: "Thay đổi", icon: "diff", kinds: ["code"] },
  { id: "library", label: "Thư viện tài liệu", icon: "library", kinds: ["docs"] },
];

/** Màn hình mở được với một loại dự án. `App` dùng nó để sửa màn hình khi đổi dự án. */
export function tabsFor(kind: ProjectKind | undefined): TabId[] {
  const conditional = NAV.filter(
    (item) => item.kinds === null || (kind !== undefined && item.kinds.includes(kind)),
  ).map((item) => item.id);
  return ["chat", ...conditional, "projects", "settings"];
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

export interface SidebarProps {
  sessions: SessionSummary[];
  currentId: string;
  loading: boolean;
  /** Màn hình đang mở, để hàng tương ứng mang `aria-current`. */
  view: TabId;
  /** Loại dự án đang mở. Vắng mặt (lõi chưa trả lời) thì chỉ hiện mục luôn có. */
  kind?: ProjectKind;
  /** Số tệp đã đụng trong phiên, làm huy hiệu cho hàng "Thay đổi". */
  changeCount: number;
  /**
   * Ô chọn dự án, đặt ở chân cột.
   *
   * Nhận vào như một mảnh JSX thay vì như dữ liệu dự án: cột này biết về phiên, và cho nó
   * biết thêm về dự án nghĩa là mỗi lần hợp đồng dự án đổi thì cả danh sách phiên phải
   * biên dịch lại cùng.
   *
   * Là **hàm** chứ không phải `JSX.Element`, và đó không phải chuyện phong cách: Solid
   * biên dịch prop mang JSX thành getter, nên mỗi lần đọc prop là một lần dựng component
   * mới. Đọc nó hai lần — một cho `Show`, một để render — sinh ra hai bản; bản không được
   * gắn vào DOM vẫn kịp đăng ký listener toàn cục của nó và phá bản thật.
   */
  projectSlot?: () => JSX.Element;
  /** Đang chuyển dự án: danh sách dưới đây đang nói về dự án sắp không còn mở nữa. */
  disabled?: boolean;
  /** Dòng phụ của mỗi phiên: câu cuối đã nói. Thiếu thì bỏ dòng đó. */
  subtitle?: (session: SessionSummary) => string | undefined;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string) => void;
  onDelete: (id: string) => void;
  onGo: (view: TabId) => void;
  onCollapse: () => void;
}

/**
 * Thanh bên — cột duy nhất bên trái, và là toàn bộ hệ điều hướng của ứng dụng.
 *
 * Bản trước có một icon rail hẹp đứng trước cột phiên, theo lối LobeChat. Gộp lại thành
 * một cột là hình dạng của ChatGPT và Codex, và cái được không chỉ là 64px: với hai cột,
 * "đang ở đâu" được kể bằng hai dấu hiệu ở hai chỗ cách nhau, và người dùng phải tự ráp
 * chúng lại. Một cột thì chỉ có một danh sách và một hàng đang sáng.
 *
 * Ô tìm lọc tại chỗ thay vì mở một bảng lệnh: với vài chục phiên thì gõ và thấy danh sách
 * ngắn lại là đủ, còn ⌘K vẫn còn đó cho lúc danh sách dài. Hai lối vào cùng một việc,
 * nhưng lối nhanh nằm ngay trên thứ nó lọc.
 */
export default function Sidebar(props: SidebarProps) {
  const [query, setQuery] = createSignal("");
  // Hàng đang mở menu phải giữ nút "…" hiện ra kể cả khi chuột đã rời đi — nếu không,
  // menu sẽ đứng lơ lửng bên cạnh một hàng trông như không có gì được chọn.
  const [menuOn, setMenuOn] = createSignal<string | null>(null);

  const matches = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (needle === "") return props.sessions;
    return props.sessions.filter((session) => session.title.toLowerCase().includes(needle));
  });

  const groups = createMemo(() => groupSessions(matches()));

  const nav = () =>
    NAV.filter(
      (item) => item.kinds === null || (props.kind !== undefined && item.kinds.includes(props.kind)),
    );

  return (
    <aside
      aria-label="Điều hướng"
      class="flex w-(--sidebar-w) shrink-0 flex-col border-r border-line bg-sidebar"
    >
      {/* Dải kéo cửa sổ, và cũng là chỗ trống cho ba nút giao thông của macOS. Nút thu gọn
          nằm ở **mép phải** của dải: mép trái đã có ba nút của hệ điều hành đè lên. */}
      <div
        class="flex h-(--titlebar-h) shrink-0 items-center justify-end pr-2xs"
        data-tauri-drag-region
      >
        <IconButton icon="panel-left" label="Thu gọn thanh bên" onClick={props.onCollapse} />
      </div>

      <div class="flex shrink-0 flex-col gap-2xs px-sm pb-sm">
        {/* Nút phiên mới là hàng rộng nhất và duy nhất mang màu nhấn: nó là việc người ta
            tới đây để làm, còn mọi thứ khác trong cột là việc quay lại chỗ đã có. */}
        <button
          type="button"
          onClick={props.onCreate}
          disabled={props.disabled}
          class="flex h-(--cta-h) w-full items-center gap-sm rounded-btn border border-line bg-surface px-sm text-left text-sm font-medium text-ink transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40 enabled:hover:border-accent enabled:hover:bg-accent-soft enabled:hover:text-accent-ink"
        >
          <span class="grid size-6 shrink-0 place-items-center rounded-btn bg-accent text-on-accent">
            <Icon name="plus" size={14} />
          </span>
          Phiên mới
        </button>

        <div class="relative">
          <span class="pointer-events-none absolute top-1/2 left-sm -translate-y-1/2 text-faint">
            <Icon name="search" size={14} />
          </span>
          <input
            type="search"
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
            placeholder="Tìm phiên…"
            aria-label="Tìm phiên theo tên"
            disabled={props.disabled}
            class="h-(--control-h) w-full rounded-btn border border-line bg-surface pr-sm pl-(--sp-2xl) text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
          />
        </div>
      </div>

      <Show when={nav().length > 0}>
        <nav aria-label="Màn hình" class="shrink-0 px-sm pb-sm">
          <ul class="m-0 flex list-none flex-col gap-3xs p-0">
            <For each={nav()}>
              {(item) => (
                <li>
                  <NavRow
                    icon={item.icon}
                    label={item.label}
                    active={props.view === item.id}
                    disabled={props.disabled}
                    badge={item.id === "diff" ? props.changeCount : 0}
                    onClick={() => props.onGo(item.id)}
                  />
                </li>
              )}
            </For>
          </ul>
        </nav>
      </Show>

      {/* Trong lúc chuyển dự án, danh sách còn là của dự án cũ. Mờ đi và không bấm được là
          cách nói "cái này sắp đổi" mà không làm cột trống rỗng một nhịp. */}
      <div
        class="flex min-h-0 flex-1 flex-col transition-opacity duration-[var(--dur-base)]"
        aria-busy={props.disabled}
        classList={{ "pointer-events-none opacity-40": props.disabled === true }}
      >
        <Show when={!props.loading} fallback={<SessionSkeleton />}>
          <Show
            when={groups().length > 0}
            fallback={
              <p class="px-md text-xs text-faint">
                {props.sessions.length === 0 ? "Chưa có phiên nào." : "Không có phiên nào khớp."}
              </p>
            }
          >
            <div class="min-h-0 flex-1 overflow-y-auto px-sm pb-md">
              <For each={groups()}>
                {(group) => (
                  <section class="mb-sm">
                    <h2 class="sticky top-0 z-10 m-0 bg-sidebar px-sm py-2xs text-2xs font-medium tracking-wide text-faint uppercase">
                      {group.label}
                    </h2>
                    <ul class="m-0 flex list-none flex-col gap-3xs p-0">
                      {/* Keyed theo `id`: danh sách xếp lại theo `updatedAt` sau mỗi lượt,
                          và keyed theo vị trí thì mọi hàng bị dựng lại — tiêu điểm bàn phím
                          rơi về `body` ngay giữa lúc người dùng đang đi bằng Tab. */}
                      <Key each={group.sessions} by="id">
                        {(session) => (
                          <SessionRow
                            session={session()}
                            active={session().id === props.currentId}
                            subtitle={props.subtitle?.(session())}
                            menuOpen={menuOn() === session().id}
                            onMenuChange={(open) => setMenuOn(open ? session().id : null)}
                            onSelect={() => props.onSelect(session().id)}
                            onRename={() => props.onRename(session().id)}
                            onDelete={() => props.onDelete(session().id)}
                          />
                        )}
                      </Key>
                    </ul>
                  </section>
                )}
              </For>
            </div>
          </Show>
        </Show>
      </div>

      {/* Chân cột: dự án đang mở, rồi hai lối đi cấu hình. Chúng ở đây vì đây là thứ người
          dùng chạm tới vài lần một tuần, còn danh sách phiên là thứ họ chạm tới vài lần
          một giờ — và cái dùng nhiều hơn phải nằm gần chỗ mắt đã đứng sẵn. */}
      <footer class="flex shrink-0 flex-col gap-3xs border-t border-line p-sm">
        <Show when={props.projectSlot}>{(slot) => slot()()}</Show>
        <NavRow
          icon="settings"
          label="Cài đặt"
          active={props.view === "settings"}
          onClick={() => props.onGo("settings")}
        />
        <NavRow
          icon={THEME_ICON[theme()]}
          label={THEME_LABEL[theme()]}
          hint="Bấm để đổi"
          onClick={() => setTheme(NEXT_THEME[theme()])}
        />
      </footer>
    </aside>
  );
}

/** Một hàng điều hướng. Biểu tượng `aria-hidden`; ý nghĩa đi qua nhãn của chính cái nút. */
function NavRow(props: {
  icon: IconName;
  label: string;
  hint?: string;
  active?: boolean;
  disabled?: boolean;
  badge?: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      disabled={props.disabled}
      aria-current={props.active ? "page" : undefined}
      aria-label={props.hint === undefined ? undefined : `${props.label}. ${props.hint}.`}
      class="flex w-full items-center gap-sm rounded-panel px-sm py-xs text-left text-sm text-text transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40 enabled:hover:bg-[var(--overlay-hover)] aria-[current=page]:bg-accent-soft aria-[current=page]:font-medium aria-[current=page]:text-accent-ink"
    >
      <span class="shrink-0 text-muted">
        <Icon name={props.icon} size={16} />
      </span>
      <span class="min-w-0 flex-1 truncate">{props.label}</span>
      {/* Số tệp đã đổi: một lượt sửa mã dài chỉ tồn tại nếu người dùng nhớ đi mở màn hình
          thay đổi ra xem, nên con số phải tự đi tìm mắt họ. */}
      <Show when={(props.badge ?? 0) > 0}>
        <span class="shrink-0 rounded-pill bg-accent px-2xs text-2xs leading-4 text-on-accent tabular-nums">
          {props.badge}
        </span>
      </Show>
    </button>
  );
}

function SessionRow(props: {
  session: SessionSummary;
  active: boolean;
  subtitle?: string;
  menuOpen: boolean;
  onMenuChange: (open: boolean) => void;
  onSelect: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  return (
    <li
      class="group relative"
      // Chuột phải mở đúng cái menu của nút "…". Hai lối vào, một menu — không có hành
      // động nào chỉ tồn tại ở một trong hai.
      onContextMenu={(event) => {
        event.preventDefault();
        props.onMenuChange(true);
      }}
    >
      <button
        type="button"
        onClick={props.onSelect}
        aria-current={props.active ? "page" : undefined}
        class="flex w-full flex-col items-start gap-3xs rounded-panel px-sm py-xs pr-(--sp-3xl) text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] aria-[current=page]:bg-accent-soft"
      >
        <span class="flex w-full min-w-0 items-baseline gap-sm">
          <span
            class="min-w-0 flex-1 truncate text-sm"
            classList={{
              "text-text": !props.active,
              "font-medium text-accent-ink": props.active,
            }}
          >
            {props.session.title}
          </span>
          <span class="shrink-0 text-2xs whitespace-nowrap text-faint tabular-nums">
            {relativeTime(props.session.updatedAt)}
          </span>
        </span>
        <Show when={props.subtitle}>
          {(text) => <span class="w-full truncate text-2xs text-muted">{text()}</span>}
        </Show>
      </button>

      <div
        class="absolute top-xs right-2xs transition-opacity duration-[var(--dur-fast)] group-hover:opacity-100 group-focus-within:opacity-100"
        classList={{ "opacity-0": !props.menuOpen, "opacity-100": props.menuOpen }}
      >
        <Menu
          label={`Tuỳ chọn cho ${props.session.title}`}
          open={props.menuOpen}
          onOpenChange={props.onMenuChange}
          onRequestClose={() => props.onMenuChange(false)}
          items={[
            { id: "rename", label: "Đổi tên", icon: "document", onSelect: props.onRename },
            { id: "delete", label: "Xoá phiên", icon: "trash", danger: true, onSelect: props.onDelete },
          ]}
        />
      </div>
    </li>
  );
}

/**
 * Khung xương lúc nạp danh sách.
 *
 * Cùng chiều cao hàng với danh sách thật, nếu không thì lúc dữ liệu về mọi thứ nhảy một
 * nhịp — và cú nhảy đó đắt hơn hẳn khoảng lặng mà khung xương che đi.
 */
export function SessionSkeleton(props: { rows?: number }) {
  return (
    <div class="flex flex-col gap-2xs px-md" aria-hidden="true">
      <For each={Array.from({ length: props.rows ?? 6 })}>
        {(_, index) => (
          <div class="flex flex-col gap-2xs py-xs">
            <div class="flex items-center gap-sm">
              <div
                class="h-3 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse"
                style={{ width: `${[68, 82, 55, 74, 60, 88][index() % 6]}%` }}
              />
            </div>
            <div class="h-2.5 w-2/5 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
          </div>
        )}
      </For>
    </div>
  );
}
