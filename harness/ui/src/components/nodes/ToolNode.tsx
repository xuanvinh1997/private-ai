import { Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { toolCard } from "../../lib/registry";
import type { NodeProps } from "../../lib/registry";

/**
 * Cầu nối giữa hai sổ đăng ký: node `tool` tra tiếp sang sổ thẻ tool theo **tên tool**.
 *
 * Tách hai tầng như vậy để `Transcript` không bao giờ phải biết tool nào tồn tại — nó
 * chỉ biết "có một node kind là tool". Tầng thứ hai mới là nơi tên tool có nghĩa.
 */
export function ToolNode(props: NodeProps<"tool">) {
  const card = () => toolCard(props.node.call.name);
  return (
    <Show when={card()}>
      {(component) => <Dynamic component={component()} call={props.node.call} />}
    </Show>
  );
}
