import { Marked, type Token, type Tokens } from "marked";
import { createMemo, For, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import CodeFence from "./CodeFence";
import MathSpan from "./MathSpan";
import { MATH_EXTENSIONS } from "./math";

/**
 * Markdown dựng thành component Solid, **không** đi qua HTML.
 *
 * ## Vì sao `lexer()` chứ không `parse()`
 *
 * Chuỗi vào đây do mô hình sinh ra, mà mô hình vừa đọc tài liệu người dùng nạp lên và kết
 * quả tool MCP — tức là nó có thể chép nguyên một đoạn do người ngoài viết. `marked.parse()`
 * trả về một chuỗi HTML, và cách duy nhất đưa chuỗi đó lên màn hình là `innerHTML`: một
 * đường tiêm HTML thẳng vào cửa sổ ứng dụng. Bịt nó lại thì phải nuôi thêm một bộ lọc HTML,
 * và một bộ lọc là thứ đúng cho tới lần nó sai.
 *
 * `lexer()` dừng lại ở **cây token** — dữ liệu, không phải đánh dấu. Mỗi token thành một
 * component, mỗi chuỗi thành một text node do Solid đặt vào DOM. Không có HTML nào được
 * dựng, nên không có gì để lọc.
 *
 * Cùng lý do đó, token `html` (thẻ do mô hình viết ra) hiện **nguyên văn** dưới dạng chữ:
 * nó là thứ mô hình đã gõ, và đọc được nó là điều duy nhất người dùng cần.
 *
 * ## Vì sao có `source()` đứng trước `tokens()`
 *
 * Chỗ gọi dựng lại mảng đoạn ở mỗi token đến, nên `props.text` được đọc lại rất nhiều lần
 * với **cùng một chuỗi**. `createMemo` so sánh bằng `===`, nên `source()` nuốt những lần
 * đọc lại đó và `lexer` chỉ chạy khi chữ thật sự đổi.
 */
/**
 * Một thể hiện riêng, **không** phải `marked.use()` lên cái toàn cục.
 *
 * `marked` xuất ra một thể hiện dùng chung, và cắm extension vào đó là sửa hành vi của
 * mọi chỗ trong ứng dụng có gọi `marked` — kể cả những chỗ chưa tồn tại. Một thể hiện
 * riêng làm luật công thức toán chỉ áp đúng ở nơi nó được yêu cầu.
 */
const md = new Marked({ extensions: MATH_EXTENSIONS });

export default function Markdown(props: { text: string }) {
  const source = createMemo(() => props.text);
  const tokens = createMemo<Token[]>(() => md.lexer(source()));

  return <BlockSeq tokens={tokens()} gap="space-y-sm" />;
}

/**
 * Dãy khối xếp dọc. Khoảng cách truyền vào vì trong trích dẫn nó phải chặt hơn.
 *
 * Giãn cách bằng `space-y-*` chứ **không** bằng `flex flex-col gap-*`, và đó không phải
 * chuyện phong cách: con của một flex container bị blockify, nên một token chữ nằm ở vị
 * trí khối (mục danh sách chặt, thân trích dẫn) sẽ vỡ thành mỗi `<strong>` một dòng. Lề
 * trên thì vô hại với phần tử inline, nên cùng một khuôn dùng được cho cả hai.
 */
function BlockSeq(props: { tokens: Token[]; gap: string }) {
  return (
    <div class={props.gap}>
      <For each={props.tokens}>{(token) => <BlockToken token={token} />}</For>
    </div>
  );
}

/** `#` của mô hình bắt đầu từ `h2` — xem ghi chú ở nhánh `heading`. */
const HEADING_TAG = ["h2", "h3", "h4", "h5", "h6", "h6"] as const;

/**
 * Một token ở vị trí khối.
 *
 * Thân component đọc `props.token` đúng một lần chứ không bọc trong getter, và điều đó an
 * toàn vì cây token là ảnh chụp bất biến: chữ đổi thì `lexer` sinh ra mảng đối tượng hoàn
 * toàn mới, `<For>` thấy tham chiếu mới và dựng lại. Không có đường nào một token đang
 * hiển thị bị sửa tại chỗ.
 */
function BlockToken(props: { token: Token }) {
  const token = props.token as Tokens.Generic;

  switch (token.type) {
    // Dòng trống và định nghĩa liên kết tham chiếu không sinh ra gì trên màn hình.
    case "space":
    case "def":
      return null;

    case "heading": {
      const heading = props.token as Tokens.Heading;
      // Thanh trên đã giữ `h1` của trang, nên `#` của mô hình bắt đầu từ `h2`: hai `h1`
      // trong một trang làm dàn bài mà trình đọc màn hình dựng ra mất nghĩa.
      const tag = HEADING_TAG[Math.min(Math.max(heading.depth, 1), 6) - 1] ?? "h6";
      const size = heading.depth === 1 ? "text-lg" : heading.depth === 2 ? "text-md" : "text-base";
      return (
        // Thêm lề trên cho tiêu đề **không** đứng đầu: một tiêu đề cách đoạn trên nó đúng
        // bằng khoảng cách giữa hai đoạn văn thì nó không còn cắt được văn bản thành mục.
        <Dynamic
          component={tag}
          class={`m-0 font-semibold text-ink not-first:mt-sm ${size}`}
        >
          <InlineSeq tokens={heading.tokens} />
        </Dynamic>
      );
    }

    case "hr":
      return <hr class="m-0 border-0 border-t border-line" />;

    case "paragraph": {
      const paragraph = props.token as Tokens.Paragraph;
      return (
        <p class="m-0 leading-[1.6]">
          <InlineSeq tokens={paragraph.tokens} />
        </p>
      );
    }

    /* Khối mã **thụt lề**. Khối rào ```…``` không tới được đây: `splitFences` đã cắt chúng
       ra trước, để `CodeFence` và `Diagram` giữ nguyên đường đi đã có. */
    case "code": {
      const code = props.token as Tokens.Code;
      return <CodeFence lang={(code.lang ?? "").trim().split(/\s+/)[0] ?? ""} code={code.text} />;
    }

    case "blockquote": {
      const quote = props.token as Tokens.Blockquote;
      return (
        <blockquote class="m-0 border-l-2 border-line-strong pl-md text-muted">
          <BlockSeq tokens={quote.tokens} gap="space-y-2xs" />
        </blockquote>
      );
    }

    case "list": {
      const list = props.token as Tokens.List;
      const items = (
        <For each={list.items}>
          {(item) => (
            <li
              class="leading-[1.6]"
              // Ô đánh dấu thay chỗ cho chấm đầu dòng: hai dấu hiệu cạnh nhau cho cùng một
              // mục đọc ra là hai mục.
              classList={{ "list-none -ml-lg": item.task }}
            >
              <For each={item.tokens}>{(child) => <BlockToken token={child} />}</For>
            </li>
          )}
        </For>
      );
      // Không `flex` ở đây: con của flex container bị blockify, `display: list-item` mất
      // theo, và cả danh sách hiện ra không còn một chấm đầu dòng nào.
      return list.ordered ? (
        <ol
          start={typeof list.start === "number" && list.start !== 1 ? list.start : undefined}
          class="m-0 list-decimal space-y-3xs py-0 pr-0 pl-lg"
        >
          {items}
        </ol>
      ) : (
        <ul class="m-0 list-disc space-y-3xs py-0 pr-0 pl-lg">{items}</ul>
      );
    }

    case "table": {
      const table = props.token as Tokens.Table;
      return (
        // Cuộn ngang nằm trong khung riêng, đúng như `CodeFence`: một bảng tám cột không
        // được phép kéo giãn cả bản ghi.
        <div class="overflow-x-auto rounded-panel border border-line">
          <table class="w-max min-w-full border-collapse text-xs">
            <thead>
              <tr>
                <For each={table.header}>
                  {(cell) => (
                    <th
                      class="border-b border-line px-sm py-2xs font-medium text-ink"
                      style={{ "text-align": cell.align ?? "left" }}
                    >
                      <InlineSeq tokens={cell.tokens} />
                    </th>
                  )}
                </For>
              </tr>
            </thead>
            <tbody>
              <For each={table.rows}>
                {(row) => (
                  <tr class="border-t border-line first:border-t-0">
                    <For each={row}>
                      {(cell) => (
                        <td
                          class="px-sm py-2xs align-top"
                          style={{ "text-align": cell.align ?? "left" }}
                        >
                          <InlineSeq tokens={cell.tokens} />
                        </td>
                      )}
                    </For>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      );
    }

    /* Mục danh sách chặt: chữ nằm thẳng trong `<li>`, không có `<p>` bọc ngoài — đúng như
       markdown quy định, và cũng là thứ giữ ô đánh dấu đứng cùng dòng với chữ. */
    case "text": {
      const text = props.token as Tokens.Text;
      return (
        <Show when={text.tokens} fallback={text.text}>
          {(children) => <InlineSeq tokens={children()} />}
        </Show>
      );
    }

    case "mathBlock":
      return <MathSpan tex={token.text} display />;

    case "checkbox": {
      const box = props.token as Tokens.Checkbox;
      return (
        <input
          type="checkbox"
          checked={box.checked}
          disabled
          // Không bấm được là đúng: đây là chữ mô hình đã nói, không phải một biểu mẫu.
          // `aria-hidden` vì trạng thái đã nằm trong chính dòng chữ ngay cạnh nó.
          aria-hidden="true"
          class="mr-2xs align-[-1px] accent-[var(--accent)]"
        />
      );
    }

    default:
      return <RawText token={props.token} />;
  }
}

function InlineSeq(props: { tokens: Token[] }) {
  return <For each={props.tokens}>{(token) => <InlineToken token={token} />}</For>;
}

function InlineToken(props: { token: Token }) {
  const token = props.token as Tokens.Generic;

  switch (token.type) {
    case "text":
    case "escape": {
      const text = props.token as Tokens.Text;
      return (
        <Show when={text.tokens} fallback={text.text}>
          {(children) => <InlineSeq tokens={children()} />}
        </Show>
      );
    }

    case "strong": {
      const strong = props.token as Tokens.Strong;
      return (
        <strong class="font-semibold text-ink">
          <InlineSeq tokens={strong.tokens} />
        </strong>
      );
    }

    case "em": {
      const em = props.token as Tokens.Em;
      return (
        <em>
          <InlineSeq tokens={em.tokens} />
        </em>
      );
    }

    case "del": {
      const del = props.token as Tokens.Del;
      return (
        <del class="text-muted">
          <InlineSeq tokens={del.tokens} />
        </del>
      );
    }

    case "codespan": {
      const code = props.token as Tokens.Codespan;
      return (
        <code class="rounded-btn bg-[var(--overlay-faint)] px-3xs py-px font-mono text-2xs">
          {code.text}
        </code>
      );
    }

    case "mathInline":
      return <MathSpan tex={token.text} display={false} />;

    case "br":
      return <br />;

    case "link": {
      const link = props.token as Tokens.Link;
      return <LinkOut href={link.href} title={link.title ?? undefined} tokens={link.tokens} />;
    }

    /* Ảnh hiện thành **liên kết**, không thành `<img>`: nạp một URL nằm trong chuỗi do mô
       hình sinh ra là một lần gọi mạng ra ngoài mà người dùng không yêu cầu, và trong một
       ứng dụng dựng để chạy tại chỗ thì đó là một cái đèn hiệu. Muốn xem thì bấm. */
    case "image": {
      const image = props.token as Tokens.Image;
      return (
        <LinkOut
          href={image.href}
          title={image.title ?? undefined}
          tokens={[{ type: "text", raw: image.text, text: `🖼 ${image.text || image.href}` }]}
        />
      );
    }

    case "checkbox":
      return <BlockToken token={props.token} />;

    default:
      return <RawText token={props.token} />;
  }
}

/**
 * Token không có cách dựng riêng — chủ yếu là `html`.
 *
 * Hiện **nguyên văn nguồn**. Thẻ do mô hình viết ra không được trở thành thẻ thật, và một
 * token bị nuốt im lặng còn tệ hơn: người dùng mất một đoạn câu trả lời mà không có dấu
 * hiệu nào.
 */
function RawText(props: { token: Token }) {
  const token = props.token as Tokens.Generic;
  return <span class="whitespace-pre-wrap">{token.raw ?? ""}</span>;
}

/** Chỉ ba lược đồ này. Xem `LinkOut`. */
const SCHEMES = new Set(["http:", "https:", "mailto:"]);

/**
 * Liên kết trong câu trả lời.
 *
 * Hai chốt, cả hai đều về cùng một chuyện — cửa sổ này không được điều hướng đi đâu cả:
 *
 * 1. `target="_blank"` đưa liên kết ra trình duyệt ngoài. Một cú điều hướng trong cửa sổ
 *    Tauri thay luôn cả ứng dụng bằng trang web đó, và người dùng mất cả phiên làm việc.
 *    (`@tauri-apps/plugin-opener` sẽ đúng hơn — nó mở bằng trình duyệt mặc định của hệ
 *    điều hành — nhưng chưa được cài, và đợt này không thêm phụ thuộc.)
 * 2. Lược đồ phải nằm trong danh sách trắng, và **URL phải tuyệt đối**. `javascript:` là
 *    lý do hiển nhiên; đường dẫn tương đối là lý do kín hơn — nó trỏ vào chính origin của
 *    ứng dụng, nên nó *là* một cú điều hướng cửa sổ. Không qua được thì hiện thành chữ:
 *    người đọc vẫn thấy đủ nội dung, chỉ là không bấm được.
 */
function LinkOut(props: { href: string; title?: string; tokens: Token[] }) {
  const href = createMemo(() => {
    try {
      const url = new URL(props.href);
      return SCHEMES.has(url.protocol) ? url.href : null;
    } catch {
      return null;
    }
  });

  return (
    <Show when={href()} fallback={<InlineSeq tokens={props.tokens} />}>
      {(safe) => (
        <a
          href={safe()}
          title={props.title}
          target="_blank"
          rel="noreferrer noopener"
          class="text-accent-ink underline decoration-line-strong underline-offset-2 hover:decoration-current"
        >
          <InlineSeq tokens={props.tokens} />
        </a>
      )}
    </Show>
  );
}
