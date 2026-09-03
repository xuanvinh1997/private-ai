import { createResource, createSignal, For, Show, Suspense } from "solid-js";
import { listDir, originHost } from "../../lib/projects";
import { relativeTime } from "../../lib/sessions";
import type { DirEntry, Project } from "../../lib/protocol";
import Icon from "../Icon";
import { IconButton } from "../primitives";

/**
 * Cây thư mục của dự án đang mở — cột phải.
 *
 * Bản trước của bảng này liệt kê *thuộc tính* của dự án và ba việc đổi trạng thái nó
 * (đổi loại, đóng, bỏ khỏi danh sách). Đó là một bảng trả lời câu hỏi người dùng hỏi
 * **một lần** rồi thôi, đặt ở chỗ họ nhìn suốt buổi. Cái họ nhìn suốt buổi là *dự án này
 * có những tệp gì*, và ba việc kia thì vẫn nằm nguyên trong menu chuột phải ở thanh bên —
 * đúng chỗ của những việc hiếm mà nặng.
 *
 * Cây đọc **theo từng tầng**: bung một nhánh là một lời gọi, nhánh chưa bung thì chưa tốn
 * gì. Nhờ vậy `.git` và `node_modules` không phải bị giấu đi để bảng còn chạy được — chúng
 * cứ nằm đó, và chỉ tốn gì khi có người cố ý mở.
 *
 * Bấm một **tệp** thì tên nó rơi vào ô soạn tin dưới dạng `@đường/dẫn`. Ứng dụng không có
 * màn hình đọc tệp, nên một cú bấm mở ra một khung xem là một lời hứa suông; còn `@` thì
 * đúng là thứ tệp ấy dùng để làm — đưa nó cho trợ lý.
 */
export default function ProjectPanel(props: {
  project: Project;
  onClose: () => void;
  onOpenFolder: () => void;
  /** Đưa một tệp vào ô soạn tin. Nhận đường dẫn tuyệt đối; nơi gọi tự rút gọn. */
  onPickFile: (path: string) => void;
  /** Mở màn hình riêng của dự án: Thay đổi với mã nguồn, Thư viện với tài liệu. */
  onOpenScreen: () => void;
}) {
  const docs = () => props.project.kind === "docs";

  return (
    <aside
      aria-label={`Tệp trong dự án ${props.project.name}`}
      class="flex w-(--changes-col-w) shrink-0 flex-col border-l border-line bg-sidebar"
    >
      <header class="flex h-(--header-h) shrink-0 items-center gap-sm border-b border-line px-md">
        <h2 class="m-0 flex-1 text-xs font-semibold text-ink">Tệp trong dự án</h2>
        {/* Màn hình riêng của dự án còn đúng **một biểu tượng** ở đây.
            Trước nó là một hàng thụt vào dưới tên dự án trong thanh bên, và ở đó nó đọc ra
            như thể loại dự án là một chỗ để bấm; rồi nó là một mục trong bảng này, chiếm
            chỗ của thứ người dùng nhìn suốt buổi. Nhưng bỏ hẳn thì hai màn hình ấy không
            còn đường nào tới ngoài phím tắt — nên nó ở lại, dưới dạng nhỏ nhất còn dùng
            được. */}
        <IconButton
          icon={docs() ? "library" : "diff"}
          label={docs() ? "Mở Thư viện tài liệu" : "Mở màn hình Thay đổi"}
          size="sm"
          onClick={props.onOpenScreen}
        />
        <IconButton
          icon="external"
          label="Mở thư mục trong trình quản lý tệp"
          size="sm"
          onClick={props.onOpenFolder}
        />
        <IconButton icon="x" label="Đóng bảng tệp" size="sm" onClick={props.onClose} />
      </header>

      {/* Tên và loại dự án: đủ để biết cây bên dưới là cây của ai, không hơn. Đường dẫn
          đầy đủ nằm trong `title` — nó dài hơn cả cột, và ở đây nó là thứ để đối chiếu chứ
          không phải thứ để đọc. */}
      <div
        class="flex shrink-0 items-center gap-sm border-b border-line px-md py-sm"
        title={props.project.path}
      >
        <span class="grid size-7 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
          <Icon name={docs() ? "library" : "code"} size={14} />
        </span>
        <div class="flex min-w-0 flex-1 flex-col gap-3xs">
          <span class="truncate text-xs font-medium text-ink">{props.project.name}</span>
          <span class="truncate text-2xs text-faint">
            {docs() ? "Thư viện tài liệu" : "Dự án mã nguồn"} · mở{" "}
            {relativeTime(props.project.lastOpenedAt)}
            <Show when={props.project.origin}>
              {(origin) => <> · {originHost(origin())}</>}
            </Show>
          </span>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto p-2xs">
        {/* `keyed` theo đường dẫn: đổi dự án là đổi cả cây, và giữ lại trạng thái bung của
            cây cũ thì các nhánh đang mở trỏ vào những thư mục không còn tồn tại. */}
        <Show when={props.project.path} keyed>
          {(root) => <Branch path={root} depth={0} onPickFile={props.onPickFile} />}
        </Show>
      </div>
    </aside>
  );
}

