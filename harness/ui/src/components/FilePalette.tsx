import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { baseName, dirName } from "../lib/changes";
import { fileIcon } from "../lib/code";
import { displayPath } from "../lib/projects";
import Icon from "./Icon";

/** Bao nhiêu kết quả vẽ ra. Quá số này thì việc cần làm là gõ thêm chữ, không phải cuộn. */
const LIMIT = 60;

/**
 * Bảng tìm tệp theo tên (⌘/Ctrl+P).
 *
 * Cùng khuôn với `SessionPalette` — cùng lớp phủ, cùng ô nhập, cùng luật mũi tên và
 * Enter. Hai bảng lệnh trông khác nhau là hai thứ phải học riêng, trong khi chúng làm
 * đúng một việc trên hai loại dữ liệu.
 *
 * Khác đúng một chỗ và chỗ đó đáng nói: danh sách tệp **không có sẵn**. Cây bên trái nạp
 * lười từng cấp, còn tìm theo tên thì phải biết mọi tên. Nên bảng này tự xin một lần một
 * cây sâu lúc mở lần đầu và giữ lại — trả giá một lần, ở đúng lúc người dùng đã nói rằng
 * họ cần nó.
 */
export default function FilePalette(props: {
  load: () => Promise<string[]>;
  /** Gốc dự án, để hiện đường dẫn ngắn. Gõ tìm vẫn khớp trên đường dẫn đầy đủ. */
  root: string | null;
  onPick: (path: string) => void;
  onClose: () => void;
}) {
  let panel: HTMLDivElement | undefined;
  const [query, setQuery] = createSignal("");
  const [cursor, setCursor] = createSignal(0);
  const [paths] = createResource(props.load);

  useFocusTrap(() => panel, props.onClose);

  const matches = createMemo(() => {
    const all = paths() ?? [];
    const needle = query().trim().toLowerCase();
    if (needle === "") return all.slice(0, LIMIT);
    // Tên tệp khớp thì lên trước đường dẫn khớp: gõ "config" là đang tìm `config.rs`,
    // không phải tìm mọi tệp nằm trong thư mục `config/`.
    const scored: { path: string; rank: number }[] = [];
    for (const path of all) {
      const name = baseName(path).toLowerCase();
      const inName = name.indexOf(needle);
      if (inName >= 0) scored.push({ path, rank: inName });
      else if (path.toLowerCase().includes(needle)) scored.push({ path, rank: 1000 });
    }
    return scored
      .sort((a, b) => a.rank - b.rank || a.path.length - b.path.length)
      .slice(0, LIMIT)
      .map((entry) => entry.path);
  });

  const move = (delta: number) => {
    const count = matches().length;
    if (count === 0) return;
    setCursor((current) => (current + delta + count) % count);
  };

  const pick = () => {
    const chosen = matches()[cursor()];
    if (chosen !== undefined) props.onPick(chosen);
  };

  return (
    <div
      class="fixed inset-0 z-40 flex items-start justify-center p-4xl"
      style={{ background: "var(--scrim)" }}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label="Tìm tệp"
        class="flex max-h-[60vh] w-full max-w-[620px] flex-col overflow-hidden rounded-card border border-line bg-surface shadow-pop"
      >
        <input
          type="search"
          value={query()}
          placeholder="Tìm tệp theo tên…"
          aria-label="Tên tệp"
          aria-controls="file-palette-results"
          onInput={(event) => {
            setQuery(event.currentTarget.value);
            setCursor(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              move(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              move(-1);
            } else if (event.key === "Enter") {
              event.preventDefault();
              pick();
            }
          }}
          class="border-b border-line bg-transparent px-(--dialog-pad-x) py-md text-base text-text outline-none placeholder:text-faint"
        />

        <Show
          when={!paths.loading}
          fallback={
            <p class="px-(--dialog-pad-x) py-md text-sm text-muted" role="status" aria-live="polite">
              Đang nạp danh sách tệp…
            </p>
          }
        >
          <Show
            when={paths.error === undefined}
            fallback={
              <p class="px-(--dialog-pad-x) py-md text-sm text-danger" role="alert">
                Không nạp được danh sách tệp: {String(paths.error)}
              </p>
            }
          >
            <Show
              when={matches().length > 0}
              fallback={
                <p class="px-(--dialog-pad-x) py-md text-sm text-faint">Không có tệp nào khớp.</p>
              }
            >
              <ul
                id="file-palette-results"
                role="listbox"
                aria-label="Kết quả"
                class="m-0 list-none overflow-y-auto p-2xs"
              >
                <For each={matches()}>
                  {(path, index) => (
                    <li role="presentation">
                      <button
                        type="button"
                        role="option"
                        onClick={() => props.onPick(path)}
                        onMouseEnter={() => setCursor(index())}
                        aria-selected={index() === cursor()}
                        class="flex w-full items-center gap-sm rounded-btn px-md py-2xs text-left transition-colors hover:bg-surface-hover aria-[selected=true]:bg-accent-soft aria-[selected=true]:text-accent-ink"
                      >
                        <span class="shrink-0 text-muted">
                          <Icon name={fileIcon(path)} size={14} />
                        </span>
                        <span class="min-w-0 shrink-0 truncate font-mono text-sm">
                          {baseName(path)}
                        </span>
                        <span
                          class="min-w-0 flex-1 truncate text-2xs text-faint"
                          dir="rtl"
                          title={path}
                        >
                          <bdi>{dirName(displayPath(props.root, path))}</bdi>
                        </span>
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </Show>
      </div>
    </div>
  );
}
