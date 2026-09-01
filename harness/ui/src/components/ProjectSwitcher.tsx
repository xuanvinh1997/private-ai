import { Key } from "@solid-primitives/keyed";
import { createSignal, For, Show } from "solid-js";
import type { Project, ProjectKind } from "../lib/protocol";
import Icon, { type IconName } from "./Icon";
import Menu, { type MenuItem } from "./Menu";

/** Mục con của dự án đang mở. `App` dựng chúng từ `projectTabs`, cột này không biết `TabId`. */
export interface ProjectSubItem {
  id: string;
  label: string;
  icon: IconName;
  badge?: number;
  active?: boolean;
  onSelect: () => void;
}

/** Bao nhiêu dự án hiện ra trước khi phải bấm "Xem thêm". */
const HIEN_TRUOC = 5;

/**
 * Nhóm "Dự án" trong thanh bên.
 *
 * Trước đây đây là một cái nút ở **chân** cột mở ra một menu, và cái menu ấy là lối vào duy
 * nhất tới danh sách dự án. Một danh sách chỉ tồn tại sau một cú bấm là một danh sách không
 * ai nhớ mình có gì trong đó; ChatGPT để dự án thành một nhóm hạng nhất giữa cột, và cái
 * được không phải là chỗ ngồi đẹp hơn mà là **thấy được mà không phải hỏi**.
 *
 * Không hành động nào của cái menu cũ bị bỏ đi: mở dự án là cú bấm vào chính hàng đó, còn
 * đổi loại / đóng / bỏ khỏi danh sách chuyển vào menu ngữ cảnh của từng hàng — cùng chỗ,
 * cùng câu giải thích hậu quả, thêm được cả chuột phải. "Tất cả dự án…" ở lại làm hàng cuối
 * của nhóm.
 */
