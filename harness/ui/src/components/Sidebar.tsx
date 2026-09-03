import { Key } from "@solid-primitives/keyed";
import { createMemo, createSignal, For, Show, type JSX } from "solid-js";
import type { ProjectKind, SessionSummary } from "../lib/protocol";
import { groupSessions, relativeTime } from "../lib/sessions";
import { setTheme, theme, type ThemeChoice } from "../lib/theme";
import { BrandLockup } from "./Brand";
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
 * Màn hình chỉ sống được **bên trong một dự án**, và **loại dự án nào thấy được nó**.
 *
 * Chúng không còn đứng trong nhóm điều hướng chung: cả hai chỉ có nghĩa khi có một thư mục
 * đang mở, nên chỗ đúng của chúng là *thụt vào dưới dự án đang mở* — đọc một lần là biết
 * chúng thuộc về ai. Đứng ngang hàng với "Phiên mới" thì chúng trông như hai màn hình toàn
 * cục lúc có lúc không, và cái "lúc không" ấy không giải thích được cho ai cả.
 *
 * Cắt mục **của loại kia** đi chứ không làm mờ nó: một hàng mờ nói rằng có thứ gì đó đang
 * bị khoá và người dùng phải tìm cách mở, trong khi sự thật là nó không áp dụng ở đây.
 */
const PROJECT_TABS: { id: TabId; label: string; icon: IconName; kinds: ProjectKind[] }[] = [
  { id: "diff", label: "Thay đổi", icon: "diff", kinds: ["code"] },
  { id: "library", label: "Thư viện tài liệu", icon: "library", kinds: ["docs"] },
];

/** Mục con của dự án đang mở. Không có dự án (`kind` vắng) thì không có mục nào. */
export function projectTabs(
  kind: ProjectKind | undefined,
): { id: TabId; label: string; icon: IconName }[] {
  return PROJECT_TABS.filter((item) => kind !== undefined && item.kinds.includes(kind)).map(
    (item) => ({ id: item.id, label: item.label, icon: item.icon }),
  );
}

/** Màn hình mở được với một loại dự án. `App` dùng nó để sửa màn hình khi đổi dự án. */
export function tabsFor(kind: ProjectKind | undefined): TabId[] {
  return ["chat", ...projectTabs(kind).map((item) => item.id), "projects", "settings"];
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
  /** Số server MCP đang nối, làm huy hiệu cho hàng "Server MCP". */
  mcpCount?: number;
  /**
   * Nhóm "Dự án", đặt giữa nhóm điều hướng và danh sách phiên.
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
  projectsSlot?: () => JSX.Element;
  /** Đang chuyển dự án: danh sách dưới đây đang nói về dự án sắp không còn mở nữa. */
  disabled?: boolean;
  /** Dòng phụ của mỗi phiên: câu cuối đã nói. Thiếu thì bỏ dòng đó. */
  subtitle?: (session: SessionSummary) => string | undefined;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string) => void;
  onDelete: (id: string) => void;
  onGo: (view: TabId) => void;
  /** Server MCP — plugin của ứng dụng này. Dẫn thẳng tới trang MCP trong Cài đặt. */
  onOpenMcp: () => void;
  onCollapse: () => void;
}

/**
 * Thanh bên — cột duy nhất bên trái, và là toàn bộ hệ điều hướng của ứng dụng.
 *
 * Xếp theo đúng thứ tự của ChatGPT desktop, vì thứ tự ấy kể một câu chuyện: hàng đầu là
 * *ứng dụng nào*, rồi tới *việc làm được ngay*, rồi *dự án*, rồi *đã làm gì*, rồi mới tới
 * cấu hình ở chân cột. Mỗi nhóm có một tiêu đề chữ nhỏ màu mờ — tiêu đề là **nhãn**, không
 * phải nút: bấm được vào một tiêu đề nhóm thì người ta sẽ bấm, và không có gì xảy ra.
 *
 * Ô lọc phiên nấp sau biểu tượng kính lúp ở hàng đầu thay vì chiếm một hàng cố định: với
 * vài chục phiên thì nó không được dùng tới trong phần lớn thời gian, mà một ô nhập rỗng
 * đứng thường trực vẫn tốn đúng chỗ như một ô đang dùng. ⌘K vẫn còn đó cho lúc danh sách dài.
 */
