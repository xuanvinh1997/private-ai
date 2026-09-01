import { Show } from "solid-js";
import { embedState } from "../../lib/docs";
import type { DocumentView } from "../../lib/protocol";
import Icon, { type IconName } from "../Icon";

/**
 * Trạng thái nhúng của một tài liệu — chỗ quan trọng nhất của cả màn hình.
 *
 * Ba trạng thái phải phân biệt được bằng **ba thứ cùng lúc**: chữ, màu, và hình. Chỉ
 * dựa vào màu là bỏ rơi người mù màu; chỉ dựa vào chữ thì mắt phải đọc từng dòng của một
 * bảng ba mươi dòng mới thấy dòng nào hỏng.
 *
 * "Đang xếp hàng" cố ý **không** dùng màu cảnh báo. Nó không phải một vấn đề: tài liệu
 * đó đã tìm được bằng từ khoá rồi, chỉ là chưa có vector. Tô nó vàng là mời người dùng
 * xoá đi nạp lại một tệp hoàn toàn bình thường — và lần nạp lại cũng sẽ "vàng" y như vậy.
 */
export default function EmbedBadge(props: { doc: DocumentView }) {
  const state = () => embedState(props.doc);
  const icon = (): IconName =>
    state() === "embedded" ? "check" : state() === "queued" ? "clock" : "warn";
  const label = () =>
    state() === "embedded" ? "Đã nhúng" : state() === "queued" ? "Đang xếp hàng" : "Hỏng";

  return (
    <span class="flex flex-col items-start gap-3xs">
      <span
        class="inline-flex items-center gap-3xs rounded-pill px-2xs py-3xs text-2xs whitespace-nowrap"
        classList={{
          "bg-success-soft text-success": state() === "embedded",
          "bg-[var(--overlay-faint)] text-muted": state() === "queued",
          "bg-danger-soft text-danger": state() === "failed",
        }}
      >
        <span classList={{ "motion-safe:animate-pulse": state() === "queued" }}>
          <Icon name={icon()} size={11} />
        </span>
        {label()}
      </span>
      {/* Lý do hỏng đứng ngay dưới huy hiệu, không giấu sau một cú rê chuột: nó là thứ
          duy nhất nói được phải làm gì tiếp, và rê chuột thì bàn phím không tới được. */}
      <Show when={props.doc.error}>
        {(reason) => <span class="max-w-[28ch] text-2xs text-danger">{reason()}</span>}
      </Show>
    </span>
  );
}
