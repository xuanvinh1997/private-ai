import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { useFocusTrap } from "../hooks/useFocusTrap";
import type { PendingApproval } from "../lib/conversation";
import { intendedDiffs } from "../lib/diff";
import type { ApprovalDecision } from "../lib/protocol";
import DiffBlock from "./DiffBlock";
import { prettyArgs, toolLabel } from "./tools/ToolCard";

/**
 * Hết giờ mặc định khi lõi không nói. Có một hạn là bắt buộc: một hộp thoại đứng mãi
 * chặn cả lượt, và người dùng có thể đã bỏ đi từ lâu.
 */
const DEFAULT_TIMEOUT_MS = 120_000;

/**
 * Hộp thoại duyệt một tool call.
 *
 * Luật duy nhất không được phép mềm đi: **fail-closed**. Đóng hộp thoại, bấm Esc, hết
 * giờ, hay component bị gỡ giữa chừng — tất cả đều là *từ chối*. Không có nhánh nào
 * dẫn tới "cho phép" mà không phải là một cú bấm cố ý vào đúng cái nút đó.
 *
 * Và **không có "nhớ lựa chọn"**. Một lần cho phép là một lần cho phép: quyết định
 * dính lại là cách một câu trả lời đúng cho lệnh này trở thành câu trả lời sai cho lệnh
 * sau, mà không ai được hỏi lại.
 */
export default function ApprovalDialog(props: {
  request: PendingApproval;
  onDecide: (decision: ApprovalDecision) => void;
}) {
  let panel: HTMLDivElement | undefined;
  let rejectButton: HTMLButtonElement | undefined;
  let answered = false;

  const budget = () => props.request.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const [left, setLeft] = createSignal(budget());

  const decide = (decision: ApprovalDecision) => {
    if (answered) return;
    answered = true;
    props.onDecide(decision);
  };

  useFocusTrap(
    () => panel,
    () => decide("rejected"),
  );

  onMount(() => {
    // Focus trap tự đưa tiêu điểm vào phần tử focus được đầu tiên, mà phần tử đó là nút
    // "Chép" trong khối diff. Kéo về nút Từ chối: lựa chọn an toàn phải là lựa chọn
    // người dùng chạm vào khi họ chỉ đập Enter cho xong.
    rejectButton?.focus();

    const started = Date.now();
    const tick = setInterval(() => {
      const remaining = budget() - (Date.now() - started);
      setLeft(remaining);
      if (remaining <= 0) decide("rejected");
    }, 250);
    onCleanup(() => clearInterval(tick));
  });

  // Gỡ khỏi cây mà chưa trả lời = từ chối. Đây là mắt lưới cuối: chuyển phiên, lỗi
  // render, hot reload — không đường nào để câu hỏi trôi qua thành "cho phép".
  onCleanup(() => decide("rejected"));

  const seconds = () => Math.max(0, Math.ceil(left() / 1000));
  const diffs = () => intendedDiffs(props.request.name, props.request.args);

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center p-lg"
      style={{ background: "var(--scrim)" }}
      // Bấm ra ngoài cũng là từ chối, không phải "bỏ qua câu hỏi".
      onClick={(event) => {
        if (event.target === event.currentTarget) decide("rejected");
      }}
    >
      <div
        ref={panel}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="approval-title"
        aria-describedby="approval-body"
        class="flex max-h-full w-full max-w-[560px] flex-col gap-(--dialog-gap) overflow-auto rounded-card border border-line bg-surface px-(--dialog-pad-x) py-(--dialog-pad-y) shadow-pop"
      >
        <h2 id="approval-title" class="m-0 text-lg font-semibold text-ink">
          Cho phép chạy {toolLabel(props.request.name)}?
        </h2>

        <div id="approval-body" class="flex flex-col gap-sm">
          <p class="m-0 text-sm text-muted">
            Trợ lý muốn gọi <code class="font-mono text-accent-ink">{props.request.name}</code>.
            <Show when={props.request.reason}>
              {(reason) => <span class="block text-text">{reason()}</span>}
            </Show>
          </p>

          <Show when={diffs()}>
            {(list) => <DiffBlock diffs={list()} maxLines={12} />}
          </Show>

          <pre class="max-h-56 overflow-auto rounded-panel border border-line bg-surface-soft px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
            {prettyArgs(props.request.args)}
          </pre>

          <p class="m-0 text-2xs text-faint" role="timer" aria-live="off">
            Không trả lời trong {seconds()} giây nữa thì tự động từ chối.
          </p>
        </div>

        <div class="flex justify-end gap-sm">
          {/* Từ chối đứng trước và là nút được focus đầu tiên: lựa chọn an toàn phải là
              lựa chọn dễ nhất, kể cả khi người dùng chỉ đập Enter cho xong. */}
          <button
            ref={rejectButton}
            type="button"
            onClick={() => decide("rejected")}
            class="h-(--cta-h) rounded-btn border border-line-strong px-lg text-sm font-medium text-text transition-colors hover:bg-surface-hover"
          >
            Từ chối
          </button>
          <button
            type="button"
            onClick={() => decide("allowed_once")}
            class="h-(--cta-h) rounded-btn bg-accent px-lg text-sm font-medium text-on-accent transition-colors hover:bg-accent-hover"
          >
            Cho phép một lần
          </button>
        </div>
      </div>
    </div>
  );
}