export default function Sidebar(props: SidebarProps) {
  const [query, setQuery] = createSignal("");
  const [searching, setSearching] = createSignal(false);
  // Hàng đang mở menu phải giữ nút "…" hiện ra kể cả khi chuột đã rời đi — nếu không,
  // menu sẽ đứng lơ lửng bên cạnh một hàng trông như không có gì được chọn.
  const [menuOn, setMenuOn] = createSignal<string | null>(null);
  let searchField: HTMLInputElement | undefined;

  const toggleSearch = () => {
    const next = !searching();
    setSearching(next);
    // Đóng ô lọc mà giữ lại chuỗi đang lọc là giấu mất lý do danh sách còn ba hàng.
    if (!next) setQuery("");
    else queueMicrotask(() => searchField?.focus());
  };

  const matches = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (needle === "") return props.sessions;
    return props.sessions.filter((session) => session.title.toLowerCase().includes(needle));
  });

  const groups = createMemo(() => groupSessions(matches()));

  return (
    <aside
      aria-label="Điều hướng"
      class="flex w-(--sidebar-w) shrink-0 flex-col border-r border-line bg-sidebar"
    >
      {/* Dải kéo cửa sổ, và cũng là chỗ trống cho ba nút giao thông của macOS. Nó ở lại
          rỗng: mọi thứ ta muốn đặt vào đây sẽ nằm dưới ba cái nút ấy. */}
      <div class="h-(--titlebar-h) shrink-0" data-tauri-drag-region />

      {/* Hàng đầu: dấu hiệu thương hiệu bên trái, hai nút nhỏ bên phải.
          `pb-xs` chứ không `pb-2xs`: hàng này giờ cao hơn hàng điều hướng bên dưới, và
          hai hàng cao gần bằng nhau dính sát nhau thì cái trên đọc ra là mục đầu tiên
          của danh sách chứ không ra là đầu đề của cả cột. */}
      <div class="flex shrink-0 items-center gap-2xs px-sm pb-xs">
        <BrandLockup class="flex-1" />
        <IconButton
          icon="search"
          label={searching() ? "Đóng ô tìm phiên" : "Tìm phiên"}
          size="sm"
          active={searching()}
          expanded={searching()}
          onClick={toggleSearch}
        />
        <IconButton icon="panel-left" label="Thu gọn thanh bên" size="sm" onClick={props.onCollapse} />
      </div>

      <Show when={searching()}>
        <div class="shrink-0 px-sm pb-2xs">
          <input
            ref={searchField}
            type="search"
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                toggleSearch();
              }
            }}
            placeholder="Tìm phiên…"
            aria-label="Tìm phiên theo tên"
            disabled={props.disabled}
            class="h-(--control-h) w-full rounded-btn border border-line bg-surface px-sm text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
          />
        </div>
      </Show>

      {/* Từ đây xuống hết "Gần đây" là một vùng cuộn duy nhất: dự án và phiên cùng dài ra
          theo thời gian, và hai vùng cuộn lồng nhau trong một cột 260px là hai thanh cuộn
          không ai bắt trúng. */}
      <div
        class="flex min-h-0 flex-1 flex-col overflow-y-auto transition-opacity duration-[var(--dur-base)]"
        aria-busy={props.disabled}
        classList={{ "pointer-events-none opacity-40": props.disabled === true }}
      >
        <nav aria-label="Điều hướng chính" class="shrink-0 px-sm pb-sm">
          <ul class="m-0 flex list-none flex-col gap-3xs p-0">
            <li>
              <NavRow
                icon="plus"
                label="Phiên mới"
                disabled={props.disabled}
                onClick={props.onCreate}
              />
            </li>
            <li>
              {/* Server MCP là chỗ của ta tương ứng với "Plugins": mỗi server cắm thêm một
                  rổ tool vào trợ lý, và đó đúng là định nghĩa của plugin ở đây. */}
              <NavRow
                icon="plug"
                label="Server MCP"
                badge={props.mcpCount ?? 0}
                disabled={props.disabled}
                onClick={props.onOpenMcp}
              />
            </li>
          </ul>
        </nav>

        <Show when={props.projectsSlot}>
          {(slot) => (
            <section class="shrink-0 px-sm pb-sm">
              <GroupTitle>Dự án</GroupTitle>
              {slot()()}
            </section>
          )}
        </Show>

        <section class="shrink-0 px-sm pb-md">
          <GroupTitle>Gần đây</GroupTitle>
          <Show when={!props.loading} fallback={<SessionSkeleton />}>
            <Show
              when={groups().length > 0}
              fallback={
                <p class="m-0 flex items-center gap-2xs px-sm py-xs text-2xs text-faint">
                  <Icon name={props.sessions.length === 0 ? "bubble" : "search"} size={12} />
                  {props.sessions.length === 0 ? "Chưa có phiên nào." : "Không có phiên nào khớp."}
                </p>
              }
            >
              <For each={groups()}>
                {(group) => (
                  <div class="mb-xs">
                    {/* Nhóm theo ngày nằm **dưới** tiêu đề "Gần đây" và nhỏ hơn nó một bậc:
                        hai cấp tiêu đề cùng cỡ chữ đọc ra là hai nhóm ngang hàng. */}
                    <h3 class="sticky top-0 z-10 m-0 bg-sidebar px-sm py-3xs text-[10px] font-medium tracking-wide text-faint uppercase">
                      {group.label}
                    </h3>
                    <ul class="m-0 flex list-none flex-col gap-3xs p-0">
                      {/* Keyed theo `id`: danh sách xếp lại theo `updatedAt` sau mỗi lượt,
                          và keyed theo vị trí thì mọi hàng bị dựng lại — tiêu điểm bàn phím
                          rơi về `body` ngay giữa lúc người dùng đang đi bằng Tab. */}
                      <Key each={group.sessions} by="id">
                        {(session) => (
                          <SessionRow
                            session={session()}
                            active={props.view === "chat" && session().id === props.currentId}
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
                  </div>
                )}
              </For>
            </Show>
          </Show>
        </section>
      </div>

      {/* Chân cột, **một hàng**: lối vào cấu hình cộng công tắc sáng/tối. Cả hai là thứ
          người dùng chạm tới vài lần một tuần, còn danh sách phiên là thứ họ chạm tới vài
          lần một giờ — và cái dùng nhiều hơn phải nằm gần chỗ mắt đã đứng sẵn. */}
      <footer class="flex shrink-0 items-center gap-2xs border-t border-line p-sm">
        <span class="min-w-0 flex-1">
          <NavRow
            icon="settings"
            label="Cài đặt"
            active={props.view === "settings"}
            onClick={() => props.onGo("settings")}
          />
        </span>
        <IconButton
          icon={THEME_ICON[theme()]}
          label={`${THEME_LABEL[theme()]}. Bấm để đổi.`}
          size="sm"
          onClick={() => setTheme(NEXT_THEME[theme()])}
        />
      </footer>
    </aside>
  );
}

