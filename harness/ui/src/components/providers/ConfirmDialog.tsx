import { Show } from "solid-js";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import Icon from "./../Icon";
import { InfoDot } from "../settings/FormKit";

/**
 * Hộp thoại xác nhận một việc không hoàn tác được.
 *
 * Đặt ở đây thay vì ở `components/` vì đợt việc này chỉ sở hữu hai thư mục `providers/`
 * và `mcp/`; màn hình MCP mượn lại chính tệp này thay vì chép ra bản thứ hai, vì hai bản
 * sao của một hộp thoại là hai bản sao của luật bàn phím, và bản thứ hai luôn là bản quên
 * cập nhật. Lúc tích hợp thì nâng nó lên `components/ConfirmDialog.tsx`.
 *
 * Nút huỷ được focus đầu tiên và Esc đóng: khi người dùng chỉ đập Enter cho xong thì thứ
 * họ chạm vào phải là lựa chọn không mất gì.
 */
export default function ConfirmDialog(props: {
  title: string;
  body: string;
  /** Đoạn giải thích dài đằng sau câu hỏi, cất trong `InfoDot` cạnh tiêu đề. */
  more?: string;
  /** Dòng phụ mang chi tiết máy móc — đường dẫn, dòng lệnh, tên. Hiện bằng font mono. */
  detail?: string;
  confirmLabel: string;
  busy?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  // Không kéo tiêu điểm về đâu cả: nút đầu tiên trong khung *là* nút Huỷ, nên mặc định
  // của bẫy tiêu điểm đã trỏ đúng chỗ an toàn rồi.
  let panel: HTMLDivElement | undefined;

  useFocusTrap(() => panel, props.onClose);

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center p-lg"
      style={{ background: "var(--scrim)" }}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={panel}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby="confirm-body"
        class="flex w-full max-w-[440px] flex-col gap-(--dialog-gap) rounded-card border border-line bg-surface px-(--dialog-pad-x) py-(--dialog-pad-y) shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
      >
        <div class="flex items-start gap-sm">
          <span class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel bg-danger-soft text-danger">
            <Icon name="warn" size={16} />
          </span>
          <div class="flex min-w-0 flex-col gap-3xs">
            <h2 id="confirm-title" class="m-0 flex items-center gap-2xs text-md font-semibold text-ink">
              {props.title}
              <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
            </h2>
            <p id="confirm-body" class="m-0 text-xs text-muted">
              {props.body}
            </p>
          </div>
        </div>

        <Show when={props.detail}>
          {(detail) => (
            <p class="m-0 overflow-x-auto rounded-panel border border-line bg-surface-soft px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
              {detail()}
            </p>
          )}
        </Show>

        <div class="flex justify-end gap-sm">
          <button
            type="button"
            onClick={props.onClose}
            class="h-(--control-h) rounded-btn border border-line-strong px-md text-xs font-medium text-text transition-colors duration-[var(--dur-fast)] hover:bg-surface-hover"
          >
            Huỷ
          </button>
          <button
            type="button"
            disabled={props.busy}
            aria-busy={props.busy}
            onClick={props.onConfirm}
            class="h-(--control-h) rounded-btn bg-danger px-md text-xs font-medium text-on-accent transition-colors duration-[var(--dur-fast)] enabled:hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {props.confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
