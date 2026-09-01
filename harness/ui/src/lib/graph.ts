import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import { isDemo } from "./demo";
import { demoGraphView, demoIndexStats } from "./fixtures/graph";
import type { GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind, GraphView, IndexStats } from "./protocol";

/**
 * Ba lệnh của đồ thị mã nguồn, chia hai nhóm theo cùng ranh giới với `projects.ts`:
 *
 *   - `indexStats` chạy lúc mở màn hình, **nuốt lỗi** và trả một bản thống kê rỗng.
 *     Chỉ mục chưa dựng cũng ra đúng hình dạng đó, nên màn hình chỉ cần biết đọc
 *     `scannedAt === null` là "chưa có gì để xem" — không cần phân biệt "hỏng" với
 *     "chưa chạy", vì với người dùng hai thứ đó dẫn tới cùng một hành động.
 *   - `graphNeighborhood` và `graphTrace` chạy sau một cú bấm và **ném ra ngoài**. Người
 *     dùng vừa chọn một ký hiệu và đang đứng chờ; im lặng ở đó không phân biệt được với
 *     "đang chậm", và họ sẽ bấm lại.
 */

/** Chưa quét lần nào trông y hệt đọc không được — và cả hai đều là "chưa có gì để xem". */
const NO_INDEX: IndexStats = {
  files: 0,
  symbols: 0,
  edges: 0,
  languages: [],
  scannedAt: null,
};

export async function indexStats(): Promise<IndexStats> {
  if (isDemo()) return demoIndexStats();
  if (!inTauri()) return NO_INDEX;
  try {
    return await invoke<IndexStats>("index_stats");
  } catch (err) {
    console.error("không đọc được thống kê chỉ mục", err);
    return NO_INDEX;
  }
}

/** Chiều đi trong đồ thị. `both` là lân cận trực tiếp, hai chiều. */
export type GraphDirection = "both" | "callers" | "callees";

export const DIRECTION_LABEL: Record<GraphDirection, string> = {
  both: "Lân cận",
  callers: "Ai gọi ký hiệu này",
  callees: "Ký hiệu này gọi ai",
};

/** Lân cận trực tiếp của một ký hiệu, cả hai chiều. */
export function graphNeighborhood(symbol: string, depth: number): Promise<GraphView> {
  return invoke<GraphView>("graph_neighborhood", { symbol, depth });
}

/** Đi theo cạnh `calls` về một phía, nhiều bậc. */
export function graphTrace(
  symbol: string,
  direction: "callers" | "callees",
  depth: number,
): Promise<GraphView> {
  return invoke<GraphView>("graph_trace", { symbol, direction, depth });
}

/**
 * Một cửa cho cả màn hình: chiều quyết định lệnh nào được gọi.
 *
 * Gộp ở đây chứ không ở component vì "hai chiều" và "một chiều" là hai lệnh khác nhau
 * của lõi, còn với người dùng thì đó là cùng một núm vặn.
 */
export function loadGraphView(
  symbol: string,
  direction: GraphDirection,
  depth: number,
): Promise<GraphView> {
  if (isDemo()) return Promise.resolve(demoGraphView(symbol, direction, depth));
  if (direction === "both") return graphNeighborhood(symbol, depth);
  return graphTrace(symbol, direction, depth);
}

export const NODE_KIND_LABEL: Record<GraphNodeKind, string> = {
  function: "hàm",
  method: "phương thức",
  struct: "struct",
  class: "lớp",
  trait: "trait",
  interface: "interface",
  enum: "enum",
  module: "mô-đun",
  constant: "hằng",
  type: "kiểu",
};

export const EDGE_KIND_LABEL: Record<GraphEdgeKind, string> = {
  calls: "gọi",
  imports: "nhập",
  contains: "chứa",
  implements: "hiện thực",
  extends: "kế thừa",
  references: "tham chiếu",
};

/** Mỗi loại cạnh một kiểu nét, để phân biệt được khi đã in đen trắng hoặc thu nhỏ. */
const EDGE_ARROW: Record<GraphEdgeKind, string> = {
  calls: "-->",
  imports: "-.->",
  contains: "---",
  implements: "==>",
  extends: "==>",
  references: "-.-",
};

/** Hình dạng đỉnh theo loại ký hiệu. Cặp mở/đóng, chèn nhãn đã trong ngoặc kép vào giữa. */
const NODE_SHAPE: Record<GraphNodeKind, [string, string]> = {
  function: ["([", "])"],
  method: ["([", "])"],
  struct: ["[", "]"],
  class: ["[", "]"],
  trait: ["{{", "}}"],
  interface: ["{{", "}}"],
  enum: ["[/", "/]"],
  module: ["[[", "]]"],
  constant: ["((", "))"],
  type: ["[(", ")]"],
};

