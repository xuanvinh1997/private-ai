import { Show, type JSX } from "solid-js";
import { originHost } from "../../lib/projects";
import { relativeTime } from "../../lib/sessions";
import type { Project, ProjectKind } from "../../lib/protocol";
import Icon, { type IconName } from "../Icon";
import { IconButton } from "../primitives";

/**
 * Chi tiết dự án đang mở — cột phải.
 *
 * Trước đây những thứ này không có chỗ nào cả. Loại dự án chỉ đọc ra được *gián tiếp*, qua
 * việc thanh bên mọc thêm một hàng tên là "Thay đổi" hay "Thư viện tài liệu" — một màn hình
 * đóng vai một thuộc tính. Đường dẫn nằm trong `title`, phải rê chuột mới thấy. Nguồn gốc
 * clone và lần mở gần nhất thì chỉ có ở màn hình Dự án, tức là phải rời chỗ đang làm.
 *
 * Nên bảng này không phải "một nút nữa cho thư viện tài liệu": nó là **chỗ trả lời câu hỏi
 * *dự án này là cái gì***, và những màn hình của nó chỉ là một mục trong câu trả lời đó,
 * đứng cạnh đường dẫn và nguồn gốc chứ không lẫn vào cây điều hướng.
 *
 * Cùng vỏ với bảng "Tệp đã thay đổi": cùng bề rộng, cùng viền trái, cùng hàng tiêu đề có
 * nút đóng. Hai bảng cùng chiếm một chỗ thì phải trông như hai mặt của một ngăn, chứ không
 * như hai thứ tình cờ rơi vào cùng một cạnh màn hình.
 */
export default function ProjectPanel(props: {
  project: Project;
  /** Số tệp phiên này đã đụng. Chỉ có nghĩa với dự án mã nguồn. */
  changedCount: number;
  onClose: () => void;
  /** Mở màn hình của dự án: Thay đổi hoặc Thư viện tài liệu. */
  onOpenScreen: () => void;
  onOpenFolder: () => void;
  onSwapKind: (kind: ProjectKind) => void;
  onCloseProject: () => void;
}) {
  const docs = () => props.project.kind === "docs";

  return (
    <aside
      aria-label={`Chi tiết dự án ${props.project.name}`}
      class="flex w-(--changes-col-w) shrink-0 flex-col border-l border-line bg-sidebar"
    >
      <header class="flex h-(--header-h) shrink-0 items-center gap-sm border-b border-line px-md">
        <h2 class="m-0 flex-1 text-xs font-semibold text-ink">Dự án</h2>
        <IconButton icon="x" label="Đóng bảng chi tiết dự án" size="sm" onClick={props.onClose} />
      </header>

      <div class="min-h-0 flex-1 overflow-y-auto">
        {/* Tên + loại, cùng cặp biểu tượng với thanh bên và màn hình Dự án. Loại đứng
            thành chữ ở đây chứ không chỉ là biểu tượng: đây là chỗ *giải thích*, và một
            bảng chi tiết mà vẫn bắt đoán ký hiệu thì không chi tiết hơn cái nó thay. */}
        <div class="flex items-start gap-sm border-b border-line px-md py-md">
          <span class="grid size-8 shrink-0 place-items-center rounded-panel bg-accent text-on-accent">
            <Icon name={docs() ? "library" : "code"} size={15} />
          </span>
          <div class="flex min-w-0 flex-1 flex-col gap-3xs">
            <span class="truncate text-sm font-medium text-ink">{props.project.name}</span>
            <span class="text-2xs text-muted">
              {docs() ? "Thư viện tài liệu" : "Dự án mã nguồn"} · mở{" "}
              {relativeTime(props.project.lastOpenedAt)}
            </span>
          </div>
        </div>

        {/* Đường dẫn: `break-all`, không `truncate`. Hai bản sao của cùng một repo chỉ khác
            nhau ở *đoạn giữa* đường dẫn, mà đó đúng là đoạn một dòng cắt cụt ăn mất. */}
        <Field icon="folder" label="Thư mục" action={{ label: "Mở trong trình quản lý tệp", icon: "external", onClick: props.onOpenFolder }}>
          <span class="font-mono text-2xs break-all text-muted">{props.project.path}</span>
        </Field>

        <Show when={props.project.origin}>
          {(origin) => (
            <Field icon="git-branch" label="Clone từ">
              <span class="font-mono text-2xs break-all text-muted" title={origin()}>
                {originHost(origin())}
              </span>
            </Field>
          )}
        </Show>

        {/* Màn hình của dự án. Đúng **một** mục, vì một dự án chỉ thuộc một loại — nó ở
            đây, cạnh đường dẫn và nguồn gốc, thay vì thụt vào dưới tên dự án trong cây
            điều hướng, nơi nó đọc ra như thể loại dự án là một chỗ để bấm. */}
        <div class="flex flex-col gap-2xs px-md py-md">
          <span class="text-[10px] font-medium tracking-wide text-faint uppercase">
            Màn hình của dự án
          </span>
          <button
            type="button"
            onClick={props.onOpenScreen}
            class="flex items-center gap-sm rounded-panel px-2xs py-xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
          >
            <span class="shrink-0 text-muted">
              <Icon name={docs() ? "library" : "diff"} size={15} />
            </span>
            <span class="flex min-w-0 flex-1 flex-col">
              <span class="truncate text-xs text-text">
                {docs() ? "Thư viện tài liệu" : "Thay đổi"}
              </span>
              <span class="truncate text-2xs text-faint">
                {docs() ? "Tìm trong tài liệu đã nạp." : "Tệp phiên này đã đụng vào."}
              </span>
            </span>
            <Show when={!docs() && props.changedCount > 0}>
              <span class="shrink-0 rounded-pill bg-accent px-2xs text-[10px] leading-4 text-on-accent tabular-nums">
                {props.changedCount}
              </span>
            </Show>
          </button>
        </div>

        {/* Ba việc đổi trạng thái dự án, gom cuối bảng và xếp theo mức phá huỷ tăng dần.
            Chúng vẫn còn trong menu chuột phải ở thanh bên; ở đây chúng có chỗ để nói ra
            hậu quả bằng một dòng, thứ mà một menu bật lên không phải lúc nào cũng đọc kịp. */}
        <div class="flex flex-col gap-2xs border-t border-line px-md py-md">
          <span class="text-[10px] font-medium tracking-wide text-faint uppercase">Thay đổi dự án</span>

          <Action
            icon={docs() ? "code" : "library"}
            label={docs() ? "Chuyển thành dự án mã nguồn" : "Chuyển thành thư viện tài liệu"}
            hint={docs() ? "Trợ lý đọc, sửa tệp và chạy được lệnh." : "Thôi sửa tệp và chạy lệnh, chỉ đọc tài liệu."}
            onClick={() => props.onSwapKind(docs() ? "code" : "docs")}
          />
          <Action
            icon="folder"
            label="Đóng dự án, chỉ trò chuyện"
            hint="Vẫn ở trong danh sách; trợ lý thôi đọc tệp."
            onClick={props.onCloseProject}
          />
          {/* Có mặt mà khoá, đúng như menu chuột phải ở thanh bên: bảng này chỉ vẽ dự án
              **đang mở**, và bỏ nó khỏi danh sách là bỏ chỗ đứng của chính mình. Giấu hàng
              đi thì người đi tìm việc ấy kết luận là ứng dụng không làm được. */}
          <Action
            icon="x"
            label="Bỏ khỏi danh sách"
            hint="Đang mở — đóng dự án trước đã."
            disabled
          />
        </div>
      </div>
    </aside>
  );
}

