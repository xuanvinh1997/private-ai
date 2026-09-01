import { createMemo, createSignal, For, Show, type JSX } from "solid-js";
import type { SessionSummary } from "../lib/protocol";
import { groupSessions, relativeTime } from "../lib/sessions";
import Icon from "./Icon";
import Menu from "./Menu";
import { IconButton } from "./primitives";

export interface SessionPanelProps {
  sessions: SessionSummary[];
  currentId: string;
  loading: boolean;
  /**
   * Ô chọn dự án, đặt ở đầu cột.
   *
   * Nhận vào như một mảnh JSX thay vì như dữ liệu dự án: cột này biết về phiên, và cho
   * nó biết thêm về dự án nghĩa là mỗi lần hợp đồng dự án đổi thì cả danh sách phiên
   * phải biên dịch lại cùng.
   *
   * Là **hàm** chứ không phải `JSX.Element`, và đó không phải chuyện phong cách: Solid
   * biên dịch prop mang JSX thành getter, nên mỗi lần đọc prop là một lần dựng component
   * mới. Đọc nó hai lần — một cho `Show`, một để render — sinh ra hai bản; bản không được
   * gắn vào DOM vẫn kịp đăng ký listener toàn cục của nó và phá bản thật.
   */
  projectSlot?: () => JSX.Element;
  /** Đang chuyển dự án: danh sách dưới đây đang nói về dự án sắp không còn mở nữa. */
  disabled?: boolean;
  /** Dòng phụ của mỗi phiên: câu cuối, hoặc số tệp đã đổi. Thiếu thì bỏ dòng đó. */
  subtitle?: (session: SessionSummary) => string | undefined;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string) => void;
  onDelete: (id: string) => void;
  onCollapse: () => void;
}

/**
 * Cột danh sách phiên.
 *
 * Ô tìm lọc tại chỗ thay vì mở một bảng lệnh: với vài chục phiên thì gõ và thấy danh
 * sách ngắn lại là đủ, còn ⌘K vẫn còn đó cho lúc danh sách dài. Hai lối vào cùng một
 * việc, nhưng lối nhanh nằm ngay trên thứ nó lọc.
 */
export default function SessionPanel(props: SessionPanelProps) {
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

  return (
    <aside
      aria-label="Phiên làm việc"
      class="flex w-(--session-col-w) shrink-0 flex-col border-r border-line bg-sidebar"
    >
      <div class="h-(--titlebar-h) shrink-0" data-tauri-drag-region />

      <Show when={props.projectSlot}>
        {(slot) => <div class="shrink-0 border-b border-line px-sm pb-sm">{slot()()}</div>}
      </Show>

      <header class="flex shrink-0 items-center gap-2xs px-md pt-sm pb-sm">
        <div class="relative min-w-0 flex-1">
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
        <IconButton
          icon="plus"
          label="Phiên mới"
          disabled={props.disabled}
          onClick={props.onCreate}
        />
        <IconButton icon="panel-left" label="Thu gọn danh sách phiên" onClick={props.onCollapse} />
      </header>

      {/* Trong lúc chuyển dự án, danh sách còn là của dự án cũ. Mờ đi và không bấm được
          là cách nói "cái này sắp đổi" mà không làm cột trống rỗng một nhịp. */}
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
                    <For each={group.sessions}>
                      {(session) => (
                        <SessionRow
                          session={session}
                          active={session.id === props.currentId}
                          subtitle={props.subtitle?.(session)}
                          menuOpen={menuOn() === session.id}
                          onMenuChange={(open) => setMenuOn(open ? session.id : null)}
                          onSelect={() => props.onSelect(session.id)}
                          onRename={() => props.onRename(session.id)}
                          onDelete={() => props.onDelete(session.id)}
                        />
                      )}
                    </For>
                  </ul>
                </section>
              )}
            </For>
          </div>
        </Show>
      </Show>
      </div>
    </aside>
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
