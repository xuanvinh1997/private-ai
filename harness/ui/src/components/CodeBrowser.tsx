import { createResource, Show } from "solid-js";
import type { FileView, TreeEntry } from "../lib/protocol";
import CodeViewer from "./CodeViewer";
import FileTree from "./FileTree";
import Icon from "./Icon";
import { IconButton } from "./primitives";

/**
 * Tab "Mã nguồn": cây tệp bên trái, khung xem bên phải.
 *
 * Cả hai nguồn dữ liệu đi vào bằng prop chứ không tự gọi `invoke`. Đó là điều kiện để
 * trang demo chạy được mà không có lõi nào: chỗ duy nhất biết "thật hay demo" là `App`,
 * và không đường nào từ đây gọi thẳng vào `lib/demo.ts`.
 */
export default function CodeBrowser(props: {
  /** Đổi giá trị là đổi dự án — cây và khung xem cùng vứt trạng thái cũ. */
  projectId: string;
  /** Tên dự án đang mở — tab này không có ô chọn dự án, nên nó phải tự nói mình ở đâu. */
  projectName: string;
  /** Gốc dự án. Chỉ dùng để cắt tiền tố lúc hiện — đường dẫn gửi cho lõi vẫn tuyệt đối. */
  root: string | null;
  loadTree: (path?: string) => Promise<TreeEntry[]>;
  loadFile: (path: string) => Promise<FileView>;
  open: { path: string; line?: number } | null;
  onOpen: (path: string, line?: number) => void;
  onFind: () => void;
}) {
  const [file] = createResource(
    () => props.open?.path ?? null,
    (path) => props.loadFile(path),
  );

  return (
    <div class="flex min-h-0 flex-1">
      <aside
        aria-label="Cây tệp"
        class="flex w-(--tree-col-w) shrink-0 flex-col border-r border-line bg-sidebar"
      >
        <header class="flex h-(--header-h) shrink-0 items-center gap-sm border-b border-line px-md">
          <span class="shrink-0 text-accent">
            <Icon name="folder-open" size={15} />
          </span>
          <h2 class="m-0 min-w-0 flex-1 truncate text-xs font-semibold text-ink" title={props.projectName}>
            {props.projectName}
          </h2>
          <IconButton
            icon="search"
            label="Tìm tệp theo tên"
            keys="Meta+P Control+P"
            size="sm"
            onClick={props.onFind}
          />
        </header>
        <FileTree
          load={props.loadTree}
          resetKey={props.projectId}
          selected={props.open?.path ?? null}
          onOpen={(path) => props.onOpen(path)}
        />
      </aside>

      <div class="flex min-w-0 flex-1 flex-col">
        <Show when={props.open} fallback={<NoFile onFind={props.onFind} />}>
          {(open) => (
            <Show
              when={file()}
              fallback={
                <Show
                  when={file.error}
                  fallback={
                    <p class="m-auto text-sm text-muted" role="status" aria-live="polite">
                      Đang đọc tệp…
                    </p>
                  }
                >
                  <p
                    class="m-auto max-w-(--reading-measure) rounded-panel bg-danger-soft px-md py-sm text-sm text-danger"
                    role="alert"
                  >
                    Không đọc được {open().path}: {String(file.error)}
                  </p>
                </Show>
              }
            >
              {(view) => (
                <CodeViewer
                  path={open().path}
                  root={props.root}
                  file={view()}
                  {...(open().line === undefined ? {} : { line: open().line })}
                />
              )}
            </Show>
          )}
        </Show>
      </div>
    </div>
  );
}

function NoFile(props: { onFind: () => void }) {
  return (
    <div class="grid min-h-0 flex-1 place-items-center px-(--page-pad-x)">
      <div class="flex max-w-[44ch] flex-col items-center gap-sm text-center">
        <span class="grid size-10 place-items-center rounded-panel bg-surface-hover text-muted">
          <Icon name="code" size={20} />
        </span>
        <h2 class="m-0 text-md font-semibold text-ink">Chưa mở tệp nào</h2>
        <p class="m-0 text-sm text-muted">
          Chọn một tệp trong cây bên trái, hoặc bấm vào đường dẫn trong một thẻ công cụ để
          mở đúng chỗ trợ lý vừa đụng tới.
        </p>
        <button
          type="button"
          onClick={props.onFind}
          class="flex h-(--control-h) items-center gap-2xs rounded-btn border border-line px-md text-xs text-text transition-colors duration-[var(--dur-fast)] hover:border-accent"
        >
          <Icon name="search" size={13} />
          Tìm tệp theo tên
          <kbd class="ml-2xs rounded-btn bg-[var(--overlay-faint)] px-3xs font-mono text-2xs text-faint">
            ⌘P
          </kbd>
        </button>
      </div>
    </div>
  );
}
