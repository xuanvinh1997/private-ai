import { clearToolRegistry, registerToolCard, registerToolFallback } from "../../lib/registry";
import BashCard from "./BashCard";
import MutationCard from "./MutationCard";
import ReadCard from "./ReadCard";
import { GlobCard, GrepCard } from "./SearchCard";
import GenericToolCard from "./ToolCard";
import TodoToolCard from "./TodoToolCard";

/**
 * Điểm mở rộng của tầng tool: thêm một tool mới nghĩa là thêm đúng một dòng ở đây.
 *
 * Khoá là **tên tool trên wire**, không phải nhãn hiển thị — nhãn đổi được, tên trên
 * wire là hợp đồng với lõi. Không có khoá thì rơi vào `GenericToolCard`, nên một tool
 * đến từ MCP vẫn hiện được ngay cả khi không ai từng nghe tên nó.
 */
registerToolFallback(GenericToolCard);

registerToolCard("read", ReadCard);
registerToolCard("write", MutationCard);
registerToolCard("edit", MutationCard);
registerToolCard("grep", GrepCard);
registerToolCard("glob", GlobCard);
registerToolCard("bash", BashCard);
registerToolCard("todo_write", TodoToolCard);

// Xem `clearToolRegistry`: hot reload nạp lại tệp này nhưng giữ nguyên sổ đăng ký.
if (import.meta.hot) import.meta.hot.dispose(() => clearToolRegistry());
