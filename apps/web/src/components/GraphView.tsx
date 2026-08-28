import cytoscape from "cytoscape";
import type { Core, EdgeSingular, EventObject, NodeSingular } from "cytoscape";
import { Focus, Layers, Plus, RefreshCw, Search, Share2, Shuffle, X, ZoomIn, ZoomOut } from "lucide-solid";
import { For, Match, Show, Switch, createEffect, createMemo, createResource, createSignal, onCleanup, onMount, untrack } from "solid-js";
import { createStore } from "solid-js/store";
import { api } from "../api";
import type { GraphEdge, GraphNode, GraphSnapshot } from "../types";

const PALETTE = ["#1c7a63", "#3d6fb4", "#a8672c", "#7d55ab", "#a8465c", "#2f8f8a", "#5c6f3a", "#8a5a86"];

/** Mỗi lần mở rộng chỉ xin lân cận trực tiếp, đủ để thêm một lớp mà không kéo cả kho về. */
const EXPAND_DEPTH = 1;
const EXPAND_LIMIT = 40;

type NodeData = {
  id: string;
  label: string;
  type: string;
  description: string;
  file: string;
  degree: number;
  color: string;
  size: number;
  layer: number;
};

type EdgeData = {
  id: string;
  source: string;
  target: string;
  label: string;
  description: string;
  weight: number;
  file: string;
};

/** Node hay quan hệ đang mở trong ô chi tiết; hai thứ dùng chung một chỗ. */
type Detail =
  | { kind: "node"; node: NodeData }
  | { kind: "edge"; edge: EdgeData; from: NodeData; to: NodeData };

function readText(properties: Record<string, unknown>, key: string) {
  const value = properties[key];
  return typeof value === "string" ? value : "";
}

