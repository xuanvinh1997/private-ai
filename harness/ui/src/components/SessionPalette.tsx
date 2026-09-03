import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { rankSessions, relativeTime } from "../lib/sessions";
import Icon from "./Icon";

import type { SessionSummary } from "../lib/protocol";

/**
 * Bảng lệnh tìm phiên (⌘/Ctrl+K).
 *
 * Chỉ làm một việc: lọc theo tên rồi mở. Nó không cố trở thành bảng lệnh đa năng —
 * thêm hành động vào đây trước khi có hành động thứ hai đáng thêm là cách nhanh nhất
 * biến một ô tìm kiếm thành một menu không ai nhớ nổi.
 *
 * Việc lọc thì nằm ở [`rankSessions`]: bỏ dấu trước khi so, bắt mọi token phải khớp, và
 * xếp hạng theo chỗ khớp rơi vào đâu. Để ở `lib/` vì đó là logic thuần, kiểm chứng được
 * mà không cần dựng DOM.
 *
 * # Ngữ nghĩa ARIA
 *
 * Tiêu điểm **không bao giờ rời ô nhập** — mũi tên chỉ dời một con trỏ vẽ bằng
 * `aria-selected`. Nên ô nhập phải là một `combobox` trỏ vào hàng đang sáng bằng
 * `aria-activedescendant`: thiếu nó thì người dùng trình đọc màn hình bấm mũi tên và
 * **không nghe thấy gì**, vì thứ duy nhất thay đổi là một màu nền.
 */
export default function SessionPalette(props: {
  sessions: SessionSummary[];
  /** Phiên đang mở, để đánh dấu nó trong danh sách. */
  currentId?: string;
  onPick: (id: string) => void;
  onClose: () => void;
}) {
  let panel: HTMLDivElement | undefined;
  let list: HTMLUListElement | undefined;
  const [query, setQuery] = createSignal("");
  const [cursor, setCursor] = createSignal(0);

  useFocusTrap(() => panel, props.onClose);

  const matches = createMemo(() => rankSessions(props.sessions, query()));

  const optionId = (index: number) => `palette-opt-${index}`;

  // Con trỏ đi bằng bàn phím phải kéo theo khung nhìn. Danh sách cuộn được mà con trỏ
  // không cuộn theo thì bấm mũi tên lần thứ tám là con trỏ biến mất khỏi màn hình, và
  // Enter mở một phiên người dùng không nhìn thấy. `block: "nearest"` cuộn tối thiểu —
  // đủ để hàng lọt vào, không giật cả danh sách về giữa.
  createEffect(() => {
    const index = cursor();
    if (matches().length === 0) return;
    list?.children[index]?.scrollIntoView({ block: "nearest" });
  });

  const move = (delta: number) => {
    const count = matches().length;
    if (count === 0) return;
    setCursor((current) => (current + delta + count) % count);
  };

  const jump = (to: number) => {
    if (matches().length > 0) setCursor(to);
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
          type="text"
          role="combobox"
          value={query()}
          placeholder="Tìm phiên…"
          aria-label="Tên phiên"
          aria-controls="palette-results"
          aria-expanded={matches().length > 0}
          aria-autocomplete="list"
          aria-activedescendant={matches().length > 0 ? optionId(cursor()) : undefined}
          autocomplete="off"
          spellcheck={false}
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
            } else if (event.key === "Home") {
              event.preventDefault();
              jump(0);
            } else if (event.key === "End") {
              event.preventDefault();
              jump(matches().length - 1);
            } else if (event.key === "Enter") {
              event.preventDefault();
              pick();
            }
          }}
          class="border-b border-line bg-transparent px-(--dialog-pad-x) py-md text-base text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
        />
        <Show
          when={matches().length > 0}
          fallback={
            <p class="flex items-center gap-2xs px-(--dialog-pad-x) py-md text-sm text-faint">
              <Icon name="search" size={14} />
              Không có phiên nào khớp.
            </p>
          }
        >
          <ul
            ref={list}
            id="palette-results"
            role="listbox"
            aria-label="Kết quả"
            class="m-0 list-none overflow-y-auto p-2xs"
          >
            <For each={matches()}>
              {(session, index) => (
                <li role="presentation">
                  <button
                    type="button"
                    id={optionId(index())}
                    role="option"
                    onClick={() => props.onPick(session.id)}
                    onMouseEnter={() => setCursor(index())}
                    aria-selected={index() === cursor()}
                    class="flex w-full flex-col items-start gap-3xs rounded-btn px-md py-2xs text-left transition-colors hover:bg-surface-hover aria-[selected=true]:bg-accent-soft aria-[selected=true]:text-accent-ink"
                  >
                    <span class="flex w-full items-center gap-xs">
                      <span class="min-w-0 flex-1 truncate text-sm text-text">{session.title}</span>
                      {/* Phiên đang mở. Nhãn chữ chứ không phải một chấm màu: ⌘K mở ra
                          giữa lúc đang đọc một phiên, và "đang mở" là thứ trả lời câu hỏi
                          "tôi đang đứng ở đâu" — một chấm thì phải đoán. */}
                      <Show when={session.id === props.currentId}>
                        <span class="shrink-0 text-2xs text-faint">đang mở</span>
                      </Show>
                    </span>
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
