/* @refresh reload */
import { render } from "solid-js/web";
import "./styles/app.css";
import App from "./App";
import { initTheme } from "./lib/theme";

// Đóng dấu theme trước lần vẽ đầu tiên. Làm sau khi render là một nháy màu sai —
// ngắn, nhưng ai cũng thấy.
initTheme();

const root = document.getElementById("root");
if (!root) throw new Error("thiếu #root trong index.html");
render(() => <App />, root);
