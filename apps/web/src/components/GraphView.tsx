import { Focus, Layers, RefreshCw, Search, Share2, X, ZoomIn, ZoomOut } from "lucide-solid";
import { For, Match, Show, Switch, batch, createEffect, createMemo, createResource, createSignal, onCleanup } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { api } from "../api";
import type { GraphSnapshot } from "../types";

// Khung toạ độ cố định của SVG: mọi tính toán lực đều nằm trong hệ này, còn kích
// thước thật trên màn hình do CSS quyết định.
const VIEW_WIDTH = 1000;
const VIEW_HEIGHT = 620;
const PALETTE = ["#1c7a63", "#3d6fb4", "#a8672c", "#7d55ab", "#a8465c", "#2f8f8a", "#5c6f3a", "#8a5a86"];

type SimNode = {
  id: string;
  label: string;
  type: string;
  description: string;
  source: string;
  degree: number;
  x: number;
  y: number;
};

type SimEdge = { source: number; target: number; label: string };

function readProperty(properties: Record<string, unknown>, key: string) {
  const value = properties[key];
  return typeof value === "string" ? value : "";
}

function colorOf(type: string) {
  let hash = 0;
  for (let index = 0; index < type.length; index += 1) hash = (hash * 31 + type.charCodeAt(index)) >>> 0;
  return PALETTE[hash % PALETTE.length];
}

function radiusOf(degree: number) {
  return 7 + Math.min(13, Math.sqrt(degree) * 4);
}

