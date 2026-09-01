import { Key } from "@solid-primitives/keyed";
import { createMemo, Match, Show, Switch } from "solid-js";
import CodeFence from "./CodeFence";
import Diagram from "./Diagram";
import { splitFences, type Segment } from "./fences";
import Markdown from "./Markdown";

/**
 * Thân một tin nhắn trợ lý: markdown, khối mã, và sơ đồ mermaid.
 *
 * ## Markdown và bẫy chảy chữ
 *
 * Một đoạn chữ chỉ được dựng thành markdown khi nó **đã chốt** — tức là khi lượt đã xong,
 * hoặc khi phía sau nó còn đoạn khác (một khối rào đã mở sau nó thì nó không dài thêm được
 * nữa). Đoạn cuối cùng của một lượt đang chảy hiện nguyên văn, `whitespace-pre-wrap`, đúng
 * như trước.
 *
 * Hai lý do, và lý do thứ hai mới là lý do thật:
 *
 * 1. Markdown giữa chừng **luôn** dở dang. `**Đọc` chưa đóng là chữ thường; nửa giây sau
 *    nó thành chữ đậm. Một hàng bảng mới có nửa là một đoạn văn; hàng sau nó biến cả cụm
 *    thành bảng và mọi thứ bên dưới nhảy xuống. Người đọc đang đọc dở đúng chỗ đó.
 * 2. Vẽ lại cả cây token ở mỗi token đến là chạy `lexer` trên toàn bộ tin nhắn, sáu chục
 *    lần mỗi giây — công tỉ lệ với *độ dài tin nhắn nhân số token*. Câu trả lời càng dài
 *    thì càng chậm, đúng lúc nó cần mượt nhất.
 *
 * Cái mất là một lần "chốt" ở cuối lượt, khi chữ thường biến thành markdown. Một lần nhảy
 * ở cuối rẻ hơn một lần nhảy mỗi token, và trong lúc chảy thì chữ vẫn đọc được nguyên vẹn:
 * đó đúng là những ký tự mô hình đã gửi, đúng thứ tự, giữ nguyên xuống dòng.
 *
 * ## Bẫy lớn nhất của tệp này: chữ đang chảy
 *
 * Trong lúc trợ lý gõ, mỗi token làm `text` đổi một lần, và một khối mermaid đang được
 * gõ dở chỉ mới có nửa cú pháp — `flowchart TD` rồi `A -->` rồi `A --> B[Đọ`. Nếu ta cứ
 * đưa cái đó cho mermaid thì nó ném lỗi vài chục lần mỗi giây và người dùng nhìn thấy
 * một ô đỏ nhấp nháy ở chỗ đáng lẽ là một sơ đồ.
 *
 * Nên luật là: **rào chưa đóng thì chưa phải sơ đồ.** Khối chưa đóng hiện dưới dạng mã
 * đang chảy — vẫn đọc được, vẫn thấy trợ lý đang làm gì — và chỉ khi rào đóng lại nó mới
 * biến thành hình. `Segment.closed` mang đúng thông tin đó từ bộ tách rào lên.
 *
 * ## Vì sao `<Key>` chứ không `<For>`
 *
 * Mỗi token làm bộ tách chạy lại và sinh ra một mảng đối tượng hoàn toàn mới. `<For>`
 * so sánh theo tham chiếu nên nó sẽ dựng lại **tất cả** — kể cả một sơ đồ đã vẽ xong ở
 * đầu tin nhắn, mỗi token một lần. `<Key>` khoá theo vị trí và loại, nên khối đã xong
 * giữ nguyên DOM và chỉ khối cuối cùng nhận nội dung mới.
 */
export default function Blocks(props: { text: string; streaming?: boolean }) {
  const rows = createMemo(() => {
    const segments = splitFences(props.text);
    return segments.map((seg, index) => ({
      key: `${index}:${seg.kind}:${seg.kind === "fence" ? seg.lang : ""}`,
      seg,
      last: index === segments.length - 1,
    }));
  });

  return (
    <div class="flex flex-col gap-sm text-base text-text">
      <Key each={rows()} by="key">
        {(row) => (
          <Switch>
            <Match when={row().seg.kind === "text"}>
              {/* Đoạn đang chảy đi đường chữ thô; đoạn đã chốt đi đường markdown. Con trỏ
                  chỉ bám vào đoạn cuối — dán nó vào giữa bản ghi thì nó không còn nghĩa
                  "đang gõ tiếp ở đây" — nên nó nằm gọn trong nhánh chưa chốt. */}
              <Show
                when={props.streaming === true && row().last}
                fallback={<Markdown text={text(row().seg)} />}
              >
                <div class="whitespace-pre-wrap">
                  {text(row().seg)}
                  <Caret />
                </div>
              </Show>
            </Match>

            <Match when={row().seg.kind === "fence" && isDiagram(row().seg)}>
              <Diagram source={code(row().seg)} />
            </Match>

            <Match when={row().seg.kind === "fence"}>
              <CodeFence
                lang={lang(row().seg)}
                code={code(row().seg)}
                streaming={!closed(row().seg)}
              />
            </Match>
          </Switch>
        )}
      </Key>

      {/* Tin nhắn mở đầu bằng một khối mã thì chưa có đoạn chữ nào để gắn con trỏ. */}
      <Show when={props.streaming === true && rows().at(-1)?.seg.kind !== "text"}>
        <Caret />
      </Show>
    </div>
  );
}

function Caret() {
  return (
    <span
      class="ml-3xs inline-block h-3.5 w-[2px] translate-y-[2px] bg-accent motion-safe:animate-pulse"
      aria-hidden="true"
    />
  );
}

/** Chỉ khối mermaid **đã đóng rào** mới được dựng thành hình. Xem ghi chú đầu tệp. */
const isDiagram = (seg: Segment): boolean =>
  seg.kind === "fence" && seg.lang === "mermaid" && seg.closed;

const text = (seg: Segment): string => (seg.kind === "text" ? seg.text : "");
const code = (seg: Segment): string => (seg.kind === "fence" ? seg.code : "");
const lang = (seg: Segment): string => (seg.kind === "fence" ? seg.lang : "");
const closed = (seg: Segment): boolean => seg.kind !== "fence" || seg.closed;
