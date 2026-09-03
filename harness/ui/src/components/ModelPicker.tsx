import { createMemo, createSignal, createUniqueId, For, onCleanup, Show } from "solid-js";
import type { ModelChoice } from "../lib/protocol";
import Icon from "./Icon";

/**
 * Mô hình có được phép đứng trong bộ chọn hội thoại không.
 *
 * Giấu đúng nhóm **chỉ** nhúng được. Chọn một mô hình nhúng để trò chuyện là hội thoại
 * chết — nó không sinh chữ — nên nó không được có mặt ở đây.
 *
 * Lọc theo `chat === true` thoạt nhìn chặt hơn nhưng sai hướng: một máy chủ Ollama đời cũ
 * không có trường `capabilities` buộc lõi đoán năng lực theo tên, và một lần đoán trượt khi
 * ấy làm biến mất một mô hình người dùng đang dùng được. Hiện thừa một dòng thì họ chọn
 * nhầm một lần rồi thôi; giấu nhầm một dòng thì họ không có cách nào tìm lại nó.
 */
export const usableForChat = (choice: ModelChoice): boolean =>
  !(choice.embedding && !choice.chat);

/**
 * Bộ chọn mô hình, ngồi **trong** ô soạn tin.
 *
 * Chỗ ngồi là điểm chính của component này. Bản trước để tên mô hình trên thanh tiêu đề,
 * nơi nó đọc như một thuộc tính của cả cửa sổ; nhưng mô hình là thuộc tính của **tin nhắn
 * sắp gửi**, và ChatGPT đã dời nó xuống composer đúng vì lý do đó (release notes 28/04/2026:
 * "Model selection now appears in the composer… so you can pick the right model before
 * sending a message"). Đứng cạnh nút Gửi thì nó được đọc lại ngay trước mỗi lần bấm.
 *
 * Không dùng `Menu` chung được: mỗi hàng ở đây mang hai dòng và hai huy hiệu, còn `Menu`
 * nhận một danh sách phẳng mỗi hàng một nhãn. Hai huy hiệu đó không phải trang trí — một
 * mô hình không gọi được tool sẽ trả lời trôi chảy mà không bao giờ đọc được tệp nào, và
 * đó là thứ phải đọc được **trước** khi chọn, không phải sau.
 *
 * Chỗ này là nơi duy nhất phần "nhiều nhà cung cấp mô hình" lộ ra trong luồng làm việc
 * hằng ngày, nên nó mang luôn lối đi tới màn hình cấu hình provider.
 */
