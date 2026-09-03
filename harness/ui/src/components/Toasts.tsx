import { For } from "solid-js";
import { dismissToast, toasts } from "../lib/toast";
import Icon from "./Icon";
import { IconButton } from "./primitives";

/**
 * Chồng thông báo nổi, ở góc trên bên phải khu làm việc.
 *
 * # Vì sao ở đó
 *
 * Ngay dưới thanh trên (`top-(--header-h)`), tức là *trong* khu làm việc chứ không đè lên
 * thanh tiêu đề: dải trên cùng là vùng kéo cửa sổ và là chỗ ngồi của nút bảng thay đổi, và
 * một thẻ thông báo che mất chúng biến một câu nói giúp thành một vật cản.
 *
 * Không ở góc dưới bên phải, dù đó là chỗ quen của toast: góc ấy đã có nút "xuống cuối" của
 * bản ghi, và ô soạn tin thì chiếm hết bề ngang bên dưới. Trên-phải là góc duy nhất của cửa
 * sổ này không có gì tranh chỗ.
 *
 * # Vì sao `z-[60]`
 *
 * Trên cả hộp thoại (`z-50`). Một thông báo thường sinh ra từ đúng cái hộp thoại đang mở,
 * và giấu nó sau hộp thoại ấy là giấu đúng câu trả lời cho việc người dùng vừa làm.
 *
 * Cả dải không nhận chuột (`pointer-events-none`), chỉ từng thẻ nhận: một vùng trong suốt
 * rộng bằng góc màn hình mà nuốt cú bấm là một lỗi không ai lần ra được, vì nó vô hình.
 */
export default function Toasts() {
  return (
    <div class="pointer-events-none fixed top-(--header-h) right-0 z-[60] flex w-[min(26rem,calc(100vw-2rem))] flex-col gap-2xs p-md">
      <For each={toasts()}>
        {(toast) => (
          <div
            // `alert` cho lỗi, `status` cho phần còn lại: lỗi nói về cử chỉ người dùng vừa
            // làm và họ đang chờ kết quả của nó, nên trình đọc màn hình phải cắt lời để đọc.
            role={toast.kind === "error" ? "alert" : "status"}
            class="pointer-events-auto flex items-start gap-2xs rounded-card border border-line bg-surface py-sm pr-2xs pl-md shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
          >
            <span
              class="mt-3xs shrink-0"
              classList={{
                "text-warn": toast.kind === "error",
                "text-muted": toast.kind !== "error",
              }}
              aria-hidden="true"
            >
              <Icon name={toast.kind === "error" ? "warn" : "bubble"} size={14} />
            </span>

            {/* `break-words` chứ không cắt bằng dấu ba chấm: câu lỗi hay mang tên tệp, và
                một cái tên bị cắt cụt bỏ đi đúng phần phân biệt được hai tệp cùng thư mục. */}
            <p class="m-0 min-w-0 flex-1 py-3xs text-xs break-words text-text">{toast.text}</p>

            <IconButton
              icon="x"
              size="sm"
              label="Đóng thông báo"
              tip="left"
              onClick={() => dismissToast(toast.id)}
            />
          </div>
        )}
      </For>
    </div>
  );
}
