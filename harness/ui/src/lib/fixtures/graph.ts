import type { GraphEdge, GraphNode, GraphView, IndexStats } from "../protocol";
import type { GraphDirection } from "../graph";

/**
 * Dữ liệu mẫu cho màn hình đồ thị và cho bộ dựng khối, ở chế độ `?demo=1`.
 *
 * Tên ký hiệu ở đây được chọn **vì chúng khó**, không vì chúng đẹp. Một đồ thị mẫu toàn
 * `foo`, `bar`, `handle_request` chứng minh được đúng một điều: bộ thoát chưa từng gặp
 * một cái tên thật. Tên thật trong Rust và TypeScript mang generic, ngoặc nhọn, dấu hai
 * chấm kép, khoảng trắng và cả `impl … for …` — và mỗi thứ đó là một cách khác nhau để
 * làm hỏng cú pháp mermaid.
 */

const RETRY = "crates/pai-providers/src/retry.rs";
const OPENAI = "crates/pai-providers/src/openai.rs";
const DRIVER = "crates/pai-agent/src/driver.rs";
const BOOTSTRAP = "scripts/bootstrap.py";

const NODES: GraphNode[] = [
  { id: "sym:retry", name: "retry_with_backoff", kind: "function", path: RETRY, line: 31 },
  { id: "sym:policy", name: "RetryPolicy<Backoff>", kind: "struct", path: RETRY, line: 12 },
  { id: "sym:should", name: "should_retry", kind: "function", path: RETRY, line: 88 },
  {
    id: "sym:pred",
    name: "Box<dyn Fn(&StatusCode) -> bool + Send>",
    kind: "type",
    path: RETRY,
    line: 19,
  },
  { id: "sym:driver", name: "Driver", kind: "trait", path: DRIVER, line: 24 },
  { id: "sym:impl", name: "impl Driver for OpenAiDriver", kind: "type", path: OPENAI, line: 96 },
  { id: "sym:stream", name: "OpenAiDriver::stream_turn", kind: "method", path: OPENAI, line: 118 },
  {
    id: "sym:insert",
    name: "HashMap<String, Vec<Token>>::insert",
    kind: "method",
    path: OPENAI,
    line: 203,
  },
  { id: "sym:queue", name: "Vec<PendingTurn>", kind: "type", path: DRIVER, line: 57 },
  { id: "sym:load", name: "load_config", kind: "function", path: BOOTSTRAP, line: 74 },
  { id: "sym:paths", name: "config_paths[env]", kind: "constant", path: BOOTSTRAP, line: 21 },
];

const EDGES: GraphEdge[] = [
  { src: "sym:stream", dst: "sym:retry", kind: "calls" },
  { src: "sym:retry", dst: "sym:should", kind: "calls" },
  { src: "sym:retry", dst: "sym:policy", kind: "references" },
  { src: "sym:policy", dst: "sym:pred", kind: "contains" },
  { src: "sym:impl", dst: "sym:driver", kind: "implements" },
  { src: "sym:impl", dst: "sym:stream", kind: "contains" },
  { src: "sym:stream", dst: "sym:insert", kind: "calls" },
  { src: "sym:stream", dst: "sym:queue", kind: "references" },
  { src: "sym:load", dst: "sym:paths", kind: "references" },
  { src: "sym:load", dst: "sym:retry", kind: "imports" },
];

const byId = new Map(NODES.map((node) => [node.id, node]));

/** Khớp theo tên như lõi làm: chứa chuỗi, không phân biệt hoa thường. */
function resolve(symbol: string): GraphNode | undefined {
  const needle = symbol.trim().toLowerCase();
  if (needle === "") return undefined;
  return (
    byId.get(symbol) ??
    NODES.find((node) => node.name.toLowerCase() === needle) ??
    NODES.find((node) => node.name.toLowerCase().includes(needle))
  );
}

/**
 * Lân cận giả lập.
 *
 * `sym:stream` cố tình trả `truncated: true`: đó là đỉnh đông cạnh nhất trong bộ mẫu, và
 * nếu không có một đường đi nào chạm vào cờ đó thì băng "đã cắt bớt" là thứ chưa ai
 * nhìn thấy bao giờ.
 */
export function demoGraphView(symbol: string, direction: GraphDirection, depth: number): GraphView {
  const focus = resolve(symbol);
  if (focus === undefined) return { nodes: [], edges: [], truncated: false };

  const keep = new Set<string>([focus.id]);
  let frontier = [focus.id];
  for (let step = 0; step < Math.max(1, depth); step += 1) {
    const next: string[] = [];
    for (const edge of EDGES) {
      const outward = direction !== "callers" && frontier.includes(edge.src);
      const inward = direction !== "callees" && frontier.includes(edge.dst);
      if (outward && !keep.has(edge.dst)) {
        keep.add(edge.dst);
        next.push(edge.dst);
      }
      if (inward && !keep.has(edge.src)) {
        keep.add(edge.src);
        next.push(edge.src);
      }
    }
    frontier = next;
  }

  return {
    nodes: NODES.filter((node) => keep.has(node.id)),
    edges: EDGES.filter((edge) => keep.has(edge.src) && keep.has(edge.dst)),
    truncated: focus.id === "sym:stream" || depth >= 3,
  };
}

/**
 * Chỉ mục đang quét dở: có tệp, có ký hiệu, **chưa có cạnh nào**.
 *
 * Đây là hình dạng thật của một lần quét chưa xong — cạnh được dựng ở lượt sau cùng — và
 * cũng là lúc màn hình dễ nói dối nhất: hiện "0 cạnh" như một sự thật thay vì nói ra
 * rằng nó chưa đếm xong.
 */
export function demoIndexStats(): IndexStats {
  return {
    files: 214,
    symbols: 1842,
    edges: 0,
    languages: [
      ["rust", 168],
      ["typescript", 39],
      ["python", 7],
    ],
    scannedAt: null,
  };
}

/**
 * Một tin nhắn trợ lý đi qua **cả bốn** đường của bộ dựng khối.
 *
 * Khối mermaid chưa đóng rào nằm ở cuối, đúng chỗ nó xuất hiện lúc chữ đang chảy. Không
 * có nó trong dữ liệu mẫu thì đường đi quan trọng nhất của `Blocks.tsx` — đường không
 * được gọi `mermaid.render` — là đường chưa ai nhìn thấy bao giờ.
 */
export function demoDiagramText(): string {
  return [
    "Đường đi của một lượt gọi provider, từ lúc agent đẩy lượt vào hàng đợi:",
    "",
    "```mermaid",
    "flowchart LR",
    '  q(["Vec#60;PendingTurn#62;"]) --> d{{"Driver"}}',
    '  d --> s(["OpenAiDriver::stream_turn"])',
    '  s -->|gọi| r(["retry_with_backoff"])',
    '  r -.->|tham chiếu| p["RetryPolicy#60;Backoff#62;"]',
    "```",
    "",
    "Chính sách thử lại đọc từ tệp cấu hình:",
    "",
    "```rust",
    "let policy = RetryPolicy::<Backoff>::from_env()",
    '    .with_predicate(Box::new(|code: &StatusCode| code.as_u16() == 429));',
    "```",
    "",
    "Còn đây là một sơ đồ tôi viết sai cú pháp — mermaid sẽ từ chối nó:",
    "",
    "```mermaid",
    "flowchart LR",
    "  a --> ",
    "  --> b]]",
    "```",
    "",
    "Và phần tôi đang vẽ dở:",
    "",
    "```mermaid",
    "sequenceDiagram",
    "  participant A as Agent",
    "  A->>D: stream_turn",
  ].join("\n");
}
