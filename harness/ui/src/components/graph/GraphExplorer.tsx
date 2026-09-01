import { createMemo, createResource, createSignal, For, Match, Show, Switch } from "solid-js";
import {
  DIRECTION_LABEL,
  EDGE_KIND_LABEL,
  incidentEdges,
  indexStats,
  loadGraphView,
  NODE_KIND_LABEL,
  viewToMermaid,
  type GraphDirection,
  type Incident,
} from "../../lib/graph";
import type { GraphNode, GraphView, IndexStats } from "../../lib/protocol";
import Icon from "../Icon";
import Diagram from "../markdown/Diagram";
import { Chip } from "../primitives";

const DEPTHS = [1, 2, 3];

/**
 * Màn hình đi trong đồ thị bộ nhớ mã nguồn.
 *
 * Hai nguồn dữ liệu đi vào bằng prop có mặc định, giống `CodeBrowser`: màn hình không tự
 * biết "thật hay demo", và `onOpenFile` là callback của bên ngoài — trình xem mã nguồn
 * không thuộc về đây, và một màn hình tự mở tệp là một màn hình thứ hai biết cách mở tệp.
 */
export default function GraphExplorer(props: {
  projectName: string;
  onOpenFile: (path: string, line?: number) => void;
  loadStats?: () => Promise<IndexStats>;
  loadView?: (symbol: string, direction: GraphDirection, depth: number) => Promise<GraphView>;
}) {
  const [query, setQuery] = createSignal("");
  const [target, setTarget] = createSignal<string | null>(null);
  const [direction, setDirection] = createSignal<GraphDirection>("both");
  const [depth, setDepth] = createSignal(1);

  const [stats] = createResource(() => (props.loadStats ?? indexStats)());

  const [view, { refetch }] = createResource(
    () => {
      const symbol = target();
      return symbol === null ? null : { symbol, direction: direction(), depth: depth() };
    },
    (input) => (props.loadView ?? loadGraphView)(input.symbol, input.direction, input.depth),
  );

  /**
   * Đỉnh đang đứng.
   *
   * Ưu tiên khớp id — đó là lúc người dùng vừa bấm một đỉnh trong danh sách. Còn khi họ
   * gõ tay thì lõi khớp theo tên, nên ta khớp theo tên đúng cách lõi làm và không giả
   * vờ chắc chắn hơn nó.
   */
  const focus = createMemo<GraphNode | null>(() => {
    const data = view();
    const symbol = target();
    if (data === undefined || symbol === null || data.nodes.length === 0) return null;
    const needle = symbol.toLowerCase();
    return (
      data.nodes.find((node) => node.id === symbol) ??
      data.nodes.find((node) => node.name.toLowerCase() === needle) ??
      data.nodes.find((node) => node.name.toLowerCase().includes(needle)) ??
      data.nodes[0] ??
      null
    );
  });

  /** Nhiều ký hiệu cùng khớp một cái tên là chuyện thường — cho chọn thay vì đoán hộ. */
  const matches = createMemo<GraphNode[]>(() => {
    const data = view();
    const symbol = target();
    if (data === undefined || symbol === null) return [];
    const needle = symbol.toLowerCase();
    const hits = data.nodes.filter((node) => node.name.toLowerCase().includes(needle));
    return hits.length > 1 ? hits : [];
  });

  const incidents = createMemo<Incident[]>(() => {
    const data = view();
    const node = focus();
    return data === undefined || node === null ? [] : incidentEdges(data, node.id);
  });

  const source = createMemo(() => {
    const data = view();
    return data === undefined ? "" : viewToMermaid(data, focus()?.id);
  });

  const go = (symbol: string): void => {
    setTarget(symbol);
  };

  const submit = (event: SubmitEvent): void => {
    event.preventDefault();
    const text = query().trim();
    if (text !== "") go(text);
  };

  return (
    <div class="flex min-h-0 flex-1 flex-col">
      <header class="flex h-(--header-h) shrink-0 items-center gap-sm border-b border-line px-(--page-pad-x)">
        <span class="shrink-0 text-accent">
          <Icon name="graph" size={16} />
        </span>
        <h2 class="m-0 shrink-0 text-xs font-semibold text-ink">Đồ thị mã nguồn</h2>
        <span class="min-w-0 truncate text-2xs text-faint" title={props.projectName}>
          {props.projectName}
        </span>
      </header>

      <div class="flex min-h-0 flex-1">
        <aside
          aria-label="Tìm ký hiệu"
          class="flex w-(--tree-col-w) shrink-0 flex-col gap-md overflow-y-auto border-r border-line bg-sidebar px-md py-md"
        >
          <form onSubmit={submit} class="flex flex-col gap-2xs">
            <label for="graph-query" class="text-2xs font-medium text-ink">
              Ký hiệu
            </label>
            <div class="flex items-center gap-2xs">
              <input
                id="graph-query"
                type="search"
                value={query()}
                onInput={(event) => setQuery(event.currentTarget.value)}
                placeholder="retry_with_backoff"
                spellcheck={false}
                class="h-(--control-h) min-w-0 flex-1 rounded-btn border border-line bg-surface px-sm font-mono text-xs text-text placeholder:text-faint"
              />
              <button
                type="submit"
                class="h-(--control-h) shrink-0 rounded-btn bg-accent px-md text-2xs font-medium text-on-accent transition-colors duration-[var(--dur-fast)] hover:bg-accent-hover"
              >
                Tra
              </button>
            </div>
          </form>

          <Picker
            label="Chiều"
            options={(["both", "callers", "callees"] as const).map((id) => ({
              id,
              label: DIRECTION_LABEL[id],
            }))}
            value={direction()}
            onPick={setDirection}
          />
          <Picker
            label="Độ sâu"
            options={DEPTHS.map((n) => ({ id: n, label: String(n) }))}
            value={depth()}
            onPick={setDepth}
          />

          <Stats stats={stats()} loading={stats.loading} />
        </aside>

        <section
          aria-label="Lân cận của ký hiệu"
          aria-busy={view.loading}
          class="flex min-h-0 min-w-0 flex-1 flex-col gap-md overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)"
        >
          <Caveat />

          <Switch>
            <Match when={target() === null}>
              <p class="m-0 text-sm text-muted">
                Gõ tên một hàm, một struct hay một mô-đun rồi bấm “Tra” để xem nó nối với
                những gì.
              </p>
            </Match>

            <Match when={view.error}>
              {(err) => (
                <div class="flex flex-col items-start gap-2xs rounded-card border border-line bg-danger-soft px-(--card-pad-x) py-(--card-pad-y)">
                  <p class="m-0 text-xs text-danger">
                    Không tra được đồ thị: {String((err() as Error).message ?? err())}
                  </p>
                  <button
                    type="button"
                    onClick={() => void refetch()}
                    class="rounded-btn border border-line bg-surface px-md py-3xs text-2xs text-text transition-colors duration-[var(--dur-fast)] hover:bg-surface-hover"
                  >
                    Thử lại
                  </button>
                </div>
              )}
            </Match>

            <Match when={view.loading}>
              <p class="m-0 text-sm text-faint">Đang tra đồ thị…</p>
            </Match>

            <Match when={view()?.nodes.length === 0}>
              <p class="m-0 text-sm text-muted">
                Không có ký hiệu nào khớp “{target()}”. Chỉ mục khớp theo tên, nên tên
                viết tắt hoặc tên có tiền tố mô-đun thường trượt.
              </p>
            </Match>

            <Match when={focus()}>
              {(node) => (
                <>
                  <Show when={matches().length > 0}>
                    <section class="flex flex-col gap-2xs">
                      <h3 class="m-0 text-2xs font-medium text-muted">
                        {matches().length} ký hiệu cùng khớp — chọn một
                      </h3>
                      <ul class="m-0 flex list-none flex-col gap-3xs p-0">
                        <For each={matches()}>
                          {(hit) => (
                            <NodeRow
                              node={hit}
                              current={hit.id === node().id}
                              // Gửi **tên**, không gửi id: lõi phân giải ký hiệu theo tên,
                              // và một id gửi lên sẽ không khớp gì cả rồi trả về đồ thị
                              // rỗng — trông y hệt một ký hiệu không có cạnh nào.
                              onGo={() => go(hit.name)}
                              onOpen={() => props.onOpenFile(hit.path, hit.line)}
                            />
                          )}
                        </For>
                      </ul>
                    </section>
                  </Show>

                  <FocusCard node={node()} onOpen={() => props.onOpenFile(node().path, node().line)} />

                  <Show when={view()?.truncated === true}>
                    <Truncated />
                  </Show>

                  <div class="grid grid-cols-1 gap-md xl:grid-cols-[minmax(0,1fr)_var(--changes-col-w)]">
                    <Show
                      when={source() !== ""}
                      fallback={
                        <p class="m-0 text-xs text-faint">Không có cạnh nào để vẽ.</p>
                      }
                    >
                      <Diagram source={source()} />
                    </Show>

                    {/* Danh sách chữ **luôn** đứng cạnh hình, không phải một lối xem thay
                        thế. SVG mermaid không có chỗ gắn sự kiện đáng tin, nên hình chỉ
                        để nhìn — còn đi tiếp sang đỉnh khác là việc của danh sách này. */}
                    <EdgeList
                      incidents={incidents()}
                      onGo={go}
                      onOpen={(path, line) => props.onOpenFile(path, line)}
                    />
                  </div>
                </>
              )}
            </Match>
          </Switch>
        </section>
      </div>
    </div>
  );
}