export default function ProjectSwitcher(props: {
  projects: Project[];
  current: Project | null;
  /** Lõi đang tháo và cắm lại nhánh plugin. Trong lúc đó mọi thứ ở đây đều khoá. */
  switching: boolean;
  /** Hàng nào đang mở menu ngữ cảnh, theo id dự án. */
  menuFor: string | null;
  onMenuChange: (id: string | null) => void;
  /** Màn hình chỉ mở được bên trong dự án đang mở, thụt vào dưới nó. */
  subItems: ProjectSubItem[];
  onPick: (id: string) => void;
  /** Mở màn hình dự án — chỗ tạo mới, clone, và lọc theo loại. */
  onSeeAll: () => void;
  onForget: (project: Project) => void;
  /** Đóng dự án đang mở. Danh sách giữ nguyên — đây không phải `onForget`. */
  onClose: () => void;
  /** Đổi loại dự án đang mở. */
  onSwapKind: (kind: ProjectKind) => void;
}) {
  const [expanded, setExpanded] = createSignal(false);

  // Mới nhất trước. Dự án đang mở vẫn nằm đúng chỗ của nó theo thời gian chứ không bị
  // ghim lên đầu: nó đã được đánh dấu bằng màu và biểu tượng rồi, ghim thêm chỉ làm thứ
  // tự đổi mỗi lần mở.
  const ordered = () => [...props.projects].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  const visible = () => (expanded() ? ordered() : ordered().slice(0, HIEN_TRUOC));
  const hidden = () => ordered().length - visible().length;

  /**
   * Menu ngữ cảnh của một hàng.
   *
   * Hàng của dự án **đang mở** mang ba việc, và đó là ba việc dễ bấm nhầm nhau nhất màn
   * hình này có — nên mỗi cái tự nói ra hậu quả của mình ngay dưới nhãn, và cái phá huỷ
   * nhất nằm cuối cùng, mang màu cảnh báo.
   */
  const itemsFor = (project: Project): MenuItem[] => {
    const items: MenuItem[] = [];
    if (project.isCurrent) {
      // Loại được đặt một lần lúc ghi nhận và mở lại thì giữ nguyên. Không có hàng này thì
      // một thư mục vào nhầm loại là ngõ cụt vĩnh viễn: một repo lỡ ghi nhận thành thư viện
      // tài liệu sẽ không bao giờ có `read` hay `bash`, và người dùng chỉ thấy trợ lý nói
      // nó không có tool nào.
      items.push({
        id: "kind",
        label:
          project.kind === "code" ? "Chuyển thành thư viện tài liệu" : "Chuyển thành dự án mã nguồn",
        icon: project.kind === "code" ? "library" : "code",
        hint:
          project.kind === "code"
            ? "Trợ lý thôi sửa tệp và chạy lệnh; nó tìm và đọc tài liệu trong thư mục."
            : "Trợ lý đọc, sửa được tệp và chạy được lệnh trong thư mục này.",
        onSelect: () => props.onSwapKind(project.kind === "code" ? "docs" : "code"),
      });
      items.push({
        id: "close",
        label: "Đóng dự án, chỉ trò chuyện",
        icon: "folder",
        hint: "Vẫn giữ trong danh sách. Trợ lý thôi đọc và sửa tệp cho tới khi mở lại.",
        onSelect: props.onClose,
      });
    } else {
      items.push({
        id: "open",
        label: "Mở dự án này",
        icon: "folder-open",
        onSelect: () => props.onPick(project.id),
      });
    }
    // Bỏ dự án đang mở là bỏ chỗ đứng của chính mình: hàng vẫn ở đó cho menu khỏi đổi hình
    // giữa hai trạng thái, nhưng khoá lại và nói ra lý do.
    items.push({
      id: "forget",
      label: "Bỏ khỏi danh sách",
      icon: "x",
      danger: !project.isCurrent,
      disabled: project.isCurrent,
      hint: project.isCurrent
        ? "Đang mở — đóng dự án trước đã."
        : "Thư mục trên đĩa vẫn nguyên, không tệp nào bị xoá.",
      onSelect: () => props.onForget(project),
    });
    return items;
  };

  return (
    <div class="flex flex-col gap-3xs">
      <Show
        when={ordered().length > 0}
        fallback={<p class="m-0 px-sm py-xs text-2xs text-faint">Danh sách chưa có dự án nào.</p>}
      >
        <ul class="m-0 flex list-none flex-col gap-3xs p-0">
          {/* Keyed theo `id`: mở một dự án khác làm cả danh sách xếp lại theo `lastOpenedAt`,
              và keyed theo vị trí thì mọi hàng bị dựng lại giữa lúc tiêu điểm đang ở trên
              một trong số chúng. */}
          <Key each={visible()} by="id">
            {(project) => (
              <li>
                <div
                  class="group/row relative flex items-center"
                  onContextMenu={(event) => {
                    event.preventDefault();
                    props.onMenuChange(project().id);
                  }}
                >
                  <button
                    type="button"
                    disabled={props.switching}
                    onClick={() => {
                      if (!project().isCurrent) props.onPick(project().id);
                    }}
                    aria-current={project().isCurrent ? "true" : undefined}
                    // Đường dẫn không còn một dòng riêng trên mỗi hàng — cột 260px không đủ
                    // cho hai dòng nhân số dự án. Nó vào `title` và vào nhãn trợ năng, nên
                    // hai dự án trùng tên vẫn phân biệt được, chỉ là phải rê chuột vào.
                    title={project().path}
                    aria-label={
                      project().isCurrent
                        ? `${project().name} — dự án đang mở. ${project().path}`
                        : `Mở dự án ${project().name}. ${project().path}`
                    }
                    class="flex min-w-0 flex-1 items-center gap-sm rounded-panel py-2xs pr-(--sp-2xl) pl-sm text-left text-sm transition-colors duration-[var(--dur-fast)] disabled:cursor-progress enabled:hover:bg-[var(--overlay-hover)] aria-[current]:bg-accent-soft aria-[current]:font-medium"
                  >
                    <span
                      class="shrink-0"
                      classList={{
                        "text-accent-ink": project().isCurrent,
                        "text-muted": !project().isCurrent,
                        "motion-safe:animate-pulse": props.switching && project().isCurrent,
                      }}
                    >
                      <Icon name={project().isCurrent ? "folder-open" : "folder"} size={15} />
                    </span>
                    <span
                      class="min-w-0 flex-1 truncate"
                      classList={{
                        "text-accent-ink": project().isCurrent,
                        "text-text": !project().isCurrent,
                      }}
                    >
                      {project().name}
                    </span>
                  </button>

                  <div
                    class="absolute right-3xs transition-opacity duration-[var(--dur-fast)] group-focus-within/row:opacity-100 group-hover/row:opacity-100"
                    classList={{
                      "opacity-0": props.menuFor !== project().id,
                      "opacity-100": props.menuFor === project().id,
                    }}
                  >
                    <Menu
                      label={`Tuỳ chọn cho dự án ${project().name}`}
                      open={props.menuFor === project().id}
                      onOpenChange={(open) => props.onMenuChange(open ? project().id : null)}
                      onRequestClose={() => props.onMenuChange(null)}
                      items={itemsFor(project())}
                    />
                  </div>
                </div>

                {/* Mục con của dự án đang mở, thụt vào và có một đường kẻ dọc: thụt lề một
                    mình đọc ra là "hàng bị lệch", còn đường kẻ nói rõ chúng treo vào hàng
                    ngay trên. */}
                <Show when={project().isCurrent && props.subItems.length > 0}>
                  <ul class="m-0 mt-3xs ml-(--sp-lg) flex list-none flex-col gap-3xs border-l border-line py-3xs pl-2xs">
                    <For each={props.subItems}>
                      {(item) => (
                        <li>
                          <button
                            type="button"
                            onClick={item.onSelect}
                            disabled={props.switching}
                            aria-current={item.active ? "page" : undefined}
                            class="flex w-full items-center gap-2xs rounded-panel px-2xs py-3xs text-left text-xs text-muted transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed enabled:hover:bg-[var(--overlay-hover)] enabled:hover:text-ink aria-[current=page]:bg-accent-soft aria-[current=page]:font-medium aria-[current=page]:text-accent-ink"
                          >
                            <span class="shrink-0">
                              <Icon name={item.icon} size={14} />
                            </span>
                            <span class="min-w-0 flex-1 truncate">{item.label}</span>
                            <Show when={(item.badge ?? 0) > 0}>
                              <span class="shrink-0 rounded-pill bg-accent px-2xs text-[10px] leading-4 text-on-accent tabular-nums">
                                {item.badge}
                              </span>
                            </Show>
                          </button>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </li>
            )}
          </Key>
        </ul>
      </Show>

      {/* Danh sách dài thì cắt bớt chứ không cuộn: một danh sách dự án dài đẩy "Gần đây"
          xuống dưới nếp gấp, mà "Gần đây" mới là thứ được bấm mỗi ngày. */}
      <Show when={hidden() > 0 || expanded()}>
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded()}
          class="flex w-full items-center gap-2xs rounded-panel px-sm py-3xs text-left text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
        >
          <Icon
            name="chevron-right"
            size={12}
            class={`transition-transform duration-[var(--dur-fast)] ${expanded() ? "rotate-90" : ""}`}
          />
          {expanded() ? "Thu gọn" : `Xem thêm ${hidden()} dự án`}
        </button>
      </Show>

      {/* Một lối ra duy nhất, và nó dẫn tới màn hình dự án chứ không tới một hộp thoại thứ
          hai: tạo mới, clone và lọc theo loại đều đã sống ở đó. */}
      <button
        type="button"
        onClick={props.onSeeAll}
        disabled={props.switching}
        class="flex w-full items-center gap-sm rounded-panel px-sm py-2xs text-left text-xs text-muted transition-colors duration-[var(--dur-fast)] disabled:cursor-progress enabled:hover:bg-[var(--overlay-hover)] enabled:hover:text-ink"
      >
        <span class="shrink-0">
          <Icon name="more" size={15} />
        </span>
        Tất cả dự án…
      </button>

      {/* Không dự án là một **trạng thái hợp lệ**, không phải một lần nạp chưa xong — nên
          nó phải tự nói ra, kể cả khi danh sách bên trên đã đủ ba dòng. Câu này gộp hai
          điều người dùng cần biết ngay: trợ lý vẫn trả lời được, và hai màn hình vắng mặt
          sẽ quay lại khi nào. */}
      <Show when={props.current === null && !props.switching}>
        <p class="m-0 flex items-start gap-2xs rounded-panel bg-[var(--overlay-faint)] px-sm py-xs text-2xs leading-[1.5] text-muted">
          <span class="mt-3xs shrink-0 text-faint">
            <Icon name="chat" size={13} />
          </span>
          <span>
            Chưa mở dự án — trợ lý chỉ trò chuyện, không đọc tệp. Mở một dự án để có Thay đổi
            và Thư viện tài liệu.
          </span>
        </p>
      </Show>

      <Show when={props.switching}>
        <p class="m-0 px-sm py-3xs text-2xs text-faint" role="status" aria-live="polite">
          đang chuyển dự án…
        </p>
      </Show>
    </div>
  );
}