export function GraphView(props: { workspaceId: string; workspaceName: string }) {
  const [focus, setFocus] = createSignal("*");
  const [depth, setDepth] = createSignal(2);
  const [limit, setLimit] = createSignal(150);
  const [term, setTerm] = createSignal("");
  const [selected, setSelected] = createSignal(-1);
  const [hovered, setHovered] = createSignal(-1);
  const [muted, setMuted] = createStore<Record<string, boolean>>({});
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPan] = createSignal({ x: 0, y: 0 });
  const [settling, setSettling] = createSignal(false);

  const [nodes, setNodes] = createStore<SimNode[]>([]);
  const [edges, setEdges] = createSignal<SimEdge[]>([]);

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

  let svgRef!: SVGSVGElement;
  let sceneRef!: SVGGElement;
  let frame = 0;
  let velocityX: Float64Array = new Float64Array(0);
  let velocityY: Float64Array = new Float64Array(0);
  let pinned = new Set<number>();
  let dragging = -1;
  let panning: { x: number; y: number; origin: { x: number; y: number } } | null = null;

  const stopSimulation = () => {
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    setSettling(false);
  };
  onCleanup(stopSimulation);

  const neighbours = createMemo(() => {
    const map = new Map<number, Set<number>>();
    edges().forEach((edge) => {
      if (!map.has(edge.source)) map.set(edge.source, new Set());
      if (!map.has(edge.target)) map.set(edge.target, new Set());
      map.get(edge.source)!.add(edge.target);
      map.get(edge.target)!.add(edge.source);
    });
    return map;
  });

  const types = createMemo(() => {
    const counts = new Map<string, number>();
    nodes.forEach((node) => counts.set(node.type, (counts.get(node.type) ?? 0) + 1));
    return [...counts.entries()].sort((left, right) => right[1] - left[1]);
  });

  const isMuted = (index: number) => Boolean(muted[nodes[index]?.type ?? ""]);

  const highlighted = createMemo(() => {
    const anchor = hovered() >= 0 ? hovered() : selected();
    if (anchor < 0) return null;
    const set = new Set<number>([anchor]);
    neighbours().get(anchor)?.forEach((index) => set.add(index));
    return set;
  });

  const nodeOpacity = (index: number) => {
    if (isMuted(index)) return 0.12;
    const set = highlighted();
    if (!set) return 1;
    return set.has(index) ? 1 : 0.2;
  };

  const edgeOpacity = (edge: SimEdge) => {
    if (isMuted(edge.source) || isMuted(edge.target)) return 0.05;
    const set = highlighted();
    if (!set) return 0.5;
    return set.has(edge.source) && set.has(edge.target) ? 0.9 : 0.08;
  };

  const fitToContent = () => {
    if (nodes.length === 0) {
      setZoom(1);
      setPan({ x: 0, y: 0 });
      return;
    }
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    nodes.forEach((node) => {
      minX = Math.min(minX, node.x);
      minY = Math.min(minY, node.y);
      maxX = Math.max(maxX, node.x);
      maxY = Math.max(maxY, node.y);
    });
    const padding = 70;
    const width = Math.max(1, maxX - minX) + padding * 2;
    const height = Math.max(1, maxY - minY) + padding * 2;
    const scale = Math.min(2, Math.max(0.25, Math.min(VIEW_WIDTH / width, VIEW_HEIGHT / height)));
    setZoom(scale);
    setPan({
      x: VIEW_WIDTH / 2 - ((minX + maxX) / 2) * scale,
      y: VIEW_HEIGHT / 2 - ((minY + maxY) / 2) * scale,
    });
  };

  // Fruchterman–Reingold: đẩy mọi cặp node ra xa, kéo hai đầu của cạnh lại gần,
  // rồi hạ nhiệt dần cho tới khi hình ổn định.
  const simulate = () => {
    stopSimulation();
    const count = nodes.length;
    if (count === 0) return;
    const positionX = new Float64Array(count);
    const positionY = new Float64Array(count);
    nodes.forEach((node, index) => {
      positionX[index] = node.x;
      positionY[index] = node.y;
    });
    velocityX = new Float64Array(count);
    velocityY = new Float64Array(count);
    const links = edges();
    const area = VIEW_WIDTH * VIEW_HEIGHT;
    const spring = Math.sqrt(area / count);
    let temperature = VIEW_WIDTH / 8;
    setSettling(true);

    const iterate = () => {
      velocityX.fill(0);
      velocityY.fill(0);
      for (let a = 0; a < count; a += 1) {
        for (let b = a + 1; b < count; b += 1) {
          let dx = positionX[a] - positionX[b];
          let dy = positionY[a] - positionY[b];
          let distance = Math.hypot(dx, dy);
          if (distance < 0.05) {
            dx = (a % 7) - 3 || 1;
            dy = (b % 5) - 2 || 1;
            distance = Math.hypot(dx, dy);
          }
          const push = (spring * spring) / distance;
          const ux = (dx / distance) * push;
          const uy = (dy / distance) * push;
          velocityX[a] += ux;
          velocityY[a] += uy;
          velocityX[b] -= ux;
          velocityY[b] -= uy;
        }
      }
      links.forEach((edge) => {
        const dx = positionX[edge.source] - positionX[edge.target];
        const dy = positionY[edge.source] - positionY[edge.target];
        const distance = Math.max(0.05, Math.hypot(dx, dy));
        const pull = (distance * distance) / spring;
        const ux = (dx / distance) * pull;
        const uy = (dy / distance) * pull;
        velocityX[edge.source] -= ux;
        velocityY[edge.source] -= uy;
        velocityX[edge.target] += ux;
        velocityY[edge.target] += uy;
      });
      for (let index = 0; index < count; index += 1) {
        if (pinned.has(index)) continue;
        velocityX[index] += (VIEW_WIDTH / 2 - positionX[index]) * 0.035;
        velocityY[index] += (VIEW_HEIGHT / 2 - positionY[index]) * 0.035;
        const speed = Math.max(0.001, Math.hypot(velocityX[index], velocityY[index]));
        const step = Math.min(speed, temperature);
        positionX[index] += (velocityX[index] / speed) * step;
        positionY[index] += (velocityY[index] / speed) * step;
      }
      temperature *= 0.94;
    };

    const step = () => {
      for (let round = 0; round < 3 && temperature > 0.4; round += 1) iterate();
      setNodes(
        produce((list) => {
          for (let index = 0; index < list.length; index += 1) {
            list[index].x = positionX[index];
            list[index].y = positionY[index];
          }
        }),
      );
      if (temperature > 0.4) {
        frame = requestAnimationFrame(step);
        return;
      }
      stopSimulation();
      fitToContent();
    };
    frame = requestAnimationFrame(step);
  };

  createEffect(() => {
    const data = snapshot() as GraphSnapshot | undefined;
    stopSimulation();
    setSelected(-1);
    setHovered(-1);
    pinned = new Set();
    if (!data || data.nodes.length === 0) {
      // Node và cạnh phải đổi cùng lúc, nếu không cạnh sẽ trỏ vào chỉ số đã biến mất.
      batch(() => {
        setNodes([]);
        setEdges([]);
      });
      return;
    }
    const index = new Map(data.nodes.map((node, position) => [node.id, position]));
    const degrees = new Array(data.nodes.length).fill(0);
    const links: SimEdge[] = [];
    data.edges.forEach((edge) => {
      const source = index.get(edge.source);
      const target = index.get(edge.target);
      if (source === undefined || target === undefined || source === target) return;
      degrees[source] += 1;
      degrees[target] += 1;
      links.push({ source, target, label: edge.type ?? readProperty(edge.properties, "keywords") });
    });
    // Xếp mầm theo xoắn ốc vàng: mỗi lần mở lại cùng dữ liệu cho cùng bố cục.
    const golden = Math.PI * (3 - Math.sqrt(5));
    const seeded: SimNode[] = data.nodes.map((node, position) => {
      const radius = 40 + Math.sqrt(position + 1) * 26;
      return {
        id: node.id,
        label: node.labels[0] ?? node.id,
        type: readProperty(node.properties, "entity_type") || "khác",
        description: readProperty(node.properties, "description"),
        source: readProperty(node.properties, "file_path"),
        degree: degrees[position],
        x: VIEW_WIDTH / 2 + Math.cos(position * golden) * radius,
        y: VIEW_HEIGHT / 2 + Math.sin(position * golden) * radius,
      };
    });
    batch(() => {
      setNodes(seeded);
      setEdges(links);
    });
    simulate();
  });

  const toScene = (event: PointerEvent | WheelEvent) => {
    const matrix = sceneRef.getScreenCTM();
    const point = svgRef.createSVGPoint();
    point.x = event.clientX;
    point.y = event.clientY;
    if (!matrix) return { x: point.x, y: point.y };
    const local = point.matrixTransform(matrix.inverse());
    return { x: local.x, y: local.y };
  };

  const toViewport = (event: PointerEvent | WheelEvent) => {
    const matrix = svgRef.getScreenCTM();
    const point = svgRef.createSVGPoint();
    point.x = event.clientX;
    point.y = event.clientY;
    if (!matrix) return { x: point.x, y: point.y };
    const local = point.matrixTransform(matrix.inverse());
    return { x: local.x, y: local.y };
  };

  const zoomAround = (factor: number, anchor: { x: number; y: number }, world: { x: number; y: number }) => {
    const next = Math.min(3.2, Math.max(0.2, zoom() * factor));
    setPan({ x: anchor.x - world.x * next, y: anchor.y - world.y * next });
    setZoom(next);
  };

  const zoomByButton = (factor: number) => {
    const centre = { x: VIEW_WIDTH / 2, y: VIEW_HEIGHT / 2 };
    const current = pan();
    const world = { x: (centre.x - current.x) / zoom(), y: (centre.y - current.y) / zoom() };
    zoomAround(factor, centre, world);
  };

  const onWheel = (event: WheelEvent) => {
    event.preventDefault();
    zoomAround(event.deltaY < 0 ? 1.12 : 1 / 1.12, toViewport(event), toScene(event));
  };

  const onBackgroundDown = (event: PointerEvent) => {
    if (event.button !== 0) return;
    const current = pan();
    panning = { x: current.x, y: current.y, origin: { x: event.clientX, y: event.clientY } };
    svgRef.setPointerCapture(event.pointerId);
  };

  const onNodeDown = (index: number, event: PointerEvent) => {
    event.stopPropagation();
    dragging = index;
    pinned.add(index);
    setSelected(index);
    svgRef.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: PointerEvent) => {
    if (dragging >= 0) {
      const point = toScene(event);
      setNodes(dragging, { x: point.x, y: point.y });
      return;
    }
    if (!panning) return;
    const scale = svgRef.getBoundingClientRect().width / VIEW_WIDTH || 1;
    setPan({
      x: panning.x + (event.clientX - panning.origin.x) / scale,
      y: panning.y + (event.clientY - panning.origin.y) / scale,
    });
  };

  const onPointerUp = (event: PointerEvent) => {
    dragging = -1;
    panning = null;
    if (svgRef.hasPointerCapture(event.pointerId)) svgRef.releasePointerCapture(event.pointerId);
  };

  const focusEntity = (name: string) => {
    setTerm(name === "*" ? "" : name);
    setFocus(name);
  };

  const detail = createMemo(() => (selected() >= 0 ? nodes[selected()] : undefined));

  return (
    <section class="page-view graph-page">
      <div class="page-heading page-heading-row">
        <div>
          <span>Kho tri thức</span>
          <h1>Đồ thị tri thức</h1>
          <p>
            Thực thể và quan hệ mà Private AI rút ra từ tài liệu của{" "}
            <strong>{props.workspaceName}</strong>. Kéo để di chuyển, lăn chuột để phóng to, bấm một
            node để xem chi tiết.
          </p>
        </div>
        <div class="page-heading-actions">
          <Show when={focus() !== "*"}>
            <button class="button button-secondary" type="button" onClick={() => focusEntity("*")}>
              <Layers size={17} /> Toàn bộ đồ thị
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
          <For each={suggestions() ?? []}>{(entity) => <option value={entity.name} />}</For>
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
          <button class="icon-button" type="button" onClick={() => zoomByButton(1 / 1.2)} aria-label="Thu nhỏ">
            <ZoomOut size={17} />
          </button>
          <button class="icon-button" type="button" onClick={() => zoomByButton(1.2)} aria-label="Phóng to">
            <ZoomIn size={17} />
          </button>
          <button class="icon-button" type="button" onClick={fitToContent} aria-label="Vừa khung hình">
            <Focus size={17} />
          </button>
        </div>
      </div>

      <div class="graph-layout">
        <div class="graph-canvas">
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
            <Match when={nodes.length === 0}>
              <p class="graph-empty">
                Chưa có thực thể nào trong không gian này. Tải tài liệu lên rồi chờ lập chỉ mục xong,
                đồ thị sẽ tự có dữ liệu.
              </p>
            </Match>
            <Match when={nodes.length > 0}>
              <svg
                ref={svgRef}
                class="graph-svg"
                viewBox={`0 0 ${VIEW_WIDTH} ${VIEW_HEIGHT}`}
                role="img"
                aria-label={`Đồ thị tri thức với ${nodes.length} thực thể`}
                onWheel={onWheel}
                onPointerDown={onBackgroundDown}
                onPointerMove={onPointerMove}
                onPointerUp={onPointerUp}
                onPointerCancel={onPointerUp}
              >
                <g ref={sceneRef} transform={`translate(${pan().x} ${pan().y}) scale(${zoom()})`}>
                  <g class="graph-edges">
                    <For each={edges()}>
                      {(edge) => (
                        <line
                          x1={nodes[edge.source].x}
                          y1={nodes[edge.source].y}
                          x2={nodes[edge.target].x}
                          y2={nodes[edge.target].y}
                          opacity={edgeOpacity(edge)}
                        />
                      )}
                    </For>
                  </g>
                  <g class="graph-nodes">
                    <For each={nodes}>
                      {(node, index) => (
                        <g
                          class="graph-node"
                          classList={{ selected: selected() === index() }}
                          opacity={nodeOpacity(index())}
                          transform={`translate(${node.x} ${node.y})`}
                          onPointerDown={(event) => onNodeDown(index(), event)}
                          onPointerEnter={() => setHovered(index())}
                          onPointerLeave={() => setHovered(-1)}
                          onDblClick={() => focusEntity(node.id)}
                        >
                          <circle r={radiusOf(node.degree)} fill={colorOf(node.type)} />
                          <text y={radiusOf(node.degree) + 14} text-anchor="middle">
                            {node.label.length > 26 ? `${node.label.slice(0, 25)}…` : node.label}
                          </text>
                        </g>
                      )}
                    </For>
                  </g>
                </g>
              </svg>
            </Match>
          </Switch>

          <Show when={nodes.length > 0}>
            <div class="graph-status">
              <span>
                <strong>{nodes.length}</strong> thực thể · <strong>{edges().length}</strong> quan hệ
                <Show when={settling()}> · đang sắp xếp…</Show>
              </span>
              <Show when={(snapshot() as GraphSnapshot | undefined)?.truncated}>
                <em>Đồ thị đã bị cắt bớt, tăng “Số node” để xem thêm.</em>
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
                    onClick={() => setMuted(type, !muted[type])}
                  >
                    <i style={{ background: colorOf(type) }} />
                    <span>{type}</span>
                    <em>{count}</em>
                  </button>
                )}
              </For>
            </div>
          </Show>

          <Show
            when={detail()}
            fallback={
              <p class="graph-hint">
                <Share2 size={16} /> Bấm vào một node để xem mô tả, hoặc bấm đúp để chỉ xem lân cận
                của nó.
              </p>
            }
          >
            {(node) => (
              <article class="graph-detail">
                <header>
                  <div>
                    <span style={{ color: colorOf(node().type) }}>{node().type}</span>
                    <h2>{node().label}</h2>
                  </div>
                  <button class="icon-button" type="button" onClick={() => setSelected(-1)} aria-label="Đóng">
                    <X size={17} />
                  </button>
                </header>
                <p class="graph-detail-meta">
                  {node().degree} quan hệ
                  <Show when={node().source}> · nguồn: {node().source}</Show>
                </p>
                <Show when={node().description}>
                  <p class="graph-detail-body">{node().description}</p>
                </Show>
                <button class="button button-secondary" type="button" onClick={() => focusEntity(node().id)}>
                  <Focus size={17} /> Chỉ xem lân cận
                </button>
              </article>
            )}
          </Show>
        </aside>
      </div>
    </section>
  );
}