/**
 * Một tầng của cây.
 *
 * Tự đọc lấy nội dung của mình thay vì nhận từ cha: mỗi nhánh là một lần gọi mạng riêng,
 * và một `createResource` cho mỗi nhánh giữ đúng cái ranh giới ấy — nhánh nào đang chờ thì
 * nhánh ấy hiện dòng chờ, không phải cả cây.
 */
function Branch(props: { path: string; depth: number; onPickFile: (path: string) => void }) {
  const [entries] = createResource(() => props.path, listDir);

  return (
    <Suspense fallback={<Line depth={props.depth}>đang đọc…</Line>}>
      <Show
        when={(entries() ?? []).length > 0}
        fallback={<Line depth={props.depth}>thư mục rỗng</Line>}
      >
        <ul class="m-0 flex list-none flex-col p-0">
          <For each={entries()}>
            {(entry) => <Node entry={entry} depth={props.depth} onPickFile={props.onPickFile} />}
          </For>
        </ul>
      </Show>
    </Suspense>
  );
}

/** Một hàng: thư mục bung ra được, tệp thì đi vào ô soạn tin. */
function Node(props: { entry: DirEntry; depth: number; onPickFile: (path: string) => void }) {
  const [open, setOpen] = createSignal(false);

  return (
    <li>
      <button
        type="button"
        onClick={() =>
          props.entry.isDir ? setOpen((v) => !v) : props.onPickFile(props.entry.path)
        }
        title={props.entry.isDir ? props.entry.name : `Đưa ${props.entry.name} vào ô soạn tin`}
        aria-expanded={props.entry.isDir ? open() : undefined}
        // Thụt lề theo chiều sâu, cộng một khoảng chừa sẵn cho mũi tên. Tệp không có mũi
        // tên nhưng vẫn chừa chỗ: không chừa thì tên tệp và tên thư mục lệch nhau một
        // nhịp, và mắt đọc ra là hai danh sách chứ không phải một cây.
        style={{ "padding-left": `${props.depth * 12 + 4}px` }}
        class="flex w-full items-center gap-2xs rounded-panel py-3xs pr-2xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
      >
        <span class="w-3 shrink-0 text-faint">
          <Show when={props.entry.isDir}>
            <Icon
              name="chevron-right"
              size={11}
              class={`transition-transform duration-[var(--dur-fast)] ${open() ? "rotate-90" : ""}`}
            />
          </Show>
        </span>
        <span class="shrink-0 text-muted">
          <Icon name={props.entry.isDir ? "folder" : "document"} size={13} />
        </span>
        <span class="min-w-0 flex-1 truncate text-2xs text-text">{props.entry.name}</span>
      </button>

      {/* Nhánh con chỉ được dựng **sau** cú bấm đầu tiên, và bị tháo đi khi thu lại: giữ
          nó trong DOM thì một cây đã mở sâu vẫn còn nguyên trong bộ nhớ sau khi người dùng
          gấp nó lại, kèm cả dữ liệu đã cũ so với đĩa. */}
      <Show when={props.entry.isDir && open()}>
        <Branch path={props.entry.path} depth={props.depth + 1} onPickFile={props.onPickFile} />
      </Show>
    </li>
  );
}

/** Dòng trạng thái của một nhánh — đang đọc, hoặc rỗng. Thụt đúng bằng hàng của nhánh. */
function Line(props: { depth: number; children: string }) {
  return (
    <p
      class="m-0 py-3xs text-2xs text-faint"
      style={{ "padding-left": `${props.depth * 12 + 24}px` }}
    >
      {props.children}
    </p>
  );
}
