import { createUniqueId, For, onCleanup, Show } from "solid-js";
import type { Project, ProjectKind } from "../lib/protocol";
import { relativeTime } from "../lib/sessions";
import Icon from "./Icon";

/**
 * Nút chọn dự án, ngồi ở **chân** thanh bên — chỗ Codex để bộ chọn kho/môi trường.
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
  /** Mở màn hình dự án — chỗ tạo mới, clone, và lọc theo loại. */
  onSeeAll: () => void;
  onForget: (project: Project) => void;
  /** Đóng dự án đang mở. Danh sách giữ nguyên — đây không phải `onForget`. */
  onClose: () => void;
  /** Đổi loại dự án đang mở. */
  onSwapKind: (kind: ProjectKind) => void;
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
        aria-label={
          props.current
            ? `Dự án: ${props.current.name}. Bấm để đổi.`
            : "Chưa mở dự án nào. Trợ lý chỉ trò chuyện. Bấm để mở một dự án."
        }
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
        {/* Không dự án thì cả ô này đổi màu chứ không chỉ đổi chữ: màu nhấn nói "đang có
            một thứ đang mở", và giữ nó lại cho một chỗ trống là nói sai. */}
        <span
          class="grid size-7 shrink-0 place-items-center rounded-btn"
          classList={{
            "motion-safe:animate-pulse": props.switching,
            "bg-accent-soft text-accent-ink": props.current !== null,
            "bg-[var(--overlay-faint)] text-muted": props.current === null,
          }}
        >
          <Icon
            name={props.switching ? "clock" : props.current ? "folder-open" : "chat"}
            size={15}
          />
        </span>
        <span class="flex min-w-0 flex-1 flex-col">
          <span
            class="min-w-0 truncate text-sm font-medium"
            classList={{ "text-ink": props.current !== null, "text-muted": props.current === null }}
          >
            {props.current?.name ?? "Chưa mở dự án"}
          </span>
          {/* Dòng dưới **luôn** nói một điều gì đó. Một gạch ngang ở đây đọc ra là "chưa
              nạp xong", và người dùng sẽ ngồi đợi một thứ không bao giờ tới. Đường dẫn thì
              cắt ở *đầu*: hai dự án cùng tên chỉ khác nhau ở phần đuôi. */}
          <Show
            when={props.current}
            fallback={
              <span class="min-w-0 truncate text-2xs text-faint">
                {props.switching ? "đang chuyển dự án…" : "Chỉ trò chuyện, không đọc tệp"}
              </span>
            }
          >
            {(current) => (
              <span class="min-w-0 truncate text-2xs text-faint" dir="rtl" title={current().path}>
                <bdi>{props.switching ? "đang chuyển dự án…" : current().path}</bdi>
              </span>
            )}
          </Show>
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
          // Bung **lên**: nút này ngồi ở chân cột, và một menu bung xuống từ đó rơi thẳng
          // ra ngoài cửa sổ.
          class="absolute right-0 bottom-full left-0 z-40 mb-3xs flex flex-col rounded-menu border border-line bg-surface p-3xs shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        >
          <Show
            when={ordered().length > 0}
            fallback={<p class="px-sm py-xs text-2xs text-faint">Danh sách chưa có dự án nào.</p>}
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
            {/* Một lối ra duy nhất, và nó dẫn tới màn hình dự án chứ không tới một hộp
                thoại thứ hai: tạo mới, clone và lọc theo loại đều đã sống ở đó, và một
                hộp thoại "mở thư mục" riêng ở đây chỉ là lối thứ tư làm cùng một việc. */}
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                close(false);
                props.onSeeAll();
              }}
              class="flex w-full items-center gap-sm rounded-btn px-sm py-2xs text-left text-xs text-text transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
            >
              <Icon name="folder-open" size={14} />
              Tất cả dự án…
            </button>
            {/* Câu này ở lại trong menu chứ không chỉ nằm trong hộp xác nhận: người ta
                quyết định có bấm hay không *trước* khi hộp xác nhận kịp hiện ra. */}
            <p class="m-0 px-sm py-3xs text-2xs text-faint">
              Bỏ một dự án khỏi danh sách không xoá bất cứ thứ gì trên đĩa.
            </p>
          </div>

          {/* "Đóng" và "bỏ khỏi danh sách" là hai việc khác nhau nằm trong cùng một menu,
              và đó là chỗ dễ bấm nhầm nhất màn hình này có. Ba thứ tách chúng ra: nút bỏ
              là dấu × nằm *trên hàng của dự án*, nút đóng nằm dưới cùng sau một đường kẻ
              riêng, và mỗi cái tự nói ra hậu quả của mình ngay dưới nhãn. */}
          <Show when={props.current}>
            {(current) => (
              <div class="mt-3xs border-t border-line pt-3xs">
                {/* Loại được đặt một lần lúc ghi nhận và mở lại thì giữ nguyên. Không có
                    nút này thì một thư mục vào nhầm loại là ngõ cụt vĩnh viễn: một repo
                    lỡ ghi nhận thành thư viện tài liệu sẽ không bao giờ có `read` hay
                    `bash`, và người dùng chỉ thấy trợ lý nói nó không có tool nào. */}
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    close(false);
                    props.onSwapKind(current().kind === "code" ? "docs" : "code");
                  }}
                  class="flex w-full flex-col gap-3xs rounded-btn px-sm py-2xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
                >
                  <span class="flex items-center gap-sm text-xs text-text">
                    <Icon name={current().kind === "code" ? "library" : "code"} size={14} />
                    {current().kind === "code"
                      ? "Chuyển thành thư viện tài liệu"
                      : "Chuyển thành dự án mã nguồn"}
                  </span>
                  <span class="text-2xs text-faint">
                    {current().kind === "code"
                      ? "Trợ lý thôi sửa tệp và chạy lệnh; nó tìm và đọc tài liệu trong thư mục."
                      : "Trợ lý đọc, sửa được tệp và chạy được lệnh trong thư mục này."}
                  </span>
                </button>
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    close(false);
                    props.onClose();
                  }}
                  aria-label={`Đóng dự án "${current().name}". Vẫn giữ trong danh sách.`}
                  class="flex w-full flex-col gap-3xs rounded-btn px-sm py-2xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
                >
                  <span class="flex items-center gap-sm text-xs text-text">
                    <Icon name="folder" size={14} />
                    Đóng dự án, chỉ trò chuyện
                  </span>
                  <span class="text-2xs text-faint">
                    Vẫn giữ trong danh sách. Trợ lý thôi đọc và sửa tệp cho tới khi mở lại.
                  </span>
                </button>
              </div>
            )}
          </Show>
        </div>
      </Show>
    </div>
  );
}