function readNumber(properties: Record<string, unknown>, key: string) {
  const value = properties[key];
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function hashOf(text: string) {
  let hash = 0;
  for (let index = 0; index < text.length; index += 1) hash = (hash * 31 + text.charCodeAt(index)) >>> 0;
  return hash;
}

function colorOf(type: string) {
  return PALETTE[hashOf(type) % PALETTE.length];
}

/** Quan hệ không có hướng, nên hai đầu sắp xếp lại để hai lần tải không sinh ra hai cạnh. */
function edgeKey(edge: GraphEdge) {
  const [from, to] = [edge.source, edge.target].sort();
  return `${from}|${to}|${edge.type ?? ""}`;
}

/** Mỗi node toả lớp mới từ một góc riêng, để hai node cạnh nhau không đè vòng lên nhau. */
function angleSeed(id: string) {
  return ((hashOf(id) % 360) * Math.PI) / 180;
}

function sizeOf(degree: number) {
  return 18 + Math.min(30, Math.sqrt(degree) * 9);
}

function prefersReducedMotion() {
  return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Cytoscape vẽ lên canvas nên không đọc được biến CSS; lấy giá trị token ra đây. */
function token(name: string, fallback: string) {
  if (typeof window === "undefined") return fallback;
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

function buildStyle(): cytoscape.StylesheetStyle[] {
  const ink = token("--ink", "#17231f");
  const text = token("--text", "#293732");
  const surface = token("--surface", "#ffffff");
  const line = token("--line-strong", "#c2cec8");
  const accent = token("--accent", "#176b59");
  const accentInk = token("--accent-ink", "#0c4d3f");
  return [
    {
      selector: "node",
      style: {
        "background-color": "data(color)",
        width: "data(size)",
        height: "data(size)",
        label: "data(label)",
        color: text,
        "font-size": 10,
        "font-weight": 600,
        "text-valign": "bottom",
        "text-margin-y": 4,
        "text-max-width": "120px",
        "text-wrap": "ellipsis",
        "text-outline-color": surface,
        "text-outline-width": 2.4,
        "border-width": 2,
        "border-color": surface,
      },
    },
    {
      selector: "edge",
      style: {
        width: 1.4,
        "line-color": line,
        "curve-style": "straight",
        "target-arrow-shape": "none",
      },
    },
    { selector: "node.faded, edge.faded", style: { opacity: 0.12, "text-opacity": 0 } },
    {
      selector: "edge.spotlight",
      style: {
        width: 2.8,
        "line-color": accent,
        label: "data(label)",
        color: accentInk,
        "font-size": 10,
        "font-weight": 600,
        "text-max-width": "160px",
        "text-wrap": "ellipsis",
        "text-outline-color": surface,
        "text-outline-width": 2.4,
      },
    },
    // Node đã mở rộng viền đậm, node đang chờ dữ liệu viền đứt: nhìn là biết còn gì để đào.
    { selector: "node.expanded", style: { "border-color": accent, "border-width": 3.4 } },
    { selector: "node.expanding", style: { "border-color": accentInk, "border-width": 4, "border-style": "dashed" } },
    { selector: "node:selected", style: { "border-color": ink, "border-width": 3 } },
    { selector: "edge:selected", style: { width: 3.4, "line-color": accent } },
    { selector: ".hidden", style: { display: "none" } },
  ];
}

export function GraphView(props: { workspaceId: string; workspaceName: string }) {
  const [focus, setFocus] = createSignal("*");
  const [depth, setDepth] = createSignal(2);
  const [limit, setLimit] = createSignal(150);
  const [term, setTerm] = createSignal("");
  const [detail, setDetail] = createSignal<Detail | null>(null);
  const [muted, setMuted] = createStore<Record<string, boolean>>({});
  const [counts, setCounts] = createSignal({ nodes: 0, edges: 0 });
  const [types, setTypes] = createSignal<[string, number][]>([]);
  const [settling, setSettling] = createSignal(false);
  const [expandedIds, setExpandedIds] = createSignal<Set<string>>(new Set());
  const [expanding, setExpanding] = createSignal("");
  const [layers, setLayers] = createSignal(0);
  const [expandNote, setExpandNote] = createSignal("");

  // Đồ thị tích luỹ qua nhiều lần gọi, nên phải nhớ cái gì đã có để lần sau chỉ thêm phần mới.
  const nodeIndex = new Map<string, GraphNode>();
  const edgeIndex = new Map<string, GraphEdge>();
  const drawnEdges = new Set<string>();
  // Đổi lát nền giữa lúc đang mở lớp thì kết quả cũ phải bị bỏ, không gộp nhầm vào đồ thị mới.
  let generation = 0;

  const [snapshot, { refetch }] = createResource(
    () => (props.workspaceId ? { id: props.workspaceId, entity: focus(), depth: depth(), limit: limit() } : undefined),
    (key) => api.graph(key.id, key.entity, key.depth, key.limit),
  );

  const [suggestions] = createResource(
    () => (props.workspaceId ? { id: props.workspaceId, q: term().trim() } : undefined),
    async (key) => {
      // Hộp tìm kiếm chỉ gợi ý; danh sách ngắn nên không cần phân trang.
      await new Promise((resolve) => setTimeout(resolve, 220));
      return api.graphEntities(key.id, key.q, 40);
    },
  );

  let stage!: HTMLDivElement;
  let cy: Core | undefined;
  const [theme, setTheme] = createSignal(
    typeof document === "undefined" ? "light" : document.documentElement.dataset.theme ?? "light",
  );

  const nodeData = (element: NodeSingular) => element.data() as NodeData;
  const edgeData = (element: EdgeSingular) => element.data() as EdgeData;

  /** Làm mờ phần còn lại để thấy rõ node đang trỏ cùng lân cận của nó. */
  const spotlight = (element: NodeSingular | EdgeSingular) => {
    if (!cy) return;
    const near = element.isNode()
      ? (element as NodeSingular).closedNeighborhood()
      : element.union((element as EdgeSingular).connectedNodes());
    cy.elements().addClass("faded");
    near.removeClass("faded");
    element.addClass("spotlight");
  };

  const clearSpotlight = () => {
    cy?.elements().removeClass("faded spotlight");
  };

  /** Gộp một lát đồ thị vừa tải vào phần đang có, trả về đúng những phần tử chưa vẽ. */
  const ingest = (data: GraphSnapshot, layer: number) => {
    const freshNodes: cytoscape.ElementDefinition[] = [];
    for (const node of data.nodes) {
      if (nodeIndex.has(node.id)) continue;
      nodeIndex.set(node.id, node);
      const type = readText(node.properties, "entity_type") || "khác";
      const item: NodeData = {
        id: node.id,
        label: node.labels[0] ?? node.id,
        type,
        description: readText(node.properties, "description"),
        file: readText(node.properties, "file_path"),
        degree: 0,
        color: colorOf(type),
        size: sizeOf(0),
        layer,
      };
      freshNodes.push({ group: "nodes", data: item, classes: muted[type] ? "hidden" : "" });
    }

    for (const edge of data.edges) {
      if (edge.source === edge.target) continue;
      const key = edgeKey(edge);
      if (!edgeIndex.has(key)) edgeIndex.set(key, edge);
    }

    // Cạnh chỉ vẽ được khi đã có cả hai đầu, nên lớp mới có thể làm sống lại cạnh treo của lớp trước.
    const freshEdges: cytoscape.ElementDefinition[] = [];
    for (const [key, edge] of edgeIndex) {
      if (drawnEdges.has(key)) continue;
      if (!nodeIndex.has(edge.source) || !nodeIndex.has(edge.target)) continue;
      drawnEdges.add(key);
      const keywords = readText(edge.properties, "keywords");
      const item: EdgeData = {
        id: key,
        source: edge.source,
        target: edge.target,
        label: keywords || edge.type || "liên quan",
        description: readText(edge.properties, "description"),
        weight: readNumber(edge.properties, "weight"),
        file: readText(edge.properties, "file_path"),
      };
      freshEdges.push({ group: "edges", data: item });
    }

    return { freshNodes, freshEdges };
  };

  /** Bậc và cỡ node đọc từ đồ thị đang vẽ, vì mỗi lớp mới lại đổi số quan hệ của node cũ. */
  const refreshStats = () => {
    const instance = cy;
    if (!instance) return;
    const tally = new Map<string, number>();
    instance.nodes().forEach((node) => {
      const degree = node.connectedEdges().length;
      node.data("degree", degree);
      node.data("size", sizeOf(degree));
      const type = nodeData(node).type;
      tally.set(type, (tally.get(type) ?? 0) + 1);
    });
    setCounts({ nodes: instance.nodes().length, edges: instance.edges().length });
    setTypes([...tally.entries()].sort((left, right) => right[1] - left[1]));
  };

  const runLayout = (elements: cytoscape.Collection, randomize: boolean) => {
    if (elements.empty()) return;
    setSettling(true);
    const layout = elements.layout({
      name: "cose",
      animate: !prefersReducedMotion(),
      animationDuration: 700,
      nodeDimensionsIncludeLabels: true,
      idealEdgeLength: () => 90,
      nodeRepulsion: () => 9000,
      randomize,
      fit: true,
      padding: 40,
    });
    layout.one("layoutstop", () => setSettling(false));
    layout.run();
  };

  /** Một lát nền mới thì bỏ hết phần tích luỹ: người dùng vừa đổi tâm hoặc đổi giới hạn. */
  const renderBase = (data: GraphSnapshot | undefined) => {
    const instance = cy;
    if (!instance) return;
    generation += 1;
    setDetail(null);
    setExpandNote("");
    setExpandedIds(new Set<string>());
    setExpanding("");
    nodeIndex.clear();
    edgeIndex.clear();
    drawnEdges.clear();
    instance.elements().remove();
    if (!data || data.nodes.length === 0) {
      setCounts({ nodes: 0, edges: 0 });
      setTypes([]);
      setLayers(0);
      return;
    }
    const { freshNodes, freshEdges } = ingest(data, 0);
    instance.add([...freshNodes, ...freshEdges]);
    setLayers(1);
    refreshStats();
    runLayout(instance.elements(), true);
  };

  /** Mở thêm một lớp quanh node đang chọn, giữ nguyên chỗ của phần đã vẽ. */
  const expandNode = async (id: string) => {
    const instance = cy;
    if (!instance || !props.workspaceId) return;
    if (expanding() || expandedIds().has(id)) return;
    const parent = instance.getElementById(id);
    if (parent.empty()) return;

    const era = generation;
    setExpanding(id);
    setExpandNote("");
    parent.addClass("expanding");
    try {
      const data = await api.graph(props.workspaceId, id, EXPAND_DEPTH, EXPAND_LIMIT);
      if (era !== generation) return;
      const layer = layers();
      const { freshNodes, freshEdges } = ingest(data, layer);
      // Xếp lớp mới thành vòng quanh node cha thay vì chạy lại layout, để hình cũ đứng yên.
      const origin = parent.position();
      const radius = 110 + Math.min(160, freshNodes.length * 7);
      const start = angleSeed(id);
      freshNodes.forEach((element, index) => {
        const angle = start + (2 * Math.PI * index) / Math.max(1, freshNodes.length);
        element.position = {
          x: origin.x + radius * Math.cos(angle),
          y: origin.y + radius * Math.sin(angle),
        };
      });
      instance.add([...freshNodes, ...freshEdges]);
      refreshStats();
      setExpandedIds(new Set(expandedIds()).add(id));
      parent.addClass("expanded");
      if (freshNodes.length > 0) setLayers(layer + 1);
      if (freshNodes.length === 0) {
        setExpandNote(
          freshEdges.length > 0
            ? `Thêm ${freshEdges.length} quan hệ giữa các thực thể đã có.`
            : "Node này không còn lân cận nào chưa hiện.",
        );
      }
      const open = detail();
      if (open?.kind === "node" && open.node.id === id) {
        setDetail({ kind: "node", node: { ...(parent.data() as NodeData) } });
      }
    } catch (error) {
      if (era === generation) {
        setExpandNote(error instanceof Error ? error.message : "Không mở rộng được node này.");
      }
    } finally {
      if (era === generation) {
        parent.removeClass("expanding");
        setExpanding("");
      }
    }
  };

  onMount(() => {
    cy = cytoscape({
      container: stage,
      minZoom: 0.15,
      maxZoom: 3.2,
      wheelSensitivity: 0.25,
      // Cytoscape lo kéo node, kéo nền và phóng to; phần dưới chỉ còn là cách vẽ.
      style: buildStyle(),
    });

    cy.on("tap", "node", (event: EventObject) => {
      setDetail({ kind: "node", node: nodeData(event.target as NodeSingular) });
    });
    cy.on("tap", "edge", (event: EventObject) => {
      const edge = event.target as EdgeSingular;
      setDetail({
        kind: "edge",
        edge: edgeData(edge),
        from: nodeData(edge.source()),
        to: nodeData(edge.target()),
      });
    });
    cy.on("tap", (event: EventObject) => {
      if (event.target === cy) setDetail(null);
    });
    // Bấm đúp là mở thêm một lớp quanh node, không phải vẽ lại đồ thị từ đầu.
    cy.on("dbltap", "node", (event: EventObject) => {
      void expandNode(nodeData(event.target as NodeSingular).id);
    });
    cy.on("mouseover", "node, edge", (event: EventObject) => {
      spotlight(event.target as NodeSingular | EdgeSingular);
    });
    cy.on("mouseout", "node, edge", clearSpotlight);
    // Kéo một node là chủ ý sắp lại hình, đừng để lớp mờ dính lại giữa chừng.
    cy.on("grab", "node", clearSpotlight);
  });

  onCleanup(() => {
    cy?.destroy();
    cy = undefined;
  });

  onMount(() => {
    // Đổi sáng/tối thì màu chữ và đường nối phải đi theo, canvas không tự biết.
    const observer = new MutationObserver(() => setTheme(document.documentElement.dataset.theme ?? "light"));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    onCleanup(() => observer.disconnect());
  });

  createEffect(() => {
    theme();
    cy?.style(buildStyle());
  });

  createEffect(() => {
    const data = snapshot() as GraphSnapshot | undefined;
    // Chỉ lát nền mới được vẽ lại: untrack để bật/tắt loại thực thể không xoá các lớp đã mở.
    untrack(() => renderBase(data));
  });

  const toggleType = (type: string) => {
    const next = !muted[type];
    setMuted(type, next);
    // Ẩn node cũng ẩn luôn cạnh nối vào nó, nên không còn cạnh treo lơ lửng.
    cy?.nodes(`[type = "${type.replaceAll('"', '\\"')}"]`).toggleClass("hidden", next);
  };

  const focusEntity = (name: string) => {
    setTerm(name === "*" ? "" : name);
    setFocus(name);
  };

  const zoomBy = (factor: number) => {
    if (!cy) return;
    cy.zoom({
      level: cy.zoom() * factor,
      renderedPosition: { x: cy.width() / 2, y: cy.height() / 2 },
    });
  };

  const fit = () => cy?.fit(undefined, 40);

  const relation = createMemo(() => {
    const current = detail();
    return current?.kind === "edge" ? current : undefined;
  });

  const entity = createMemo(() => {
    const current = detail();
    return current?.kind === "node" ? current.node : undefined;
  });

  return (
    <section class="page-view graph-page">
      <div class="page-heading page-heading-row">
        <div>
          <span>Kho tri thức</span>
          <h1>Đồ thị tri thức</h1>
          <p>
            Thực thể và quan hệ mà Private AI rút ra từ tài liệu của{" "}
            <strong>{props.workspaceName}</strong>. Kéo node để sắp lại, kéo nền để dời khung, lăn
            chuột để phóng to, bấm một node hoặc một đường nối để xem chi tiết. Bấm đúp vào một node
            để mở thêm một lớp lân cận ngay trên hình đang có.
          </p>
        </div>
        <div class="page-heading-actions">
          <Show when={focus() !== "*"}>
            <button class="button button-secondary" type="button" onClick={() => focusEntity("*")}>
              <Layers size={17} /> Toàn bộ đồ thị
            </button>
          </Show>
          <Show when={layers() > 1}>
            <button
              class="button button-secondary"
              type="button"
              disabled={settling()}
              onClick={() => cy && runLayout(cy.elements(), false)}
            >
              <Shuffle size={17} /> Sắp xếp lại
            </button>
          </Show>
          <button class="button button-secondary" type="button" onClick={() => void refetch()}>
            <RefreshCw size={17} /> Tải lại
          </button>
        </div>
      </div>

      <div class="library-toolbar graph-toolbar">
        <div class="library-search">
          <Search size={16} />
          <input
            type="search"
            list="graph-entity-options"
            value={term()}
            placeholder="Tìm thực thể để xem lân cận"
            aria-label="Tìm thực thể trong đồ thị"
            onInput={(event) => setTerm(event.currentTarget.value)}
            onChange={(event) => {
              const value = event.currentTarget.value.trim();
              focusEntity(value || "*");
            }}
          />
        </div>
        <datalist id="graph-entity-options">
          <For each={suggestions() ?? []}>{(item) => <option value={item.name} />}</For>
        </datalist>

        <label class="graph-control">
          <span>Độ sâu</span>
          <select
            value={String(depth())}
            disabled={focus() === "*"}
            onChange={(event) => setDepth(Number(event.currentTarget.value))}
          >
            <For each={[1, 2, 3, 4]}>{(value) => <option value={value}>{value}</option>}</For>
          </select>
        </label>
        <label class="graph-control">
          <span>Số node</span>
          <select value={String(limit())} onChange={(event) => setLimit(Number(event.currentTarget.value))}>
            <For each={[50, 150, 300, 500]}>{(value) => <option value={value}>{value}</option>}</For>
          </select>
        </label>
        <div class="graph-zoom">
          <button class="icon-button" type="button" onClick={() => zoomBy(1 / 1.2)} aria-label="Thu nhỏ">
            <ZoomOut size={17} />
          </button>
          <button class="icon-button" type="button" onClick={() => zoomBy(1.2)} aria-label="Phóng to">
            <ZoomIn size={17} />
          </button>
          <button class="icon-button" type="button" onClick={fit} aria-label="Vừa khung hình">
            <Focus size={17} />
          </button>
        </div>
      </div>

      <div class="graph-layout">
        <div class="graph-canvas">
          <div
            ref={stage}
            class="graph-stage"
            classList={{ busy: snapshot.loading }}
            role="application"
            aria-label={`Đồ thị tri thức của ${props.workspaceName}`}
          />
          <Show when={counts().nodes === 0}>
            <Switch>
              <Match when={!props.workspaceId}>
                <p class="graph-empty">Hãy mở một không gian làm việc để xem đồ thị của nó.</p>
              </Match>
              <Match when={snapshot.loading}>
                <p class="graph-empty">Đang dựng đồ thị…</p>
              </Match>
              <Match when={snapshot.error}>
                <p class="graph-empty inline-error" role="alert">
                  {(snapshot.error as Error).message}
                </p>
              </Match>
              <Match when>
                <p class="graph-empty">
                  Chưa có thực thể nào trong không gian này. Tải tài liệu lên rồi chờ lập chỉ mục
                  xong, đồ thị sẽ tự có dữ liệu.
                </p>
              </Match>
            </Switch>
          </Show>

          <Show when={counts().nodes > 0}>
            <div class="graph-status">
              <span>
                <strong>{counts().nodes}</strong> thực thể · <strong>{counts().edges}</strong> quan hệ
                <Show when={layers() > 1}> · {layers()} lớp</Show>
                <Show when={expanding()}> · đang mở lớp mới…</Show>
                <Show when={settling()}> · đang sắp xếp…</Show>
              </span>
              <Show when={expandNote()}>
                <em class="graph-status-note">{expandNote()}</em>
              </Show>
              <Show when={(snapshot() as GraphSnapshot | undefined)?.truncated}>
                <em>Lớp nền đã bị cắt bớt, tăng “Số node” hoặc bấm đúp từng node để đào thêm.</em>
              </Show>
            </div>
          </Show>
        </div>

        <aside class="graph-side">
          <Show when={types().length > 0}>
            <div class="graph-legend">
              <h2>Loại thực thể</h2>
              <For each={types()}>
                {([type, count]) => (
                  <button
                    class="graph-legend-item"
                    classList={{ off: Boolean(muted[type]) }}
                    type="button"
                    onClick={() => toggleType(type)}
                  >
                    <i style={{ background: colorOf(type) }} />
                    <span>{type}</span>
                    <em>{count}</em>
                  </button>
                )}
              </For>
            </div>
          </Show>

          <Switch
            fallback={
              <p class="graph-hint">
                <Share2 size={16} /> Kéo node để sắp lại chỗ. Bấm một node hoặc một đường nối để xem
                chi tiết, bấm đúp vào node để mở thêm một lớp lân cận quanh nó.
              </p>
            }
          >
            <Match when={entity()}>
              {(node) => (
                <article class="graph-detail">
                  <header>
                    <div>
                      <span style={{ color: node().color }}>{node().type}</span>
                      <h2>{node().label}</h2>
                    </div>
                    <button class="icon-button" type="button" onClick={() => setDetail(null)} aria-label="Đóng">
                      <X size={17} />
                    </button>
                  </header>
                  <p class="graph-detail-meta">
                    {node().degree} quan hệ
                    <Show when={node().layer > 0}> · mở ở lớp {node().layer + 1}</Show>
                    <Show when={node().file}> · nguồn: {node().file}</Show>
                  </p>
                  <Show when={node().description}>
                    <p class="graph-detail-body">{node().description}</p>
                  </Show>
                  <div class="graph-detail-actions">
                    <button
                      class="button button-secondary"
                      type="button"
                      disabled={Boolean(expanding()) || expandedIds().has(node().id)}
                      onClick={() => void expandNode(node().id)}
                    >
                      <Plus size={17} />
                      {expanding() === node().id
                        ? "Đang mở lớp…"
                        : expandedIds().has(node().id)
                          ? "Đã mở lớp này"
                          : "Mở rộng lân cận"}
                    </button>
                    <button class="button button-secondary" type="button" onClick={() => focusEntity(node().id)}>
                      <Focus size={17} /> Chỉ xem lân cận
                    </button>
                  </div>
                </article>
              )}
            </Match>
            <Match when={relation()}>
              {(current) => (
                <article class="graph-detail">
                  <header>
                    <div>
                      <span>Quan hệ</span>
                      <h2>{current().edge.label}</h2>
                    </div>
                    <button class="icon-button" type="button" onClick={() => setDetail(null)} aria-label="Đóng">
                      <X size={17} />
                    </button>
                  </header>
                  <div class="graph-relation-ends">
                    <button
                      type="button"
                      onClick={() => focusEntity(current().from.id)}
                      title={`Chỉ xem lân cận của ${current().from.label}`}
                    >
                      <i style={{ background: current().from.color }} />
                      {current().from.label}
                    </button>
                    <span aria-hidden="true">→</span>
                    <button
                      type="button"
                      onClick={() => focusEntity(current().to.id)}
                      title={`Chỉ xem lân cận của ${current().to.label}`}
                    >
                      <i style={{ background: current().to.color }} />
                      {current().to.label}
                    </button>
                  </div>
                  <p class="graph-detail-meta">
                    <Show when={current().edge.weight > 0}>độ mạnh {current().edge.weight.toFixed(1)}</Show>
                    <Show when={current().edge.file}> · nguồn: {current().edge.file}</Show>
                  </p>
                  <Show when={current().edge.description}>
                    <p class="graph-detail-body">{current().edge.description}</p>
                  </Show>
                </article>
              )}
            </Match>
          </Switch>
        </aside>
      </div>
    </section>
  );
}

export default GraphView;
