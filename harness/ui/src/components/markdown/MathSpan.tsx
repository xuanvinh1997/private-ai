import { createEffect, createResource, createSignal, Show } from "solid-js";
import { loadKatex, renderMath } from "../../lib/katex";

/**
 * Một công thức toán, vẽ bằng KaTeX vào một nút do Solid sở hữu.
 *
 * Ba trạng thái, và cả ba đều hiện ra **chính chuỗi TeX** chứ không hiện một ô trống:
 *
 * - *Đang nạp* — bó KaTeX về trễ hơn tin nhắn. Trong nhịp đó chữ phải có mặt, nếu không
 *   một bản ghi dài trông như bị thủng lỗ chỗ rồi từng lỗ lần lượt được vá.
 * - *Hỏng* — mô hình gõ sai cú pháp. Chuỗi gốc là thứ duy nhất giúp người dùng bảo nó sửa
 *   chỗ nào, nên nó nằm ngay đó cùng thông điệp lỗi.
 * - *Không nạp được* — không có KaTeX thì vẫn còn TeX, và TeX vẫn đọc được bằng mắt.
 *
 * Không bao giờ để mất chuỗi gốc là luật của cả tệp này: một công thức biến mất im lặng
 * lấy đi cả nội dung câu trả lời, còn một công thức chưa vẽ được thì chỉ xấu.
 */
export default function MathSpan(props: { tex: string; display: boolean }) {
  const [katex] = createResource(loadKatex);
  const [error, setError] = createSignal<string | null>(null);
  let host: HTMLElement | undefined;

  createEffect(() => {
    const mod = katex();
    // Đọc cả hai prop **trước** khi thoát sớm, để hiệu ứng còn theo dõi được chúng: đổi
    // công thức trong lúc gói chưa về mà không đọc ở đây thì lần vẽ sau không chạy lại.
    const tex = props.tex;
    const display = props.display;
    if (mod === undefined || host === undefined) return;
    setError(renderMath(mod, host, tex, display));
  });

  return (
    <Show
      when={katex.error === undefined && error() === null}
      fallback={<Fallback tex={props.tex} display={props.display} message={error()} />}
    >
      {/* `display` là khối riêng và cuộn ngang được: một ma trận sáu cột không được đẩy
          cả bản ghi hội thoại rộng ra. */}
      <Show
        when={props.display}
        fallback={<span ref={(el) => (host = el)}>{props.tex}</span>}
      >
        <div class="overflow-x-auto py-2xs text-center" ref={(el) => (host = el)}>
          {props.tex}
        </div>
      </Show>
    </Show>
  );
}

/** Chưa vẽ được thì hiện chuỗi gốc dưới dạng mã, kèm lý do nếu có. */
function Fallback(props: { tex: string; display: boolean; message: string | null }) {
  const code = (
    <code class="rounded-btn bg-[var(--overlay-faint)] px-3xs py-px font-mono text-2xs text-text">
      {props.tex}
    </code>
  );
  return (
    <Show when={props.display} fallback={code}>
      <div class="flex flex-col gap-3xs overflow-x-auto py-2xs">
        {code}
        <Show when={props.message}>
          {(message) => <span class="text-2xs text-danger">Không vẽ được: {message()}</span>}
        </Show>
      </div>
    </Show>
  );
}
