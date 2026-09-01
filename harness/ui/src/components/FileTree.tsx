import { Key } from "@solid-primitives/keyed";
import { createEffect, createSignal, For, on, Show } from "solid-js";
import { fileIcon } from "../lib/code";
import type { TreeEntry } from "../lib/protocol";
import Icon from "./Icon";

interface Row {
  entry: TreeEntry;
  depth: number;
  parent: string | null;
  posinset: number;
  setsize: number;
}

/**
 * Cây tệp, nạp lười từng cấp một.
 *
 * Hai quyết định đáng nói:
 *
 * **Nạp một cấp mỗi lần mở.** `list_tree` nhận `depth`, và cám dỗ là xin luôn cả cây cho
 * xong. Một repo thật có hàng chục nghìn tệp; xin cả cây là treo giao diện trước khi vẽ
 * được dòng đầu tiên, và người dùng chỉ nhìn vào ba thư mục trong số đó.
 *
 * **DOM phẳng, cấp bậc khai bằng `aria-level`.** Đây là kiểu cây được ARIA cho phép, và
 * nó làm cho điều hướng bàn phím trở thành phép cộng chỉ số trên một mảng — thay vì một
 * cuộc đi bộ đệ quy qua cây DOM mỗi lần bấm mũi tên. Cây lồng nhau trông đúng hơn trong
 * mã nguồn nhưng sai thường xuyên hơn trên bàn phím.
 */
