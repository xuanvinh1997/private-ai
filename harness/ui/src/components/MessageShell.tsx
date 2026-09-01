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
 * Hai chế độ:
 *   - **bong bóng**: tin của người dùng là một bong bóng dạt phải, **không có avatar**;
 *   - **tài liệu**: mọi thứ căn trái, toàn chiều rộng — đọc một lượt sửa mã dài bằng bong
 *     bóng là tự bóp cột chữ xuống còn một nửa.
 *
 * Câu trả lời của trợ lý **không có thẻ**: không viền, không nền, chữ chảy thẳng trên nền
 * trang với đúng một avatar bên trái. Đây là hình dạng của ChatGPT, và lý do không phải
 * chuyện thẩm mỹ — bọc mỗi câu trả lời trong một khung làm hai câu liên tiếp đọc ra là hai
 * mẩu rời rạc, trong khi thứ nằm trong đó thường là một mạch giải thích dài. Bong bóng chỉ
 * còn ở phía người dùng, đúng chỗ ChatGPT vẫn giữ nó: một câu ngắn cần một hình dạng nói
 * rằng nó do người gõ.
 *
 * Avatar phía người dùng bị bỏ trong chế độ bong bóng vì một vòng tròn màu nhấn cỡ lớn đứng
 * trên một bong bóng vài chữ làm lệch hẳn tỉ lệ — bên phải đã đủ nói "của tôi". Chế độ tài
 * liệu thì **giữ** nó: ở đó không có bên phải bên trái, và bỏ avatar đi thì chữ của người
 * dùng bắt đầu ở một cột khác chữ của trợ lý.
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
      <Show when={!flip()}>
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
      </Show>

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
            // Trợ lý: không viền, không nền, không đệm — chữ chảy thẳng trên nền trang.
            "w-full": !bubble() || !mine(),
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
