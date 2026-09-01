import { Show } from "solid-js";
import type { IconName } from "../Icon";
import DialogShell, { Button } from "./DialogShell";

/**
 * Hộp xác nhận cho những việc không hoàn lại được.
 *
 * Chữ trên nút xác nhận do chỗ gọi đặt, và đó là điểm quan trọng nhất của component này:
 * một nút ghi "Đồng ý" không nói được việc sắp xảy ra, còn "Bỏ khỏi danh sách" và "Xoá
 * khỏi thư viện" thì nói được — và hai việc đó khác nhau xa. Người đọc chỉ nhìn cái nút
 * họ sắp bấm chứ không đọc lại câu hỏi phía trên.
 *
 * `window.confirm` làm được việc này nhưng không mang được đoạn giải thích *thư mục trên
 * đĩa không bị đụng tới*, mà chính đoạn đó mới là thứ giữ người dùng dám bấm.
 *
 * Tiêu điểm rơi vào nút **Huỷ**, không vào nút xác nhận — cố ý, và là chỗ duy nhất trong
 * đợt này lệch khỏi luật "Enter xác nhận". Hộp thoại này chỉ mở ra trước những việc
 * không hoàn lại được, và một cú Enter theo quán tính từ màn hình trước sẽ thực hiện
 * đúng cái việc người dùng đang được hỏi lại. Esc vẫn đóng, và Tab một nhịp là tới nút
 * xác nhận.
 */
export default function ConfirmDialog(props: {
  title: string;
  /** Câu nói rõ chuyện gì xảy ra và chuyện gì **không** xảy ra. */
  body: string;
  detail?: string;
  confirmLabel: string;
  icon?: IconName;
  busy?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <DialogShell
      icon={props.icon ?? "warn"}
      tone="danger"
      title={props.title}
      busy={props.busy}
      onClose={props.onClose}
      footer={() => (
        <>
          <Button onClick={props.onClose} disabled={props.busy}>
            Huỷ
          </Button>
          <Button variant="danger" onClick={props.onConfirm} disabled={props.busy}>
            {props.confirmLabel}
          </Button>
        </>
      )}
    >
      <p class="m-0 text-sm text-text">{props.body}</p>
      <Show when={props.detail}>
        {(text) => (
          <p
            class="m-0 min-w-0 truncate rounded-panel bg-surface-soft px-sm py-2xs font-mono text-2xs text-muted"
            dir="rtl"
            title={text()}
          >
            <bdi>{text()}</bdi>
          </p>
        )}
      </Show>
    </DialogShell>
  );
}
