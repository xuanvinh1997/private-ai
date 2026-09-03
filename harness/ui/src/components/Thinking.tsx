import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import type { ConversationNode } from "../lib/protocol";
import Icon from "./Icon";
import { toolLabel } from "./tools/ToolCard";

/**
 * Chỉ báo "trợ lý đang làm việc".
 *
 * Nó lấp đúng khoảng lặng mà bản ghi không có gì để vẽ: từ lúc câu hỏi rời đi cho tới
 * token đầu tiên, và mọi khoảng giữa hai bước sau đó. Khoảng ấy dài nhất đúng ở chỗ mô
 * hình chạy tại chỗ — vài giây nạp trọng số trên một màn hình đứng im, và người dùng
 * kết luận là bấm hụt rồi gửi lại.
 *
 * Nó **tự tắt khi chữ bắt đầu chảy**: khối trợ lý đang stream đã có con trỏ nhấp nháy
 * của riêng nó, và hai chỉ báo cùng nói một điều thì cái thứ hai chỉ là nhiễu.
 *
 * Nhãn đi theo việc đang làm chứ không phải một câu cố định: "đang chạy lệnh" trả lời
 * được câu hỏi *chờ cái gì*, còn "đang suy nghĩ" trong lúc một tool chạy hai mươi giây
 * thì nói sai.
 */
export default function Thinking(props: { nodes: ConversationNode[]; busy: boolean }) {
  const last = () => props.nodes[props.nodes.length - 1];

  const show = () => {
    if (!props.busy) return false;
    const node = last();
    return !(node?.kind === "assistant" && node.streaming);
  };

  /** Tool đang chạy gần nhất — quét ngược chứ không chỉ nhìn node cuối: danh sách việc
   *  và thông báo chen vào *sau* một `tool_start` là chuyện thường, và nhìn mỗi node cuối
   *  thì nhãn tụt về "đang suy nghĩ" giữa lúc một lệnh vẫn đang chạy. */
  const running = () => {
    for (let i = props.nodes.length - 1; i >= 0; i--) {
      const node = props.nodes[i]!;
      if (node.kind === "tool" && node.call.state === "running") return node.call.name;
    }
    return null;
  };

  const label = () => {
    const name = running();
    if (name !== null) {
      const pretty = toolLabel(name);
      // Tool có nhãn tiếng Việt thì ghép thành câu; tên lạ (thường là tool từ MCP) giữ
      // nguyên dạng gốc — hạ chữ hoa một chuỗi như `mcp__jira__search` chỉ làm nó khó đọc.
      return pretty === name ? `Đang chạy ${name}` : `Đang ${pretty.toLowerCase()}`;
    }
    const node = last();
    if (node?.kind === "progress") return node.label;
    return "Đang suy nghĩ";
  };

  // Đồng hồ đếm từ lúc lượt bắt đầu, không phải từ lúc đổi pha: người dùng muốn biết
  // *đã chờ bao lâu*, và một con số nhảy về 0 sau mỗi tool trả lời sai câu hỏi đó.
  const [secs, setSecs] = createSignal(0);
  createEffect(() => {
    if (!props.busy) {
      setSecs(0);
      return;
    }
    const start = Date.now();
    setSecs(0);
    const timer = setInterval(() => setSecs(Math.floor((Date.now() - start) / 1000)), 1000);
    onCleanup(() => clearInterval(timer));
  });

  return (
    <Show when={show()}>
      {/* `role="status"` + `aria-live="polite"`: trình đọc màn hình nói một lần khi nhãn
          đổi, và không cắt ngang thứ người dùng đang nghe. Ba chấm là trang trí thuần
          nên chúng ẩn khỏi cây trợ năng. */}
      <div class="flex gap-md" role="status" aria-live="polite">
        <div
          aria-hidden="true"
          class="mt-3xs grid size-(--avatar) shrink-0 place-items-center rounded-pill bg-surface-hover text-accent-ink"
        >
          <Icon name="sparkle" size={15} />
        </div>

        <div class="flex min-w-0 items-center gap-sm">
          <span class="min-w-0 truncate text-sm text-muted">{label()}</span>
          <span class="pai-dots flex shrink-0 items-center gap-3xs" aria-hidden="true">
            <span />
            <span />
            <span />
          </span>
          {/* Con số chỉ xuất hiện khi chờ đã đủ lâu để thành một câu hỏi. Hiện nó ngay từ
              giây đầu là biến mọi câu trả lời nhanh thành một cái đồng hồ nháy. */}
          <Show when={secs() >= 3}>
            <span class="shrink-0 text-2xs text-faint tabular-nums">{secs()}s</span>
          </Show>
        </div>
      </div>
    </Show>
  );
}
