import type { NodeProps } from "../../lib/registry";
import { useTranscriptActions } from "../../lib/transcriptActions";
import Blocks from "../markdown/Blocks";
import MessageShell, { type MessageAction } from "../MessageShell";

/**
 * Giờ đến của một tin nhắn.
 *
 * Bản ghi nạp lại mang giờ **trong sổ**; lượt đang chạy thì không có gì để mang, nên nó
 * lấy lúc node xuất hiện. Hai nguồn cho hai tình huống, và cả hai đều đúng với tình
 * huống của mình. Chốt ở thân component nên nó cố định theo node, không nhảy theo mỗi
 * lần vẽ lại.
 */
const arrivedAt = (at?: number) => at ?? Date.now();

async function copy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch (err) {
    console.error("không chép được", err);
  }
}

export function UserMessage(props: NodeProps<"user">) {
  const at = arrivedAt(props.node.at);
  const actions = useTranscriptActions();

  const list = (): MessageAction[] => [
    {
      id: "copy",
      label: "Chép tin nhắn",
      icon: "copy",
      onSelect: () => void copy(props.node.text),
    },
    ...(actions.resend
      ? [
          {
            id: "retry",
            label: "Gửi lại",
            icon: "retry" as const,
            onSelect: () => actions.resend?.(props.node.text),
          },
        ]
      : []),
    {
      id: "delete",
      label: "Xoá khỏi bản ghi",
      icon: "trash",
      danger: true,
      onSelect: () => actions.remove(props.node.id),
    },
  ];

  return (
    <MessageShell role="user" name="Bạn" at={at} actions={list()}>
      <div class="text-base whitespace-pre-wrap">{props.node.text}</div>
    </MessageShell>
  );
}

/**
 * Tin nhắn trợ lý.
 *
 * `aria-live` phải có mặt **trước** khi chữ bắt đầu chảy vào, nếu không trình đọc màn
 * hình bỏ qua lần thay đổi đầu — nên khối này được tạo rỗng ngay từ token đầu tiên chứ
 * không đợi có nội dung. `polite` chứ không `assertive`: câu trả lời không được cắt
 * ngang những gì người dùng đang nghe.
 */
export function AssistantMessage(props: NodeProps<"assistant">) {
  const at = arrivedAt(props.node.at);
  const actions = useTranscriptActions();

  const list = (): MessageAction[] =>
    props.node.streaming
      ? []
      : [
          {
            id: "copy",
            label: "Chép câu trả lời",
            icon: "copy",
            onSelect: () => void copy(props.node.text),
          },
          {
            id: "delete",
            label: "Xoá khỏi bản ghi",
            icon: "trash",
            danger: true,
            onSelect: () => actions.remove(props.node.id),
          },
        ];

  return (
    <MessageShell
      role="assistant"
      name="Trợ lý"
      at={at}
      live={props.node.streaming}
      busy={props.node.streaming}
      actions={list()}
    >
      {/* Chữ trợ lý đi qua bộ dựng khối: khối rào ```mermaid thành hình, khối rào khác
          thành khối mã có nhãn, phần còn lại vẫn là chữ `whitespace-pre-wrap` như cũ.
          Con trỏ nhấp nháy chuyển vào `Blocks` vì chỗ đặt nó phụ thuộc vào khối cuối
          cùng đang là chữ hay đang là một khối mã chưa đóng rào. */}
      <Blocks text={props.node.text} streaming={props.node.streaming} />
    </MessageShell>
  );
}
