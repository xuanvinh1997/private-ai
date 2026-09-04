import { clearNodeRegistry, registerNode } from "../../lib/registry";
import TodoCard from "../TodoCard";
import { AssistantMessage, UserMessage } from "./MessageNodes";
import { ErrorNode, NoticeNode, ProgressNode } from "./StatusNodes";
import { ToolNode } from "./ToolNode";

// Register everything the transcript can draw; importing this file once at startup is enough.
import "../tools";

registerNode("user", UserMessage);
registerNode("assistant", AssistantMessage);
registerNode("tool", ToolNode);
registerNode("notice", NoticeNode);
registerNode("progress", ProgressNode);
registerNode("error", ErrorNode);
// The todo list is indented onto the tool-card axis, since it is work rather than speech.
registerNode("todo", (props) => (
  <div class="ml-[calc(var(--avatar)+var(--sp-md))]">
    <TodoCard items={props.node.items} />
  </div>
));

// See `clearNodeRegistry`: hot reload re-runs this file but keeps the registry.
if (import.meta.hot) import.meta.hot.dispose(() => clearNodeRegistry());