export default function FileTree(props: {
  /** Nạp con của `path`; `path` vắng nghĩa là gốc dự án. */
  load: (path?: string) => Promise<TreeEntry[]>;
  /** Đổi giá trị là đổi dự án: cây vứt sạch trạng thái cũ và nạp lại từ gốc. */
  resetKey: string;
  selected: string | null;
  onOpen: (path: string) => void;
}) {
  const [roots, setRoots] = createSignal<TreeEntry[] | null>(null);
  const [kids, setKids] = createSignal(new Map<string, TreeEntry[]>());
  const [expanded, setExpanded] = createSignal(new Set<string>());
  const [pending, setPending] = createSignal(new Set<string>());
  const [failed, setFailed] = createSignal(new Map<string, string>());
  const [rootError, setRootError] = createSignal<string | null>(null);
  const [active, setActive] = createSignal<string | null>(null);

  const seats = new Map<string, HTMLElement>();

  /** Thêm/bớt một khoá trong một `Set` phản ứng. `Set` được thay mới để signal nổ. */
  const flip = (
    read: () => Set<string>,
    write: (next: Set<string>) => void,
    key: string,
    present: boolean,
  ) => {
    const next = new Set(read());
    if (present) next.add(key);
    else next.delete(key);
    write(next);
  };

  /** Nạp con của một thư mục. `undefined` là gốc. */
  async function fetchLevel(path?: string) {
    const key = path ?? "";
    flip(pending, setPending, key, true);
    try {
      const entries = await props.load(path);
      if (path === undefined) setRoots(entries);
      else setKids((map) => new Map(map).set(path, entries));
      setFailed((map) => {
        const next = new Map(map);
        next.delete(key);
        return next;
      });
    } catch (err) {
      if (path === undefined) setRootError(String(err));
      else setFailed((map) => new Map(map).set(path, String(err)));
    } finally {
      flip(pending, setPending, key, false);
    }
  }

  // Đổi dự án thì cây cũ không còn nghĩa gì: giữ lại phần đã mở sẽ hiện đường dẫn của dự
  // án trước dưới cái tên của dự án sau.
  createEffect(
    on(
      () => props.resetKey,
      () => {
        seats.clear();
        setRoots(null);
        setKids(new Map());
        setExpanded(new Set<string>());
        setFailed(new Map());
        setRootError(null);
        setActive(null);
        void fetchLevel();
      },
    ),
  );

  const rows = (): Row[] => {
    const out: Row[] = [];
    const walk = (entries: TreeEntry[], depth: number, parent: string | null) => {
      entries.forEach((entry, index) => {
        out.push({ entry, depth, parent, posinset: index + 1, setsize: entries.length });
        if (!entry.isDir || !expanded().has(entry.path)) return;
        const children = entry.children ?? kids().get(entry.path);
        if (children) walk(children, depth + 1, entry.path);
      });
    };
    walk(roots() ?? [], 1, null);
    return out;
  };

  const focusRow = (path: string | null) => {
    if (path === null) return;
    setActive(path);
    seats.get(path)?.focus();
  };

  const openDir = (entry: TreeEntry) => {
    flip(expanded, setExpanded, entry.path, true);
    // `children` có sẵn nghĩa là cấp này đã về cùng cấp cha; xin lại là một vòng IPC
    // thừa và một nháy "đang nạp" cho thứ đã nằm sẵn trong bộ nhớ.
    if (entry.children === undefined && !kids().has(entry.path)) void fetchLevel(entry.path);
  };

  const activate = (entry: TreeEntry) => {
    if (!entry.isDir) {
      props.onOpen(entry.path);
      return;
    }
    if (expanded().has(entry.path)) flip(expanded, setExpanded, entry.path, false);
    else openDir(entry);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const list = rows();
    if (list.length === 0) return;
    const at = list.findIndex((row) => row.entry.path === active());
    const row = list[at];

    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        focusRow(list[Math.min(at + 1, list.length - 1)]?.entry.path ?? list[0]!.entry.path);
        return;
      case "ArrowUp":
        event.preventDefault();
        focusRow(list[Math.max(at - 1, 0)]?.entry.path ?? list[0]!.entry.path);
        return;
      case "Home":
        event.preventDefault();
        focusRow(list[0]!.entry.path);
        return;
      case "End":
        event.preventDefault();
        focusRow(list[list.length - 1]!.entry.path);
        return;
      case "ArrowRight":
        if (!row) return;
        event.preventDefault();
        // Thư mục đã mở thì mũi tên phải *đi vào*, không mở lại. Đó là chỗ duy nhất
        // trong bàn phím của cây mà một phím làm hai việc khác nhau, và luật là của ARIA.
        if (row.entry.isDir && !expanded().has(row.entry.path)) openDir(row.entry);
        else if (row.entry.isDir) focusRow(list[at + 1]?.entry.path ?? null);
        return;
      case "ArrowLeft":
        if (!row) return;
        event.preventDefault();
        if (row.entry.isDir && expanded().has(row.entry.path)) {
          flip(expanded, setExpanded, row.entry.path, false);
        } else {
          focusRow(row.parent);
        }
        return;
      case "Enter":
      case " ":
        if (!row) return;
        event.preventDefault();
        activate(row.entry);
        return;
      default:
    }
  };

  const busyLabel = () => {
    if (pending().has("")) return "Đang nạp cây tệp…";
    const first = [...pending()][0];
    return first === undefined ? "" : `Đang nạp ${first.split("/").pop() ?? first}…`;
  };

  return (
    <div class="flex min-h-0 flex-1 flex-col">
      <Show
        when={rootError() === null}
        fallback={
          <div class="flex flex-col items-start gap-sm p-md">
            <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-2xs text-danger" role="alert">
              {rootError()}
            </p>
            <button
              type="button"
              onClick={() => {
                setRootError(null);
                void fetchLevel();
              }}
              class="flex items-center gap-2xs rounded-btn px-sm py-3xs text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
            >
              <Icon name="retry" size={12} />
              Thử lại
            </button>
          </div>
        }
      >
        <Show
          when={roots() !== null}
          fallback={<TreeSkeleton />}
        >
          <Show
            when={rows().length > 0}
            fallback={<p class="p-md text-2xs text-faint">Thư mục này không có gì để hiện.</p>}
          >
            <div
              role="tree"
              aria-label="Cây tệp của dự án"
              onKeyDown={onKeyDown}
              class="min-h-0 flex-1 overflow-auto py-2xs"
            >
              {/* Keyed theo đường dẫn, không theo vị trí. `rows()` dựng lại toàn bộ mảng mỗi
                  lần mở một thư mục; keyed theo vị trí thì mọi hàng bị tạo lại, và tiêu
                  điểm bàn phím rơi về `body` ngay giữa lúc người dùng đang đi bằng mũi tên. */}
              <Key each={rows()} by={(row) => row.entry.path}>
                {(keyed) => {
                  // Đọc một lần là đủ: `depth`, `posinset` và `setsize` do chính đường
                  // dẫn quyết định, mà đường dẫn là khoá — nên chúng không đổi dưới chân
                  // một hàng đang sống. Phần đổi được (`expanded`, `pending`) đọc trong JSX.
                  const row = keyed();
                  return (
                  <div
                    ref={(el) => seats.set(row.entry.path, el)}
                    role="treeitem"
                    aria-level={row.depth}
                    aria-posinset={row.posinset}
                    aria-setsize={row.setsize}
                    aria-expanded={row.entry.isDir ? expanded().has(row.entry.path) : undefined}
                    aria-selected={props.selected === row.entry.path}
                    aria-busy={pending().has(row.entry.path) || undefined}
                    // Roving tabindex: đúng **một** hàng nhận được Tab. Cho mọi hàng
                    // `tabindex=0` thì đi qua một cây mở sẵn tốn vài trăm lần bấm Tab.
                    tabindex={
                      active() === row.entry.path || (active() === null && row.posinset === 1 && row.depth === 1)
                        ? 0
                        : -1
                    }
                    onFocus={() => setActive(row.entry.path)}
                    onClick={() => {
                      setActive(row.entry.path);
                      activate(row.entry);
                    }}
                    style={{ "padding-left": `calc(${row.depth - 1} * var(--sp-md) + var(--sp-sm))` }}
                    class="flex cursor-default items-center gap-2xs py-3xs pr-sm text-xs transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] aria-[selected=true]:bg-accent-soft"
                    classList={{
                      "text-accent-ink": props.selected === row.entry.path,
                      "text-text": props.selected !== row.entry.path,
                    }}
                  >
                    <span class="grid size-4 shrink-0 place-items-center text-faint">
                      <Show when={row.entry.isDir}>
                        <Icon
                          name="chevron-right"
                          size={12}
                          class={`transition-transform duration-[var(--dur-fast)] ${
                            expanded().has(row.entry.path) ? "rotate-90" : ""
                          }`}
                        />
                      </Show>
                    </span>
                    <span
                      class="shrink-0"
                      classList={{
                        "text-accent": row.entry.isDir,
                        "text-muted": !row.entry.isDir,
                      }}
                    >
                      <Icon
                        name={
                          row.entry.isDir
                            ? expanded().has(row.entry.path)
                              ? "folder-open"
                              : "folder"
                            : fileIcon(row.entry.path)
                        }
                        size={14}
                      />
                    </span>
                    <span class="min-w-0 truncate" title={row.entry.path}>
                      {row.entry.name}
                    </span>
                    <Show when={pending().has(row.entry.path)}>
                      <span class="ml-auto shrink-0 text-2xs text-faint">đang nạp…</span>
                    </Show>
                    <Show when={failed().get(row.entry.path)}>
                      {(message) => (
                        <span class="ml-auto shrink-0 text-2xs text-danger" title={message()}>
                          lỗi
                        </span>
                      )}
                    </Show>
                  </div>
                  );
                }}
              </Key>
            </div>
          </Show>
        </Show>
      </Show>

      {/* Vùng thông báo cho trình đọc màn hình. Chữ "đang nạp…" ở cuối hàng chỉ có mắt
          đọc được, và một cây nạp lười mà không nói gì thì với bàn phím nó chỉ là im lặng. */}
      <p class="sr-only" role="status" aria-live="polite">
        {busyLabel()}
      </p>
    </div>
  );
}

/** Khung xương cùng nhịp thụt đầu dòng với cây thật, để lúc dữ liệu về không có cú nhảy. */
function TreeSkeleton() {
  const widths = [62, 48, 74, 40, 56, 68, 44, 60];
  return (
    <div class="flex flex-col gap-2xs px-sm py-xs" aria-hidden="true">
      <For each={widths}>
        {(width, index) => (
          <div
            class="h-3 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse"
            style={{ width: `${width}%`, "margin-left": `${(index() % 3) * 12}px` }}
          />
        )}
      </For>
    </div>
  );
}
