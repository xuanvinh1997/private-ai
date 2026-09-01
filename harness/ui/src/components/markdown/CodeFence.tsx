import { Show } from "solid-js";
import { langLabel } from "./fences";
import { CopyButton } from "../primitives";

/**
 * Khối mã có rào, ngôn ngữ bất kỳ.
 *
 * Mượn nguyên khung của `DiffBlock`: cùng viền, cùng hàng tiêu đề có nhãn bên trái và
 * nút bên phải, cùng khung cuộn ngang riêng. Hai thứ này đứng cạnh nhau trong một bản
 * ghi, và hai kiểu khung khác nhau cho hai khối mã khiến mắt phải đọc lại bố cục.
 */
export default function CodeFence(props: { lang: string; code: string; streaming?: boolean }) {
  return (
    <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
      <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
        <figcaption class="min-w-0 truncate text-2xs text-muted">
          {langLabel(props.lang)}
          <Show when={props.streaming}>
            <span class="text-faint"> · đang nhận</span>
          </Show>
        </figcaption>
        <CopyButton text={() => props.code} label="Chép khối mã" />
      </div>

      {/* Cuộn ngang trong khung riêng — dòng mã dài không được kéo giãn cả bản ghi. */}
      <div class="overflow-x-auto" aria-busy={props.streaming === true}>
        <pre class="m-0 w-max min-w-full px-sm py-2xs font-mono text-2xs leading-[1.55] text-text">
          {props.code === "" ? " " : props.code}
        </pre>
      </div>
    </figure>
  );
}