/**
 * Câu cảnh báo đứng **trên** kết quả, không phải trong một chú giải gập lại.
 *
 * Lõi phân giải lời gọi bằng cách khớp tên, không phân tích kiểu. Một đồ thị trình bày
 * như sự thật khiến người ta kết luận sai mà lại tự tin — và một dòng chữ ở chỗ không ai
 * mở ra thì không cứu được ai.
 */
function Caveat() {
  return (
    <p class="m-0 flex items-start gap-2xs rounded-card border border-line bg-warn-soft px-(--card-pad-x) py-2xs text-2xs text-warn">
      <span class="mt-3xs shrink-0">
        <Icon name="warn" size={13} />
      </span>
      <span>
        Đồ thị này là <strong class="font-semibold">suy đoán theo tên</strong>, không phải
        sự thật: lõi nối lời gọi bằng cách khớp tên ký hiệu chứ không phân tích kiểu. Hai
        hàm trùng tên ở hai mô-đun sẽ hiện ra như một, và lời gọi qua trait object, con
        trỏ hàm hay phản chiếu sẽ không có cạnh nào. Kiểm lại ở mã nguồn trước khi kết luận.
      </span>
    </p>
  );
}

function Truncated() {
  return (
    <p class="m-0 flex items-start gap-2xs rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-2xs text-2xs text-muted">
      <span class="mt-3xs shrink-0 text-warn">
        <Icon name="warn" size={13} />
      </span>
      <span>
        Đã cắt bớt lân cận. Đỉnh này có quá nhiều cạnh để vẽ — một đỉnh bốn trăm cạnh dựng
        ra là một quả cầu đen không đọc được gì. Để xem hẹp hơn: giảm độ sâu xuống 1, hoặc
        đổi chiều sang “{DIRECTION_LABEL.callers}” hay “{DIRECTION_LABEL.callees}” để chỉ
        lấy một phía.
      </span>
    </p>
  );
}

