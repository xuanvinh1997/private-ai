import { For, Show, type JSX } from "solid-js";
import { displayMode } from "../lib/prefs";
import { clockTime } from "../lib/sessions";
import Icon, { type IconName } from "./Icon";
import { IconButton } from "./primitives";

export interface MessageAction {
  id: string;
  label: string;
  icon: IconName;
  danger?: boolean;
  onSelect: () => void;
}

/**
 * Khung chung của một tin nhắn: hàng avatar + tên + giờ, rồi nội dung, rồi thanh hành động.
 *
 * Hai chế độ, đúng như LobeChat:
 *   - **bong bóng**: người dùng dạt phải, trợ lý dạt trái, nội dung có nền và bo góc;
 *   - **tài liệu**: mọi thứ căn trái, toàn chiều rộng, không nền — đọc một lượt sửa mã
 *     dài bằng bong bóng là tự bóp cột chữ xuống còn một nửa.
 *
 * Thanh hành động chỉ hiện khi rê chuột **hoặc khi có tiêu điểm bàn phím ở bên trong**.
 * Vế thứ hai không phải là phần thêm cho đẹp: chỉ ẩn theo `:hover` thì với người dùng
 * bàn phím, mấy cái nút đó tồn tại nhưng không bao giờ nhìn thấy được.
 */
export default function MessageShell(props: {
  role: "user" | "assistant";
  name: string;
  at: number;
  actions?: MessageAction[];
  live?: boolean;
  busy?: boolean;
  children: JSX.Element;
}) {
  const bubble = () => displayMode() === "bubble";
  const mine = () => props.role === "user";
  const flip = () => bubble() && mine();

  return (
    <article
      class="group flex gap-md"
      classList={{ "flex-row-reverse": flip() }}
      aria-live={props.live ? "polite" : undefined}
      aria-busy={props.busy || undefined}
    >
      <div
        aria-hidden="true"
        class="mt-3xs grid size-(--avatar) shrink-0 place-items-center rounded-pill"
        classList={{
          "bg-accent text-on-accent": mine(),
          "bg-surface-hover text-accent-ink": !mine(),
        }}
      >
        <Icon name={mine() ? "chat" : "sparkle"} size={15} />
      </div>

      <div class="flex min-w-0 flex-1 flex-col gap-2xs" classList={{ "items-end": flip() }}>
        <div class="flex items-baseline gap-sm text-2xs">
          <span class="font-medium text-muted">{props.name}</span>
          <time class="text-faint tabular-nums">{clockTime(props.at)}</time>
        </div>

        <div
          class="min-w-0 max-w-full"
          classList={{
            "rounded-bubble bg-accent px-(--card-pad-x) py-(--card-pad-y) text-on-accent":
              bubble() && mine(),
            "rounded-bubble border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)":
              bubble() && !mine(),
            "w-full": !bubble(),
          }}
        >
          {props.children}
        </div>

        <Show when={props.actions && props.actions.length > 0}>
          <div
            class="flex items-center gap-3xs opacity-0 transition-opacity duration-[var(--dur-fast)] group-hover:opacity-100 group-focus-within:opacity-100"
            classList={{ "flex-row-reverse": flip() }}
          >
            <For each={props.actions}>
              {(action) => (
                <IconButton
                  icon={action.icon}
                  label={action.label}
                  size="sm"
                  danger={action.danger}
                  onClick={action.onSelect}
                />
              )}
            </For>
          </div>
        </Show>
      </div>
    </article>
  );
}
