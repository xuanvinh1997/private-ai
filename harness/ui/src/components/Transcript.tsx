import { Key } from "@solid-primitives/keyed";
import { createSignal, onCleanup, onMount, Show, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";
import { displayMode } from "../lib/prefs";
import type { ConversationNode } from "../lib/protocol";
import { nodeRenderer } from "../lib/registry";
import Icon from "./Icon";

/** Ngưỡng bám đáy, lấy đúng số của chat_view.py:1226. */
const STICK_PX = 80;

/**
 * Bản ghi hội thoại.
 *
 * Tệp này cố ý **không biết** có những loại nội dung nào. Nó tra sổ đăng ký theo `kind`
 * rồi dựng component tương ứng; thêm thẻ tool mới hay loại thông báo mới không đụng tới
 * đây. Đó là toàn bộ lý do sổ đăng ký tồn tại.
 *
 * Việc bám đáy đo bằng `ResizeObserver` chứ không bằng một effect nghe cả mảng: nghe cả
 * mảng thì mỗi token là một lần đọc `scrollHeight`, tức một lần buộc layout chạy lại
 * giữa lúc đang stream — đúng thứ làm giao diện giật. `ResizeObserver` chỉ nổ khi chiều
 * cao thật sự đổi, và nổ *sau* layout nên số đo đã đúng.
 */
export default function Transcript(props: { nodes: ConversationNode[]; empty?: JSX.Element }) {
  let scroller: HTMLDivElement | undefined;
  let content: HTMLDivElement | undefined;

  // Người dùng cuộn lên đọc lại thì thôi bám đáy. Cuộn ép là cách nhanh nhất làm người
  // ta mất chỗ đang đọc, và họ không có cách nào đòi lại.
  let stuck = true;
  const [atBottom, setAtBottom] = createSignal(true);

  const toBottom = (smooth: boolean) => {
    stuck = true;
    setAtBottom(true);
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  };

  onMount(() => {
    const el = scroller;
    const body = content;
    if (!el || !body) return;

    const onScroll = () => {
      stuck = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_PX;
      setAtBottom(stuck);
    };
    el.addEventListener("scroll", onScroll, { passive: true });

    const observer = new ResizeObserver(() => {
      if (stuck) el.scrollTop = el.scrollHeight;
    });
    observer.observe(body);

    onCleanup(() => {
      el.removeEventListener("scroll", onScroll);
      observer.disconnect();
    });
  });

  return (
    <div class="relative min-h-0 flex-1">
      <div
        ref={scroller}
        class="h-full overflow-y-auto px-(--page-pad-x)"
        // Trình duyệt tự neo cuộn khi nội dung phía trên đổi kích thước; giữa lúc stream
        // nó đánh nhau với logic bám đáy ở trên và kết quả là màn hình rung.
        style={{ "overflow-anchor": "none" }}
      >
        <div
          ref={content}
          class="mx-auto flex flex-col gap-lg py-lg"
          // Chế độ tài liệu bỏ giới hạn bề rộng đọc: nó tồn tại để diff và đầu ra lệnh
          // có chỗ thở, mà cắt nó xuống 720px thì đúng thứ đó bị bóp lại đầu tiên.
          classList={{
            "max-w-(--reading-measure)": displayMode() === "bubble",
            "max-w-[min(100%,980px)]": displayMode() === "document",
          }}
        >
          <Show when={props.nodes.length > 0} fallback={props.empty}>
            {/* Keyed theo `id`: một node giữ nguyên DOM của nó kể cả khi danh sách được nạp
                thêm ở đầu (phân trang ngược). Keyed theo vị trí thì cả bản ghi remount. */}
            <Key each={props.nodes} by="id">
              {(node) => <NodeSeat node={node()} />}
            </Key>
          </Show>
        </div>
      </div>

      <BackBottom visible={!atBottom()} onClick={() => toBottom(true)} />
    </div>
  );
}

/**
 * Nút về đáy.
 *
 * Luôn nằm trong cây DOM và chỉ đổi độ mờ với vị trí: gắn/gỡ nó theo trạng thái cuộn sẽ
 * làm tiêu điểm bàn phím rơi mất giữa chừng nếu người dùng đang đứng trên nó. Khi ẩn thì
 * `pointer-events: none` để nó không nuốt cú bấm vào bản ghi phía dưới, và `tabIndex=-1`
 * để Tab không dừng ở một cái nút vô hình.
 */
function BackBottom(props: { visible: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      tabIndex={props.visible ? 0 : -1}
      aria-hidden={!props.visible}
      class="absolute right-lg bottom-lg z-20 flex items-center gap-2xs rounded-pill border border-line bg-[var(--glass)] px-md py-2xs text-2xs text-text shadow-float backdrop-blur transition-all duration-[var(--dur-base)] ease-[var(--ease-out)] hover:bg-surface"
      classList={{
        "pointer-events-none translate-y-2 opacity-0": !props.visible,
        "translate-y-0 opacity-100": props.visible,
      }}
    >
      <Icon name="arrow-down" size={13} />
      Về cuối
    </button>
  );
}

function NodeSeat(props: { node: ConversationNode }) {
  const render = () => nodeRenderer(props.node.kind);
  return (
    // Id trên chỗ ngồi chứ không trong renderer: bảng "tệp đã thay đổi" cần cuộn tới một
    // node bất kỳ, và nó không được phép biết node đó do component nào vẽ.
    <div id={`node-${props.node.id}`} class="scroll-mt-lg">
      <Show when={render()} fallback={<UnknownNode kind={props.node.kind} />}>
        {(component) => <Dynamic component={component()} node={props.node} />}
      </Show>
    </div>
  );
}

/**
 * Không gian khoá là mở, nên "không có renderer" là trạng thái hợp lệ chứ không phải
 * lỗi. Hiện một dòng xám còn hơn nuốt mất một sự kiện mà không ai biết.
 */
function UnknownNode(props: { kind: string }) {
  return (
    <p class="m-0 px-sm text-2xs text-faint">
      (chưa có cách hiển thị cho <code class="font-mono">{props.kind}</code>)
    </p>
  );
}
