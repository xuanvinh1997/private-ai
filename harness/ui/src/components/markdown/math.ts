import type { TokenizerAndRendererExtension, Tokens } from "marked";

/**
 * Bốn cặp dấu công thức, thêm vào bộ tách token của marked.
 *
 * Markdown không có công thức toán — `$…$` là quy ước của TeX, và mọi công cụ hiển thị nó
 * đều phải tự dán thêm. Không dán thì `$\frac{a}{b}$` hiện nguyên văn ra màn hình, kèm cả
 * dấu đô la, và đó là thứ người dùng đang nhìn thấy.
 *
 * **Nhận cả bốn cặp, không chỉ `$`.** `$…$`/`$$…$$` là quy ước của TeX; `\(…\)`/`\[…\]`
 * là thứ các mô hình OpenAI sinh ra gần như mặc định. Chọn một cặp thôi nghĩa là công
 * thức hiện đúng hay không phụ thuộc vào việc người dùng đang cắm provider nào — một kiểu
 * hỏng không ai lần ra được.
 *
 * **Vẫn dừng ở token, không dựng HTML.** Đây là lý do phần này là một extension của
 * `lexer` chứ không phải một phép thay chuỗi trước khi lex: token là dữ liệu, và chuỗi
 * TeX đi thẳng từ token sang [`../../lib/katex`], nơi nó thành nút DOM. Không có bước nào
 * chữ của mô hình biến thành đánh dấu.
 */

/** Chuỗi TeX rỗng không phải công thức — nó chỉ là hai dấu đô la người ta gõ cạnh nhau. */
function token(
  type: string,
  raw: string | undefined,
  tex: string | undefined,
): Tokens.Generic | undefined {
  // Nhóm bắt của một biểu thức chính quy là `string | undefined` với cấu hình của repo,
  // nên chỗ này nhận cả hai và tự lọc — thay vì rải `!` ra sáu chỗ gọi.
  const text = tex?.trim() ?? "";
  return raw === undefined || text === "" ? undefined : { type, raw, text };
}

/**
 * Công thức đứng riêng một khối.
 *
 * Phải là **block-level**: một công thức trưng bày nằm giữa hai đoạn văn, và để nó ở tầng
 * inline thì marked gói nó vào `<p>` cùng dòng chữ liền trước, rồi mọi thứ căn giữa của
 * KaTeX kéo cả đoạn ấy theo.
 */
export const mathBlock: TokenizerAndRendererExtension = {
  name: "mathBlock",
  level: "block",

  // `start` nói cho marked biết chỗ **gần nhất** có thể có công thức. Thiếu nó thì đoạn
  // văn phía trước nuốt luôn cả công thức vào làm chữ thường.
  start(src: string) {
    return src.match(/\$\$|\\\[/)?.index;
  },

  tokenizer(src: string) {
    const dollars = /^\$\$([\s\S]+?)\$\$(?:\n+|$)/.exec(src);
    if (dollars !== null) return token("mathBlock", dollars[0], dollars[1]);
    const brackets = /^\\\[([\s\S]+?)\\\](?:\n+|$)/.exec(src);
    if (brackets !== null) return token("mathBlock", brackets[0], brackets[1]);
    return undefined;
  },

  // Không có đường nào gọi tới — cả cây đi qua `lexer()`, không qua `parse()`. Có mặt để
  // nếu một ngày ai đó gọi `parse()` thì thứ rơi ra là chữ, không phải đánh dấu.
  renderer(t) {
    return t.text;
  },
};

/**
 * Công thức nằm trong dòng.
 *
 * Chỗ khó duy nhất là **đồng đô la**. "$5 và $10" không phải công thức, nhưng nó khớp mọi
 * biểu thức chính quy ngây thơ, và hỏng theo kiểu tệ nhất: một câu về giá tiền biến thành
 * một dòng ký hiệu toán học nghiêng nghiêng. Ba luật dưới đây là bộ lọc tối thiểu mà mọi
 * bản cài đặt nghiêm túc đều có:
 *
 * 1. Không có khoảng trắng ngay sau dấu mở hay ngay trước dấu đóng — "$5 và " bị loại vì
 *    dấu đóng của nó đứng sau một khoảng trắng.
 * 2. Không có chữ số ngay sau dấu đóng, nên "$10" không đóng được một công thức mở ở "$5".
 * 3. Thân rỗng thì bỏ qua.
 *
 * Viết bằng kiểm tra trong mã chứ không bằng `lookbehind`: hai cách cho cùng kết quả, còn
 * cách này đọc được và sửa được mà không phải giải mã một biểu thức chính quy dài gấp đôi.
 */
export const mathInline: TokenizerAndRendererExtension = {
  name: "mathInline",
  level: "inline",

  start(src: string) {
    return src.match(/\$|\\\(/)?.index;
  },

  tokenizer(src: string) {
    const parens = /^\\\(([\s\S]+?)\\\)/.exec(src);
    if (parens !== null) return token("mathInline", parens[0], parens[1]);

    const dollars = /^\$((?:\\.|[^\\$])+?)\$(?!\d)/.exec(src);
    const body = dollars?.[1];
    if (body === undefined) return undefined;
    if (/^\s/.test(body) || /\s$/.test(body)) return undefined;
    return token("mathInline", dollars?.[0], body);
  },

  renderer(t) {
    return t.text;
  },
};

export const MATH_EXTENSIONS = [mathBlock, mathInline];