function FocusCard(props: { node: GraphNode; onOpen: () => void }) {
  return (
    <section class="flex flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
      <div class="flex min-w-0 items-center gap-sm">
        <Chip tone="accent">{NODE_KIND_LABEL[props.node.kind]}</Chip>
        <h3
          class="m-0 min-w-0 flex-1 truncate font-mono text-sm text-ink"
          title={props.node.name}
        >
          {props.node.name}
        </h3>
      </div>
      <PathButton path={props.node.path} line={props.node.line} onOpen={props.onOpen} />
    </section>
  );
}

function EdgeList(props: {
  incidents: Incident[];
  onGo: (id: string) => void;
  onOpen: (path: string, line: number) => void;
}) {
  return (
    <section aria-label="Cạnh của ký hiệu" class="flex min-w-0 flex-col gap-2xs">
      <h3 class="m-0 text-2xs font-medium text-muted">Cạnh ({props.incidents.length})</h3>
      <Show
        when={props.incidents.length > 0}
        fallback={<p class="m-0 text-2xs text-faint">Không có cạnh nào chạm vào đỉnh này.</p>}
      >
        <ul class="m-0 flex list-none flex-col gap-3xs p-0">
          <For each={props.incidents}>
            {(item) => (
              <NodeRow
                node={item.other}
                relation={`${item.outgoing ? "→" : "←"} ${EDGE_KIND_LABEL[item.edge.kind]}`}
                onGo={() => props.onGo(item.other.id)}
                onOpen={() => props.onOpen(item.other.path, item.other.line)}
              />
            )}
          </For>
        </ul>
      </Show>
    </section>
  );
}

/**
 * Một đỉnh trong danh sách: bấm thân để đi tiếp, bấm đường dẫn để mở tệp.
 *
 * Hai đích đến khác nhau nên là hai nút khác nhau, lồng nhau thì bàn phím không tới được
 * cái bên trong và chuột thì bấm trúng cái nào là chuyện của vài pixel.
 */
