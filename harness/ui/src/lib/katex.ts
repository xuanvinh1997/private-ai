import "katex/dist/katex.min.css";

/**
 * Tầng bọc quanh KaTeX. Cùng ba quyết định như [`./mermaid`], vì cùng một bài toán.
 *
 * **Nạp trễ, nhưng chữ nghĩa thì nạp sẵn.** Gói KaTeX nặng gần 300 KB, và phần lớn phiên
 * làm việc không có công thức nào — nên `import()` chỉ chạy ở công thức đầu tiên. Tệp CSS
 * thì ngược lại, nó nằm trong bó chính: nó nhỏ, và nạp trễ nó nghĩa là công thức đầu tiên
 * hiện ra một nhịp không có kiểu dáng rồi mới nhảy vào đúng chỗ.
 *
 * **Phông chữ nằm trong bản dựng.** KaTeX vẽ bằng bộ phông riêng của nó, và bản CSS trên
 * mạng trỏ tới CDN. Ở đây `import` đi qua Vite, nên phông được sao vào `dist` và tải từ
 * chính ứng dụng. Đó là điều kiện để công thức hiện đúng trên một máy không có mạng —
 * mà "chạy được khi rút dây mạng" là cả lý do ứng dụng này tồn tại.
 *
 * **`trust: false`.** Chuỗi TeX do mô hình sinh ra, mà mô hình vừa đọc tài liệu người
 * dùng nạp lên — nên nó thực chất có thể do người ngoài viết. Cờ này khoá `\href`,
 * `\includegraphics` và cả họ `\html…`, tức là mọi lệnh TeX dựng ra được URL hay thuộc
 * tính DOM. Đây là mặc định của KaTeX, và nó được viết ra ở đây vì nó là hàng rào chứ
 * không phải một tuỳ chọn hiển thị: ai đó bật nó lên để cho `\href` chạy sẽ mở lại đúng
 * con đường mà `securityLevel: "strict"` của mermaid đang bịt.
 *
 * Còn một lẽ nữa để dùng `render` chứ không `renderToString`: `render` dựng **nút DOM**
 * bằng `createElement` rồi `appendChild`. Không có chuỗi HTML nào được dựng từ chữ của mô
 * hình, đúng luật mà cả `Markdown.tsx` đang giữ.
 */

export type KatexModule = typeof import("katex").default;

let pending: Promise<KatexModule> | null = null;

/** Chỉ nạp một lần cho cả phiên; hỏng thì cho phép thử lại ở công thức sau. */
export function loadKatex(): Promise<KatexModule> {
  if (pending === null) {
    pending = import("katex")
      .then((mod) => mod.default)
      .catch((err) => {
        pending = null;
        throw err;
      });
  }
  return pending;
}

/**
 * Vẽ một công thức vào sẵn một nút. Trả về thông điệp lỗi, hoặc `null` khi vẽ được.
 *
 * Không bao giờ ném: mô hình sinh sai cú pháp TeX thường xuyên, và ở chỗ gọi thì "vẽ
 * hỏng" cần hiện ra cho người dùng đọc chứ không cần thêm một `try` nữa.
 */
export function renderMath(
  katex: KatexModule,
  host: HTMLElement,
  tex: string,
  display: boolean,
): string | null {
  try {
    katex.render(tex, host, {
      displayMode: display,
      // Ném ra để ta tự vẽ phần hỏng — bản vẽ lỗi sẵn của KaTeX là một dòng đỏ không nói
      // được mô hình sai ở đâu, và người dùng cần đúng thông tin đó để bảo nó sửa.
      throwOnError: true,
      trust: false,
      // "warn" là mặc định, và nó đổ vào console mỗi lần mô hình gõ một chữ có dấu trong
      // chế độ toán — chuyện xảy ra liên tục ở một ứng dụng tiếng Việt.
      strict: false,
    });
    return null;
  } catch (err) {
    if (err instanceof Error && err.message !== "") return err.message;
    return "KaTeX không đọc được công thức này.";
  }
}
