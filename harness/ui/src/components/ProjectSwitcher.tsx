import { createUniqueId, For, onCleanup, Show } from "solid-js";
import type { Project } from "../lib/protocol";
import { relativeTime } from "../lib/sessions";
import Icon from "./Icon";

/**
 * Nút chọn dự án, ngồi trên đầu thanh bên.
 *
 * Không dùng `Menu` chung được: menu ở đây có hai hành động trên **cùng một hàng** (mở
 * dự án, và bỏ nó khỏi danh sách), còn `Menu` chỉ nhận một danh sách phẳng mỗi hàng một
 * việc. Nhét việc thứ hai vào đó thì mỗi dự án chiếm hai dòng, và danh sách dài gấp đôi
 * chỉ để nói lại cùng một cái tên.
 *
 * Hành vi bàn phím và cú bấm ra ngoài thì chép nguyên của `Menu` — người dùng không nên
 * đoán được hai menu này do hai đoạn mã khác nhau vẽ.
 */
export default function ProjectSwitcher(props: {
  projects: Project[];
  current: Project | null;
  /** Lõi đang tháo và cắm lại nhánh plugin. Trong lúc đó mọi thứ ở đây đều khoá. */
  switching: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (id: string) => void;
  onOpenFolder: () => void;
  onForget: (project: Project) => void;
}) {
  const id = createUniqueId();
  let popup: HTMLDivElement | undefined;
  let trigger: HTMLButtonElement | undefined;

  const onDocPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (popup?.contains(target ?? null) || trigger?.contains(target ?? null)) return;
    props.onOpenChange(false);
  };
  document.addEventListener("pointerdown", onDocPointerDown, true);
  onCleanup(() => document.removeEventListener("pointerdown", onDocPointerDown, true));

  const move = (delta: number) => {
    const buttons = [...(popup?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
    if (buttons.length === 0) return;
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[(at + delta + buttons.length) % buttons.length]?.focus();
  };

  const close = (restore: boolean) => {
    props.onOpenChange(false);
    if (restore) trigger?.focus();
  };

  // Mới nhất trước. Dự án đang mở vẫn nằm đúng chỗ của nó theo thời gian chứ không bị
  // ghim lên đầu: nó đã có tên trên nút rồi, ghim thêm chỉ làm thứ tự đổi mỗi lần mở.
  const ordered = () => [...props.projects].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);

  return (
    <div class="relative">
      <button
        ref={trigger}
        type="button"
        disabled={props.switching}
        aria-haspopup="menu"
        aria-expanded={props.open}
        aria-controls={id}
        aria-label={`Dự án: ${props.current?.name ?? "chưa mở dự án nào"}. Bấm để đổi.`}
        onClick={() => props.onOpenChange(!props.open)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            props.onOpenChange(true);
            queueMicrotask(() => move(1));
          }
        }}
        class="flex w-full items-center gap-sm rounded-panel px-sm py-xs text-left transition-colors duration-[var(--dur-fast)] disabled:cursor-progress enabled:hover:bg-[var(--overlay-hover)]"
      >
        <span
          class="grid size-7 shrink-0 place-items-center rounded-btn bg-accent-soft text-accent-ink"
          classList={{ "motion-safe:animate-pulse": props.switching }}
        >
          <Icon name={props.switching ? "clock" : "folder-open"} size={15} />
        </span>
        <span class="flex min-w-0 flex-1 flex-col">
          <span class="min-w-0 truncate text-sm font-medium text-ink">
            {props.current?.name ?? "Chưa mở dự án"}
          </span>
          {/* Đường dẫn cắt ở *đầu*: hai dự án cùng tên chỉ khác nhau ở phần đuôi. */}
          <span
            class="min-w-0 truncate text-2xs text-faint"
            dir="rtl"
            title={props.current?.path}
          >
            <bdi>{props.switching ? "đang chuyển dự án…" : (props.current?.path ?? "—")}</bdi>
          </span>
        </span>
        <span class="shrink-0 text-faint">
          <Icon name="chevron-down" size={13} />
        </span>
      </button>

      <Show when={props.open && !props.switching}>
        <div
          ref={popup}
          id={id}
          role="menu"
          aria-label="Dự án"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              close(true);
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              move(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              move(-1);
            }
          }}
          class="absolute top-full right-0 left-0 z-40 mt-3xs flex flex-col rounded-menu border border-line bg-surface p-3xs shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        >
          <Show
            when={ordered().length > 0}
            fallback={<p class="px-sm py-xs text-2xs text-faint">Chưa mở dự án nào.</p>}
          >
            <ul class="m-0 flex max-h-64 list-none flex-col gap-3xs overflow-y-auto p-0">
              <For each={ordered()}>
                {(project) => (
                  <li class="group/row relative flex items-center">
                    <button
                      type="button"
                      role="menuitem"
                      onClick={() => {
                        close(false);
                        if (!project.isCurrent) props.onPick(project.id);
                      }}
                      aria-current={project.isCurrent ? "true" : undefined}
                      class="flex min-w-0 flex-1 items-center gap-sm rounded-btn py-2xs pr-(--sp-3xl) pl-sm text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] aria-[current]:bg-accent-soft"
                    >
                      <span
                        class="shrink-0"
                        classList={{
                          "text-accent-ink": project.isCurrent,
                          "text-faint": !project.isCurrent,
                        }}
                      >
                        <Icon name={project.isCurrent ? "folder-open" : "folder"} size={14} />
                      </span>
                      <span class="flex min-w-0 flex-1 flex-col">
                        <span class="min-w-0 truncate text-xs text-text">{project.name}</span>
                        <span class="min-w-0 truncate text-2xs text-faint" dir="rtl" title={project.path}>
                          <bdi>{project.path}</bdi>
                        </span>
                      </span>
                      <span class="shrink-0 text-2xs whitespace-nowrap text-faint tabular-nums">
                        {relativeTime(project.lastOpenedAt)}
                      </span>
                    </button>

                    {/* Bỏ dự án đang mở là bỏ chỗ đứng của chính mình: nút vẫn ở đó cho
                        hàng khỏi lệch, nhưng khoá và nói ra lý do. */}
                    <button
                      type="button"
                      disabled={project.isCurrent}
                      onClick={(event) => {
                        event.stopPropagation();
                        close(false);
                        props.onForget(project);
                      }}
                      aria-label={
                        project.isCurrent
                          ? `Không bỏ được "${project.name}" vì đang mở`
                          : `Bỏ "${project.name}" khỏi danh sách. Thư mục trên đĩa không bị xoá.`
                      }
                      title={
                        project.isCurrent
                          ? "Đang mở — không bỏ khỏi danh sách được"
                          : "Bỏ khỏi danh sách (không xoá thư mục)"
                      }
                      class="absolute right-2xs grid size-6 place-items-center rounded-icon text-faint opacity-0 transition-colors duration-[var(--dur-fast)] group-focus-within/row:opacity-100 group-hover/row:opacity-100 enabled:hover:bg-danger-soft enabled:hover:text-danger disabled:opacity-0"
                    >
                      <Icon name="x" size={13} />
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>

          <div class="mt-3xs border-t border-line pt-3xs">
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                close(false);
                props.onOpenFolder();
              }}
              class="flex w-full items-center gap-sm rounded-btn px-sm py-2xs text-left text-xs text-text transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
            >
              <Icon name="plus" size={14} />
              Mở thư mục…
            </button>
            {/* Câu này ở lại trong menu chứ không chỉ nằm trong hộp xác nhận: người ta
                quyết định có bấm hay không *trước* khi hộp xác nhận kịp hiện ra. */}
            <p class="m-0 px-sm py-3xs text-2xs text-faint">
              Bỏ một dự án khỏi danh sách không xoá bất cứ thứ gì trên đĩa.
            </p>
          </div>
        </div>
      </Show>
    </div>
  );
}