function NodeRow(props: {
  node: GraphNode;
  relation?: string;
  current?: boolean;
  onGo: () => void;
  onOpen: () => void;
}) {
  return (
    <li
      class="flex flex-col gap-3xs rounded-panel border px-sm py-2xs transition-colors duration-[var(--dur-fast)]"
      classList={{
        "border-line bg-surface": props.current !== true,
        "border-accent bg-accent-soft": props.current === true,
      }}
    >
      <button
        type="button"
        onClick={props.onGo}
        title={`Đi tới ${props.node.name}`}
        class="flex min-w-0 items-center gap-2xs text-left"
      >
        <Show when={props.relation}>
          <span class="shrink-0 tabular-nums text-2xs text-faint">{props.relation}</span>
        </Show>
        <span class="min-w-0 flex-1 truncate font-mono text-xs text-accent-ink">
          {props.node.name}
        </span>
        <span class="shrink-0 text-2xs text-faint">{NODE_KIND_LABEL[props.node.kind]}</span>
      </button>
      <PathButton path={props.node.path} line={props.node.line} onOpen={props.onOpen} />
    </li>
  );
}

function PathButton(props: { path: string; line: number; onOpen: () => void }) {
  return (
    <button
      type="button"
      onClick={props.onOpen}
      title={`Mở ${props.path} ở dòng ${props.line}`}
      class="min-w-0 truncate rounded-btn text-left font-mono text-2xs text-muted underline decoration-transparent underline-offset-2 transition-colors duration-[var(--dur-fast)] hover:text-ink hover:decoration-current"
      dir="rtl"
    >
      <bdi>
        {props.path}:{props.line}
      </bdi>
    </button>
  );
}

function Picker<T extends string | number>(props: {
  label: string;
  options: { id: T; label: string }[];
  value: T;
  onPick: (value: T) => void;
}) {
  return (
    <div class="flex flex-col gap-2xs">
      <span class="text-2xs font-medium text-ink">{props.label}</span>
      <div role="radiogroup" aria-label={props.label} class="flex flex-wrap gap-3xs">
        <For each={props.options}>
          {(option) => (
            <button
              type="button"
              role="radio"
              aria-checked={props.value === option.id}
              onClick={() => props.onPick(option.id)}
              class="rounded-pill border px-sm py-3xs text-2xs transition-colors duration-[var(--dur-fast)]"
              classList={{
                "border-line text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
                  props.value !== option.id,
                "border-accent bg-accent-soft text-accent-ink": props.value === option.id,
              }}
            >
              {option.label}
            </button>
          )}
        </For>
      </div>
    </div>
  );
}

/**
 * Dải thống kê chỉ mục.
 *
 * Một chỉ mục quét dở có số cạnh bằng 0 vì cạnh được dựng ở lượt cuối, không phải vì mã
 * nguồn không có lời gọi nào. Hiện "0" ở đó là nói dối bằng một con số đúng, nên khi
 * chưa quét xong thì ô đó ghi "chưa đếm".
 */
function Stats(props: { stats: IndexStats | undefined; loading: boolean }) {
  const done = () => props.stats?.scannedAt !== null && props.stats !== undefined;
  const num = (value: number | undefined): string => {
    if (value === undefined) return "—";
    if (!done() && value === 0) return "chưa đếm";
    return value.toLocaleString("vi-VN");
  };

  return (
    <section aria-label="Thống kê chỉ mục" aria-busy={props.loading} class="flex flex-col gap-2xs">
      <span class="text-2xs font-medium text-ink">Chỉ mục</span>
      <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-sm gap-y-3xs text-2xs">
        <Stat label="Tệp" value={num(props.stats?.files)} />
        <Stat label="Ký hiệu" value={num(props.stats?.symbols)} />
        <Stat label="Cạnh" value={num(props.stats?.edges)} />
        <Stat label="Quét lúc" value={when(props.stats?.scannedAt ?? null)} />
      </dl>

      <Show when={(props.stats?.languages.length ?? 0) > 0}>
        <div class="flex flex-wrap gap-3xs">
          <For each={props.stats?.languages ?? []}>
            {(entry) => (
              <Chip>
                {entry[0]} · {entry[1].toLocaleString("vi-VN")}
              </Chip>
            )}
          </For>
        </div>
      </Show>

      <Show when={props.stats !== undefined && props.stats.scannedAt === null}>
        <p class="m-0 text-2xs text-warn">
          Chỉ mục chưa dựng xong. Những gì hiện ra dưới đây là phần đã quét được, không
          phải toàn bộ dự án.
        </p>
      </Show>
    </section>
  );
}

function Stat(props: { label: string; value: string }) {
  return (
    <>
      <dt class="text-faint">{props.label}</dt>
      <dd class="m-0 tabular-nums text-text">{props.value}</dd>
    </>
  );
}

function when(at: number | null): string {
  if (at === null) return "chưa xong";
  return new Date(at).toLocaleString("vi-VN", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}
