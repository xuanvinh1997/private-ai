import { For, Show } from "solid-js";
import { baseName, dirName, type ChangedFile } from "../lib/changes";
import Icon from "./Icon";
import { IconButton } from "./primitives";

/**
 * Bảng "tệp đã đổi trong phiên" — thứ một coding agent cần mà một chatbot thì không.
 *
 * Một lượt sửa mã sinh ra hàng chục khối trong bản ghi, và tệp bị đụng ba lần thì nằm ở
 * ba chỗ cách xa nhau. Bảng này gấp tất cả lại thành *một hàng cho mỗi tệp*, và bấm vào
 * hàng đó thì cuộn tới lần đụng gần nhất — nghĩa là nó không phải một khung nhìn thứ hai
 * của cùng dữ liệu, mà là mục lục của khung nhìn thứ nhất.
 */
export default function ChangesPanel(props: {
  files: ChangedFile[];
  onReveal: (nodeId: string) => void;
  /**
   * Mở tệp trong tab Mã nguồn.
   *
   * Đứng cạnh `onReveal` chứ không thay nó, vì hai cú bấm trả lời hai câu hỏi khác nhau:
   * "trợ lý đã đổi cái gì ở đây" (bản ghi) và "tệp bây giờ đang ra sao" (đĩa). Gộp lại
   * thì mất một câu, và câu mất đi phụ thuộc vào việc ai gộp.
   */
  onOpenFile: ((path: string) => void) | null;
  onClose: () => void;
}) {
  const added = () => props.files.reduce((sum, file) => sum + file.added, 0);
  const removed = () => props.files.reduce((sum, file) => sum + file.removed, 0);

  return (
    <aside
      aria-label="Tệp đã thay đổi"
      class="flex w-(--changes-col-w) shrink-0 flex-col border-l border-line bg-sidebar"
    >
      <header class="flex h-(--header-h) shrink-0 items-center gap-sm border-b border-line px-md">
        <h2 class="m-0 flex-1 text-xs font-semibold text-ink">Tệp đã thay đổi</h2>
        <IconButton icon="x" label="Đóng bảng thay đổi" size="sm" onClick={props.onClose} />
      </header>

      <Show
        when={props.files.length > 0}
        fallback={
          <p class="px-md py-lg text-xs text-faint">
            Chưa có tệp nào bị đụng vào trong phiên này.
          </p>
        }
      >
        <div class="flex items-center gap-sm border-b border-line px-md py-xs text-2xs tabular-nums">
          <span class="text-muted">{props.files.length} tệp</span>
          <span class="text-success">+{added()}</span>
          <span class="text-danger">−{removed()}</span>
        </div>

        <ul class="m-0 min-h-0 flex-1 list-none overflow-y-auto p-sm">
          <For each={props.files}>
            {(file) => (
              <li class="group relative">
                <button
                  type="button"
                  onClick={() => props.onReveal(file.nodeId)}
                  class="flex w-full items-center gap-sm rounded-panel px-sm py-xs pr-(--sp-3xl) text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
                >
                  <span
                    class="shrink-0"
                    classList={{ "text-warn": file.pending, "text-muted": !file.pending }}
                  >
                    <Icon name="diff" size={15} />
                  </span>
                  <span class="flex min-w-0 flex-1 flex-col">
                    <span class="flex min-w-0 items-baseline gap-2xs">
                      <span class="min-w-0 truncate font-mono text-xs text-text">
                        {baseName(file.path)}
                      </span>
                      <Show when={file.created}>
                        <span class="shrink-0 text-2xs text-success">tệp mới</span>
                      </Show>
                      <Show when={file.pending}>
                        <span class="shrink-0 text-2xs text-warn">dự kiến</span>
                      </Show>
                    </span>
                    <Show when={dirName(file.path)}>
                      {(dir) => (
                        <span class="min-w-0 truncate text-2xs text-faint" dir="rtl" title={file.path}>
                          <bdi>{dir()}</bdi>
                        </span>
                      )}
                    </Show>
                  </span>
                  <span class="shrink-0 text-2xs tabular-nums">
                    <span class="text-success">+{file.added}</span>{" "}
                    <span class="text-danger">−{file.removed}</span>
                  </span>
                </button>

                <Show when={props.onOpenFile}>
                  {(open) => (
                    <span class="absolute top-1/2 right-2xs -translate-y-1/2 opacity-0 transition-opacity duration-[var(--dur-fast)] group-focus-within:opacity-100 group-hover:opacity-100">
                      <IconButton
                        icon="code"
                        label={`Mở ${baseName(file.path)} trong Mã nguồn`}
                        size="sm"
                        onClick={() => open()(file.path)}
                      />
                    </span>
                  )}
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </aside>
  );
}
