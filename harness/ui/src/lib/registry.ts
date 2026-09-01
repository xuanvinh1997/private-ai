import type { Component } from "solid-js";
import type { ConversationNode, NodeKind, ToolCall } from "./protocol";

/**
 * Sổ đăng ký renderer — bản thu nhỏ của "everything is a plugin" ở tầng giao diện.
 *
 * Lý do không dùng `switch` trong `Transcript`: một `switch` bắt mọi loại nội dung mới
 * phải sửa đúng một tệp, và tệp đó dần biết hết về mọi tính năng. Với sổ đăng ký, thứ
 * duy nhất `Transcript` biết là "có một khoá, tra ra một component". Thêm tool mới =
 * thêm một lần gọi `registerToolCard`, không ai phải mở lại `Transcript`.
 *
 * Hai sổ, hai không gian khoá khác nhau — đúng như dsh:
 *   - node registry, khoá theo `ConversationNode.kind`
 *   - tool card registry, khoá theo **tên tool trên wire** (`read`, `edit`, `bash`…)
 */

export type NodeProps<K extends NodeKind = NodeKind> = {
  node: Extract<ConversationNode, { kind: K }>;
};

const nodeRenderers = new Map<string, Component<NodeProps>>();

export function registerNode<K extends NodeKind>(kind: K, render: Component<NodeProps<K>>): void {
  // Trùng khoá là lỗi lập trình, không phải trường hợp biên: hai renderer cho cùng một
  // loại node thì cái nào thắng là chuyện của thứ tự import — thứ không ai đọc được.
  if (nodeRenderers.has(kind)) throw new Error(`đã có renderer cho node "${kind}"`);
  // Ép kiểu là cần thiết và an toàn ở đúng một điều kiện: khoá `kind` và kiểu node
  // được buộc với nhau ở chữ ký hàm này. Sổ đăng ký cất kiểu rộng, chỗ gọi nhận kiểu
  // hẹp — TypeScript không biểu diễn được ràng buộc đó nếu không có một lần ép.
  nodeRenderers.set(kind, render as unknown as Component<NodeProps>);
}

export function nodeRenderer(kind: string): Component<NodeProps> | undefined {
  return nodeRenderers.get(kind);
}

export type ToolCardProps = { call: ToolCall };

const toolRenderers = new Map<string, Component<ToolCardProps>>();
let toolFallback: Component<ToolCardProps> | undefined;

export function registerToolCard(name: string, render: Component<ToolCardProps>): void {
  if (toolRenderers.has(name)) throw new Error(`đã có thẻ cho tool "${name}"`);
  toolRenderers.set(name, render);
}

/** Thẻ dùng khi tool chưa có renderer riêng. Đăng ký sau sẽ đè cái trước — cố ý. */
export function registerToolFallback(render: Component<ToolCardProps>): void {
  toolFallback = render;
}

/**
 * Tra thẻ cho một tool. Không gian khoá là **mở**: tên lạ thì rơi vào fallback chứ
 * không nổ. Một tool mới xuất hiện từ MCP vẫn hiện được, chỉ là hiện thô.
 */
export function toolCard(name: string): Component<ToolCardProps> | undefined {
  return toolRenderers.get(name) ?? toolFallback;
}


/**
 * Dọn sổ trước khi module đăng ký chạy lại.
 *
 * Hot reload nạp lại tệp đăng ký nhưng KHÔNG nạp lại tệp này, nên lần chạy thứ hai gặp
 * toàn khoá trùng và ném — biến một lỗi thật thành tiếng ồn mỗi lần lưu tệp. Hai hàm
 * dưới đây chỉ được gọi từ `import.meta.hot.dispose`; ở bản dựng thật chúng không có
 * chỗ gọi nào.
 */
export function clearNodeRegistry(): void {
  nodeRenderers.clear();
}

export function clearToolRegistry(): void {
  toolRenderers.clear();
  toolFallback = undefined;
}
