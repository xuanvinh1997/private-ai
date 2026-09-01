import { createEffect, createMemo, For, on, Show } from "solid-js";
import { baseName, dirName } from "../lib/changes";
import { highlight, langFromExtension, langFromPath, type TokenKind } from "../lib/code";
import { displayPath } from "../lib/projects";
import type { FileView } from "../lib/protocol";
import Icon from "./Icon";
import { Chip, CopyButton } from "./primitives";

/**
 * Bao nhiêu dòng vẽ ra DOM.
 *
 * Lõi đã cắt tệp trước khi gửi (`FileView.truncated`), nhưng ngưỡng của lõi là ngưỡng
 * *truyền dữ liệu*, còn đây là ngưỡng *dựng DOM* — vài chục nghìn phần tử làm trình
 * duyệt đứng hình bất kể chúng đến từ đâu. Hai ngưỡng cho hai chi phí khác nhau.
 */
const MAX_ROWS = 4000;

const TOKEN_COLOR: Record<TokenKind, string> = {
  plain: "var(--text)",
  comment: "var(--code-comment)",
  string: "var(--code-string)",
  number: "var(--code-number)",
  keyword: "var(--code-keyword)",
};

/**
 * Khung xem một tệp: số dòng bên trái, mã nguồn cuộn ngang bên phải.
 *
 * Cột số dòng `sticky left-0`: cuộn ngang một dòng dài mà số dòng trôi đi mất thì cuộn
 * xong không còn biết đang đứng ở đâu — và dòng dài đúng là lúc cần biết nhất.
 *
 * Cả khối chỉ cuộn **trong chính nó**. Một khung mã làm cả trang trượt ngang là cách
 * nhanh nhất đẩy thanh bên ra khỏi màn hình.
 */
export default function CodeViewer(props: {
  path: string;
  /** Gốc dự án, chỉ để cắt tiền tố lúc hiện. Đường dẫn thật vẫn là `path`. */
  root: string | null;
  file: FileView;
  /** Dòng cần nhảy tới, nếu chỗ mở tệp biết. Đánh dấu rồi cuộn tới. */
  line?: number;
}) {
  let scroller: HTMLDivElement | undefined;

  // Tên tệp trước, `lang` của lõi sau: `lang` là đuôi tệp trần, còn tên tệp mang cả
  // những trường hợp đuôi không nói đủ. Hai nguồn cùng đi qua một bảng.
  const lang = () => langFromPath(props.path) ?? langFromExtension(props.file.lang);
  const shownPath = () => displayPath(props.root, props.path);
  const lines = createMemo(() => highlight(props.file.text, lang()));
  const shown = createMemo(() => lines().slice(0, MAX_ROWS));
  const clipped = () => lines().length > MAX_ROWS;

  // Cuộn tới dòng được trỏ. Chạy lại khi *đường dẫn* hoặc *dòng* đổi: mở lại cùng một
  // tệp ở một dòng khác là chuyện thường xuyên khi đi qua danh sách khớp của grep.
  createEffect(
    on(
      () => [props.path, props.line] as const,
      () => {
        const target = props.line;
        if (target === undefined || !scroller) return;
        queueMicrotask(() => {
          scroller
            ?.querySelector(`[data-line="${target}"]`)
            ?.scrollIntoView({ block: "center", behavior: "auto" });
        });
      },
    ),
  );

  return (
    <div class="flex min-h-0 flex-1 flex-col">
      <header class="flex h-(--header-h) shrink-0 items-center gap-sm border-b border-line px-md">
        <span class="shrink-0 text-muted">
          <Icon name="file-code" size={15} />
        </span>
        {/* Tên tệp đứng trước, thư mục theo sau và co lại trước: khi chỗ hẹp thì thứ
            phải giữ lại là cái tên, không phải mấy cấp thư mục đầu. */}
        <span class="shrink-0 font-mono text-xs font-medium text-ink">{baseName(shownPath())}</span>
        <span class="min-w-0 truncate font-mono text-2xs text-faint" dir="rtl" title={props.path}>
          <bdi>{dirName(shownPath())}</bdi>
        </span>
        <span class="flex-1" />
        <Show when={lang()}>{(name) => <Chip>{name()}</Chip>}</Show>
        <span class="shrink-0 text-2xs text-faint tabular-nums">{props.file.totalLines} dòng</span>
        <CopyButton text={() => props.file.text} label="Chép nội dung tệp" />
      </header>

      {/* Tệp bị cắt phải nói ra *trước* khi người ta đọc, không phải sau dòng cuối: đọc
          hết rồi mới biết là thiếu thì kết luận đã rút xong rồi. */}
      <Show when={props.file.truncated || clipped()}>
        <p
          class="m-0 flex shrink-0 items-center gap-2xs border-b border-line bg-warn-soft px-md py-2xs text-2xs text-warn"
          role="status"
        >
          <Icon name="warn" size={13} />
          Chỉ hiện {shown().length} dòng đầu trên tổng {props.file.totalLines} — tệp đã bị cắt bớt.
        </p>
      </Show>

      <div ref={scroller} class="min-h-0 flex-1 overflow-auto bg-surface">
        <div class="w-max min-w-full font-mono text-2xs leading-[1.6]">
          <For each={shown()}>
            {(tokens, index) => {
              const number = () => index() + 1;
              const hit = () => props.line === number();
              return (
                <div
                  data-line={number()}
                  class="flex items-start"
                  style={hit() ? { background: "var(--code-line-hit)" } : undefined}
                >
                  <span
                    aria-hidden="true"
                    class="sticky left-0 w-12 shrink-0 border-r border-line bg-surface px-sm text-right text-faint tabular-nums select-none"
                    style={hit() ? { background: "var(--code-line-hit-solid)" } : undefined}
                  >
                    {number()}
                  </span>
                  <code class="px-sm whitespace-pre">
                    <Show when={tokens.length > 0} fallback={<span> </span>}>
                      <For each={tokens}>
                        {(token) => (
                          <span style={{ color: TOKEN_COLOR[token.kind] }}>{token.text}</span>
                        )}
                      </For>
                    </Show>
                  </code>
                </div>
              );
            }}
          </For>
        </div>
      </div>
    </div>
  );
}