/** Một mục dữ liệu: nhãn nhỏ, nội dung, và một hành động tuỳ chọn ở cột phải. */
function Field(props: {
  icon: IconName;
  label: string;
  action?: { label: string; icon: IconName; onClick: () => void };
  children: JSX.Element;
}) {
  return (
    <div class="flex items-start gap-sm border-b border-line px-md py-sm">
      <span class="mt-3xs shrink-0 text-faint">
        <Icon name={props.icon} size={13} />
      </span>
      <div class="flex min-w-0 flex-1 flex-col gap-3xs">
        <span class="text-[10px] font-medium tracking-wide text-faint uppercase">{props.label}</span>
        {props.children}
      </div>
      <Show when={props.action}>
        {(action) => (
          <IconButton
            icon={action().icon}
            label={action().label}
            size="sm"
            onClick={action().onClick}
          />
        )}
      </Show>
    </div>
  );
}

/** Một việc đổi trạng thái dự án: nhãn, và một dòng nói hậu quả. */
function Action(props: {
  icon: IconName;
  label: string;
  hint: string;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      disabled={props.disabled}
      onClick={props.onClick}
      class="flex items-start gap-sm rounded-panel px-2xs py-xs text-left transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-50 enabled:hover:bg-[var(--overlay-hover)]"
    >
      <span class="mt-3xs shrink-0 text-muted">
        <Icon name={props.icon} size={14} />
      </span>
      <span class="flex min-w-0 flex-col gap-3xs">
        <span class="text-xs text-text">{props.label}</span>
        <span class="text-2xs text-faint">{props.hint}</span>
      </span>
    </button>
  );
}