/**
 * Thoát một tên ký hiệu để nhét được vào nhãn mermaid.
 *
 * Tên thật trong Rust và TypeScript mang gần hết những ký tự mermaid dùng làm cú pháp:
 * `Vec<Config>`, `HashMap<String, Vec<Token>>::insert`, `Box<dyn Fn(&str) -> bool>`,
 * `impl Driver for OpenAiDriver`. Ngoặc nhọn đóng khối, ngoặc vuông đổi hình dạng đỉnh,
 * ngoặc kép kết thúc nhãn, gạch đứng mở nhãn cạnh — mỗi cái là một cách khác nhau để
 * làm hỏng cả sơ đồ chứ không chỉ một đỉnh.
 *
 * Cách thoát là mã thực thể dạng số của mermaid (`#60;`), thứ được giải mã lại thành
 * đúng ký tự lúc vẽ. `#` phải đi trước, nếu không những lần thay sau lại bị thoát thêm
 * một lần nữa và người dùng đọc được `#60;` trên hình.
 */
export function escapeLabel(name: string): string {
  return name
    .replace(/#/g, "#35;")
    .replace(/"/g, "#34;")
    .replace(/</g, "#60;")
    .replace(/>/g, "#62;")
    .replace(/\[/g, "#91;")
    .replace(/\]/g, "#93;")
    .replace(/\{/g, "#123;")
    .replace(/\}/g, "#125;")
    .replace(/\(/g, "#40;")
    .replace(/\)/g, "#41;")
    .replace(/\|/g, "#124;")
    .replace(/`/g, "#96;")
    .replace(/\s+/g, " ")
    .trim();
}

/** Nhãn dài làm đỉnh phình ra và đẩy cả sơ đồ; tên đầy đủ vẫn nằm ở danh sách cạnh. */
const MAX_LABEL = 44;

function label(node: GraphNode): string {
  const name = node.name.length > MAX_LABEL ? `${node.name.slice(0, MAX_LABEL - 1)}…` : node.name;
  return escapeLabel(name);
}

/**
 * `GraphView` → nguồn mermaid.
 *
 * Id đỉnh sinh theo thứ tự (`n0`, `n1`, …) chứ **không** lấy từ `GraphNode.id`. Id của
 * lõi là đường dẫn cộng tên ký hiệu — có dấu cách, dấu hai chấm, dấu chấm — và mermaid
 * không nhận id như vậy. Sinh số cũng cắt luôn mọi đường một cái tên lạ làm hỏng cú
 * pháp: chỗ duy nhất văn bản của người dùng đi vào là nhãn, và nhãn đã qua `escapeLabel`.
 */
export function viewToMermaid(view: GraphView, focusId?: string): string {
  if (view.nodes.length === 0) return "";

  const ids = new Map<string, string>();
  view.nodes.forEach((node, index) => ids.set(node.id, `n${index}`));

  const lines = ["flowchart LR"];
  for (const node of view.nodes) {
    const id = ids.get(node.id);
    if (id === undefined) continue;
    const shape = NODE_SHAPE[node.kind];
    const mark = node.id === focusId ? " ◂ đang xem" : "";
    lines.push(`  ${id}${shape[0]}"${label(node)}${mark}"${shape[1]}`);
  }

  for (const edge of view.edges) {
    const src = ids.get(edge.src);
    const dst = ids.get(edge.dst);
    // Cạnh trỏ ra ngoài tập đỉnh đã bị cắt: bỏ, chứ không dựng một đỉnh ma không có
    // đường dẫn để bấm vào.
    if (src === undefined || dst === undefined) continue;
    lines.push(`  ${src} ${EDGE_ARROW[edge.kind]}|${EDGE_KIND_LABEL[edge.kind]}| ${dst}`);
  }

  const focus = focusId === undefined ? undefined : ids.get(focusId);
  if (focus !== undefined) {
    // Chỉ dày nét, không đổi màu: màu ở đây phải là mã màu thô, mà mã màu thô thì sai
    // một trong hai theme.
    lines.push("  classDef focus stroke-width:3px");
    lines.push(`  class ${focus} focus`);
  }

  return lines.join("\n");
}

/** Cạnh nhìn từ một đỉnh: đầu kia là đâu, và ta đang đứng ở phía nào của mũi tên. */
export interface Incident {
  edge: GraphEdge;
  other: GraphNode;
  outgoing: boolean;
}

/**
 * Cạnh chạm vào một đỉnh, đã ghép sẵn với đỉnh đầu kia.
 *
 * Danh sách chữ này mới là thứ bấm được để đi tiếp, không phải hình: SVG mermaid sinh ra
 * không có chỗ nào để gắn sự kiện một cách đáng tin, và một lối đi chỉ hoạt động một
 * nửa còn tệ hơn không có.
 */
export function incidentEdges(view: GraphView, focusId: string): Incident[] {
  const byId = new Map(view.nodes.map((node) => [node.id, node]));
  const out: Incident[] = [];
  for (const edge of view.edges) {
    if (edge.src === focusId) {
      const other = byId.get(edge.dst);
      if (other !== undefined) out.push({ edge, other, outgoing: true });
    } else if (edge.dst === focusId) {
      const other = byId.get(edge.src);
      if (other !== undefined) out.push({ edge, other, outgoing: false });
    }
  }
  return out;
}
