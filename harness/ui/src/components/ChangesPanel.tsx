import { createSignal, For, Show } from "solid-js";
import { baseName, dirName, type ChangedFile } from "../lib/changes";
import DiffBlock from "./DiffBlock";
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
  onClose: () => void;
}) {
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
        <Totals files={props.files} class="border-b border-line px-md py-xs" />

        <ul class="m-0 min-h-0 flex-1 list-none overflow-y-auto p-sm">
          <For each={props.files}>
            {(file) => (
              <li>
                <button
                  type="button"
                  onClick={() => props.onReveal(file.nodeId)}
                  class="flex w-full items-center gap-sm rounded-panel px-sm py-xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
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
                  <Counts added={file.added} removed={file.removed} />
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </aside>
  );
}

/**
 * Màn hình thay đổi ở dạng trang đầy — bố cục review của Codex.
 *
 * Khác cột phải ở đúng một chỗ, và chỗ đó là toàn bộ lý do nó tồn tại: bấm vào một hàng
 * **mở diff ngay tại đó** thay vì ném người đọc ngược về bản ghi. Cột phải là mục lục để
 * liếc trong lúc đang chat; trang này là chỗ ngồi xuống đọc lại toàn bộ những gì trợ lý
 * vừa làm, và một mục lục không mở ra được thì không đọc lại được cái gì.
 *
 * Mọi hàng mở sẵn ở lần đầu: sau một lượt sửa mã, câu hỏi đầu tiên luôn là "nó đã làm gì",
 * và bắt người dùng bấm mở từng tệp để trả lời câu đó là bắt họ trả tiền cho một cú gập
 * mà chưa ai xin.
 */
export function ChangesBoard(props: {
  files: ChangedFile[];
  onReveal: (nodeId: string) => void;
}) {
  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-(--reading-measure) flex-col gap-md">
        <Show
          when={props.files.length > 0}
          fallback={<p class="m-0 text-sm text-faint">Phiên này chưa đụng vào tệp nào.</p>}
        >
          <Totals files={props.files} class="px-3xs" />
          <For each={props.files}>{(file) => <FileReview file={file} onReveal={props.onReveal} />}</For>
        </Show>
      </div>
    </div>
  );
}

function FileReview(props: { file: ChangedFile; onReveal: (nodeId: string) => void }) {
  const [open, setOpen] = createSignal(true);
  return (
    <section class="overflow-hidden rounded-card border border-line bg-surface">
      <div class="flex items-center gap-sm px-(--card-pad-x) py-(--card-pad-y)">
        {/* Cả dải tên tệp là công tắc gập, đúng như review pane của Codex: đích bấm lớn
            nhất trong hàng nên là việc làm nhiều nhất, chứ không phải một mũi tên 12px. */}
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open()}
          class="flex min-w-0 flex-1 items-center gap-sm text-left"
        >
          <span
            class="shrink-0 text-muted transition-transform duration-[var(--dur-fast)]"
            classList={{ "rotate-90": open() }}
          >
            <Icon name="chevron-right" size={14} />
          </span>
          <span
            class="min-w-0 flex-1 truncate font-mono text-xs text-text"
            dir="rtl"
            title={props.file.path}
          >
            <bdi>{props.file.path}</bdi>
          </span>
          <Show when={props.file.created}>
            <span class="shrink-0 text-2xs text-success">tệp mới</span>
          </Show>
          <Show when={props.file.pending}>
            <span class="shrink-0 text-2xs text-warn">dự kiến</span>
          </Show>
          <Counts added={props.file.added} removed={props.file.removed} />
        </button>

        {/* Lối về bản ghi vẫn còn, chỉ là không còn là hành động chính nữa: nó trả lời một
            câu khác — "trợ lý nói gì lúc nó sửa chỗ này". */}
        <IconButton
          icon="chat"
          size="sm"
          label={`Xem lúc trợ lý sửa ${baseName(props.file.path)} trong bản ghi`}
          onClick={() => props.onReveal(props.file.nodeId)}
        />
      </div>

      <Show when={open() && props.file.hunks.length > 0}>
        <div class="border-t border-line p-(--card-pad-y)">
          {/* Hạn dòng cao hơn hẳn trong chat: ở đây diff *là* nội dung, không phải một
              trích đoạn chen giữa hai câu trả lời. */}
          <DiffBlock diffs={props.file.hunks} maxLines={40} />
        </div>
      </Show>
    </section>
  );
}

/** Dòng tổng: bao nhiêu tệp, cộng bao nhiêu, trừ bao nhiêu. */
function Totals(props: { files: ChangedFile[]; class?: string }) {
  const added = () => props.files.reduce((sum, file) => sum + file.added, 0);
  const removed = () => props.files.reduce((sum, file) => sum + file.removed, 0);
  return (
    <div class={`flex items-center gap-sm text-2xs tabular-nums ${props.class ?? ""}`}>
      <span class="text-muted">{props.files.length} tệp</span>
      <span class="text-success">+{added()}</span>
      <span class="text-danger">−{removed()}</span>
    </div>
  );
}

function Counts(props: { added: number; removed: number }) {
  return (
    <span class="shrink-0 text-2xs tabular-nums">
      <span class="text-success">+{props.added}</span>{" "}
      <span class="text-danger">−{props.removed}</span>
    </span>
  );
}