export default function ModelPicker(props: {
  value: string;
  models: ModelChoice[];
  onPick: (id: string) => void;
  /** Mở màn hình cài đặt → nhà cung cấp mô hình. */
  onManageProviders: () => void;
  disabled?: boolean;
}) {
  const id = createUniqueId();
  const [open, setOpen] = createSignal(false);
  const choices = createMemo(() => props.models.filter(usableForChat));
  let popup: HTMLDivElement | undefined;
  let trigger: HTMLButtonElement | undefined;

  // Bấm ra ngoài đóng menu. Nghe ở pha bắt để cú bấm đó không kịp kích hoạt một nút khác
  // trước khi menu biết mình phải đóng.
  const onDocPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (popup?.contains(target ?? null) || trigger?.contains(target ?? null)) return;
    setOpen(false);
  };
  document.addEventListener("pointerdown", onDocPointerDown, true);
  onCleanup(() => document.removeEventListener("pointerdown", onDocPointerDown, true));

  const move = (delta: number) => {
    const buttons = [...(popup?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    if (buttons.length === 0) return;
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[(at + delta + buttons.length) % buttons.length]?.focus();
  };

  const close = (restore: boolean) => {
    setOpen(false);
    if (restore) trigger?.focus();
  };

  return (
    <div class="relative">
      <button
        ref={trigger}
        type="button"
        disabled={props.disabled}
        aria-haspopup="menu"
        aria-expanded={open()}
        aria-controls={id}
        aria-label={`Mô hình: ${props.value}. Bấm để đổi.`}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
            queueMicrotask(() => move(1));
          }
        }}
        class="flex h-(--control-h) items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-sm text-xs text-muted transition-colors duration-[var(--dur-fast)] disabled:opacity-40 enabled:hover:bg-[var(--overlay-hover)] enabled:hover:text-ink"
      >
        <Icon name="model" size={13} />
        <span class="max-w-40 truncate">{props.value}</span>
        <Icon name="chevron-down" size={12} />
      </button>

      <Show when={open()}>
        <div
          ref={popup}
          id={id}
          role="menu"
          aria-label="Mô hình"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              close(true);
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              move(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              move(-1);
            }
          }}
          // Bung **lên**: ô soạn tin nằm sát đáy cửa sổ, và một menu bung xuống từ đó không
          // có chỗ nào để rơi vào.
          class="absolute bottom-full left-0 z-40 mb-3xs flex w-[min(22rem,72vw)] flex-col rounded-menu border border-line bg-surface p-3xs shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        >
          {/* Hai lý do rỗng, hai câu khác nhau. "Chưa hỏi được máy chủ" là một thứ có thể
              hỏng ở mạng; "chỉ có mô hình nhúng" là một máy chủ trả lời tử tế mà không có
              gì dùng được ở đây, và việc phải làm là đi nạp một mô hình trò chuyện. Gộp
              lại thành một câu là dạy người dùng đi sửa nhầm chỗ. */}
          <Show
            when={choices().length > 0}
            fallback={
              <p class="m-0 flex items-center gap-2xs px-sm py-xs text-2xs text-faint">
                <Icon name="model" size={13} />
                {props.models.length === 0
                  ? "Chưa hỏi được máy chủ mô hình nào."
                  : "Chỉ có mô hình nhúng, không trò chuyện được."}
              </p>
            }
          >
            <ul class="m-0 flex max-h-72 list-none flex-col gap-3xs overflow-y-auto p-0">
              <For each={choices()}>
                {(choice) => (
                  <li>
                    <button
                      type="button"
                      role="menuitemradio"
                      aria-checked={choice.id === props.value}
                      onClick={() => {
                        close(false);
                        props.onPick(choice.id);
                      }}
                      class="flex w-full items-start gap-sm rounded-btn px-sm py-2xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] aria-[checked=true]:bg-accent-soft"
                    >
                      <span
                        class="mt-3xs shrink-0"
                        classList={{
                          "text-accent-ink": choice.id === props.value,
                          "text-transparent": choice.id !== props.value,
                        }}
                      >
                        <Icon name="check" size={13} />
                      </span>
                      <span class="flex min-w-0 flex-1 flex-col gap-3xs">
                        <span class="min-w-0 truncate font-mono text-xs text-text">
                          {choice.id}
                        </span>
                        <span class="flex flex-wrap items-center gap-2xs text-2xs">
                          {/* Nói ra ngay ở đây thay vì để một câu cảnh báo hiện lên sau khi
                              đã chọn: hậu quả của việc chọn nhầm là một trợ lý im lặng vô
                              dụng, và người dùng không có manh mối nào để đoán ra. */}
                          <Show
                            when={choice.tools}
                            fallback={<span class="text-warn">không gọi được công cụ</span>}
                          >
                            <span class="text-muted">gọi được công cụ</span>
                          </Show>
                          <Show when={choice.contextWindow}>
                            {(size) => (
                              <span class="text-faint tabular-nums">
                                · {Math.round(size() / 1024)}K ngữ cảnh
                              </span>
                            )}
                          </Show>
                        </span>
                      </span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>

          <div class="mt-3xs border-t border-line pt-3xs">
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                close(false);
                props.onManageProviders();
              }}
              class="flex w-full items-center gap-sm rounded-btn px-sm py-2xs text-left text-xs text-text transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
            >
              <Icon name="server" size={14} />
              Nhà cung cấp mô hình…
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
}
