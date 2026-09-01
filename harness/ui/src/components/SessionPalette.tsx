import { createMemo, createSignal, For, Show } from "solid-js";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { relativeTime } from "../lib/sessions";

import type { SessionSummary } from "../lib/protocol";

/**
 * Bảng lệnh tìm phiên (⌘/Ctrl+K).
 *
 * Chỉ làm một việc: lọc theo tên rồi mở. Nó không cố trở thành bảng lệnh đa năng —
 * thêm hành động vào đây trước khi có hành động thứ hai đáng thêm là cách nhanh nhất
 * biến một ô tìm kiếm thành một menu không ai nhớ nổi.
 */
export default function SessionPalette(props: {
  sessions: SessionSummary[];
  onPick: (id: string) => void;
  onClose: () => void;
}) {
  let panel: HTMLDivElement | undefined;
  const [query, setQuery] = createSignal("");
  const [cursor, setCursor] = createSignal(0);

  useFocusTrap(() => panel, props.onClose);

  const matches = createMemo(() => {
    const needle = query().trim().toLowerCase();
    const all = props.sessions;
    if (needle === "") return all;
    return all.filter((session) => session.title.toLowerCase().includes(needle));
  });

  const move = (delta: number) => {
    const count = matches().length;
    if (count === 0) return;
    setCursor((current) => (current + delta + count) % count);
  };

  const pick = () => {
    const chosen = matches()[cursor()];
    if (chosen) props.onPick(chosen.id);
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
        aria-label="Tìm phiên"
        class="flex max-h-[60vh] w-full max-w-[520px] flex-col overflow-hidden rounded-card border border-line bg-surface shadow-pop"
      >
        <input
          type="search"
          value={query()}
          placeholder="Tìm phiên…"
          aria-label="Tên phiên"
          aria-controls="palette-results"
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
          when={matches().length > 0}
          fallback={<p class="px-(--dialog-pad-x) py-md text-sm text-faint">Không có phiên nào khớp.</p>}
        >
          <ul id="palette-results" role="listbox" aria-label="Kết quả" class="m-0 list-none overflow-y-auto p-2xs">
            <For each={matches()}>
              {(session, index) => (
                <li role="presentation">
                  <button
                    type="button"
                    role="option"
                    onClick={() => props.onPick(session.id)}
                    onMouseEnter={() => setCursor(index())}
                    aria-selected={index() === cursor()}
                    class="flex w-full flex-col items-start gap-3xs rounded-btn px-md py-2xs text-left transition-colors hover:bg-surface-hover aria-[selected=true]:bg-accent-soft aria-[selected=true]:text-accent-ink"
                  >
                    <span class="w-full truncate text-sm text-text">{session.title}</span>
                    <span class="text-2xs text-faint">{relativeTime(session.updatedAt)}</span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </div>
  );
}
