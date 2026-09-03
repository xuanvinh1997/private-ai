import { createSignal } from "solid-js";

/**
 * Thông báo nổi: chỗ đứng của những câu **vừa xảy ra**.
 *
 * # Vì sao không để chúng dưới ô soạn tin
 *
 * Dưới ô soạn tin là chỗ của những điều kiện **đang tồn tại** — chưa mở dự án, mô hình
 * đang cảnh báo, ngữ cảnh sắp đầy. Chúng ở lại vì tình trạng ở lại, và người dùng đọc
 * chúng khi tới lượt. Một câu lỗi thì ngược hẳn: nó nói về một cử chỉ vừa xong, nó đúng
 * trong vài giây, và nó phải tự đi. Trộn hai giọng ấy vào cùng một dải chữ làm hỏng cả
 * hai — dải điều kiện nhấp nháy theo từng cú bấm, còn câu lỗi thì nằm im như thể nó là
 * một tình trạng mới của ứng dụng.
 *
 * Chỗ này cũng là chỗ duy nhất nói được khi màn hình gây ra lỗi đã không còn trước mắt:
 * một cú thả tệp hỏng vẫn phải nói được thành lời sau khi người dùng đã chuyển sang tab
 * khác.
 *
 * # Vì sao một kho ở tầng module chứ không phải một context
 *
 * Cùng khuôn với `prefs.ts` và `theme.ts`. Bất kỳ đâu cũng gọi `notify` được mà không phải
 * xâu một prop qua bốn tầng component, và một ứng dụng một cửa sổ thì không có cái thứ hai
 * để tách kho ra cho.
 */

export type ToastKind = "error" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

/**
 * Ba cái một lúc là hết. Cái thứ tư đẩy cái **già nhất** đi.
 *
 * Một chồng thông báo cao hơn thế thì không ai đọc từ đầu; nó chỉ che mất giao diện và tự
 * biến mình thành thứ người dùng học cách nhìn xuyên qua.
 */
const MAX = 3;

/**
 * Tám giây, và cùng một khoảng cho cả lỗi.
 *
 * Lỗi ở đây luôn là lỗi của một **thao tác vừa làm**, không phải một tình trạng của ứng
 * dụng: tệp này không đính kèm được, hộp thoại này không mở được. Người dùng biết mình vừa
 * làm gì, nên câu trả lời chỉ cần sống đủ lâu để đọc hết. Thứ đáng ở lại thì đã có chỗ ở
 * lại — dòng điều kiện dưới ô soạn tin, hoặc dải lỗi của chính màn hình gây ra nó.
 *
 * Vẫn có nút đóng: tám giây là quá lâu khi người dùng đã đọc xong ở giây thứ hai và cái
 * thẻ ấy đang nằm trên đúng chỗ họ muốn bấm.
 */
const LIFETIME_MS = 8_000;

const [toasts, setToasts] = createSignal<Toast[]>([]);
export { toasts };

let seq = 0;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function forget(id: number) {
  const timer = timers.get(id);
  if (timer !== undefined) clearTimeout(timer);
  timers.delete(id);
}

function arm(id: number) {
  forget(id);
  timers.set(
    id,
    setTimeout(() => {
      forget(id);
      setToasts((all) => all.filter((toast) => toast.id !== id));
    }, LIFETIME_MS),
  );
}

/**
 * Đẩy một thông báo lên.
 *
 * Câu trùng nguyên văn với một thông báo đang hiện thì **không** thành thẻ thứ hai: nó chỉ
 * đặt lại đồng hồ của cái đang có. Thả hỏng ba lần liên tiếp là cùng một tin xấu ba lần,
 * và xếp nó thành ba thẻ chồng lên nhau biến một câu đọc được thành một bức tường.
 */
export function notify(kind: ToastKind, text: string): void {
  const trimmed = text.trim();
  if (trimmed === "") return;

  const existing = toasts().find((toast) => toast.kind === kind && toast.text === trimmed);
  if (existing !== undefined) {
    arm(existing.id);
    return;
  }

  const toast: Toast = { id: ++seq, kind, text: trimmed };
  setToasts((all) => {
    const next = [...all, toast];
    // Cắt từ đầu: cái già nhất đi trước. Cái vừa tới nói về cử chỉ vừa xong, nên nó là cái
    // cuối cùng đáng bị bỏ.
    for (const dropped of next.slice(0, Math.max(0, next.length - MAX))) forget(dropped.id);
    return next.slice(-MAX);
  });
  arm(toast.id);
}

export function dismissToast(id: number): void {
  forget(id);
  setToasts((all) => all.filter((toast) => toast.id !== id));
}
