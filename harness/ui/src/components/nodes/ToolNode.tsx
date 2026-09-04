import { Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { toolCard } from "../../lib/registry";
import type { NodeProps } from "../../lib/registry";

/** Bridge between the two registries, so `Transcript` never needs to know which tools exist. */
export function ToolNode(props: NodeProps<"tool">) {
  const card = () => toolCard(props.node.call.name);
  return (
    <Show when={card()}>
      {(component) => <Dynamic component={component()} call={props.node.call} />}
    </Show>
  );
}
