import { clearNodeRegistry, registerNode } from "../../lib/registry";
import TodoCard from "../TodoCard";
import { AssistantMessage, UserMessage } from "./MessageNodes";
import { ErrorNode, NoticeNode, ProgressNode } from "./StatusNodes";
import { ToolNode } from "./ToolNode";

// Đăng ký mọi thứ hiện được trong bản ghi hội thoại. Import tệp này một lần ở chỗ khởi
// động là đủ — không component nào phải import lẫn nhau, nên thêm loại node mới không
// kéo theo sửa đổi ở `Transcript`.
import "../tools";

registerNode("user", UserMessage);
registerNode("assistant", AssistantMessage);
registerNode("tool", ToolNode);
registerNode("notice", NoticeNode);
registerNode("progress", ProgressNode);
registerNode("error", ErrorNode);
// Danh sách việc thụt vào bằng đúng bề rộng avatar: nó là thứ trợ lý *làm*, nên nó
// đứng cùng trục dọc với thẻ tool chứ không cùng trục với avatar.
registerNode("todo", (props) => (
  <div class="ml-[calc(var(--avatar)+var(--sp-md))]">
    <TodoCard items={props.node.items} />
  </div>
));

// Xem `clearNodeRegistry`: hot reload nạp lại tệp này nhưng giữ nguyên sổ đăng ký.
if (import.meta.hot) import.meta.hot.dispose(() => clearNodeRegistry());