/**
 * Tiêu đề một nhóm trong cột.
 *
 * Là chữ, không phải nút: mọi thứ bấm được trong cột này đều dẫn đi đâu đó, và một tiêu đề
 * bấm vào không đi đâu dạy người dùng rằng cột này có những chỗ chết.
 */
function GroupTitle(props: { children: JSX.Element }) {
  return (
    <h2 class="m-0 px-sm py-2xs text-2xs font-medium text-faint">{props.children}</h2>
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
      class="flex w-full items-center gap-sm rounded-panel px-sm py-2xs text-left text-sm text-text transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40 enabled:hover:bg-[var(--overlay-hover)] aria-[current=page]:bg-accent-soft aria-[current=page]:font-medium aria-[current=page]:text-accent-ink"
    >
      <span class="shrink-0 text-muted">
        <Icon name={props.icon} size={16} />
      </span>
      <span class="min-w-0 flex-1 truncate">{props.label}</span>
      {/* Con số phải tự đi tìm mắt người dùng: số tệp đã đổi và số server đang nối đều là
          thứ chỉ tồn tại nếu có ai đó nhớ đi mở màn hình tương ứng ra xem. */}
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
        class="flex w-full flex-col items-start gap-3xs rounded-panel px-sm py-2xs pr-(--sp-3xl) text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] aria-[current=page]:bg-accent-soft"
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
        class="absolute top-2xs right-2xs transition-opacity duration-[var(--dur-fast)] group-hover:opacity-100 group-focus-within:opacity-100"
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
function SessionSkeleton(props: { rows?: number }) {
  return (
    <div class="flex flex-col gap-2xs px-sm" aria-hidden="true">
      <For each={Array.from({ length: props.rows ?? 6 })}>
        {(_, index) => (
          <div class="flex flex-col gap-2xs py-2xs">
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
