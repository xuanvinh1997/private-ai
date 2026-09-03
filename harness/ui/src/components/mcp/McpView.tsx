import { Key } from "@solid-primitives/keyed";
import { createSignal, For, onMount, Show } from "solid-js";
import {
  listMcpServers,
  mcpCatalog,
  reloadMcpServers,
  removeMcpServer,
  saveMcpServer,
  setMcpEnabled,
} from "../../lib/mcp";
import type { McpCatalogEntry, McpServer, McpServerInput, McpState } from "../../lib/protocol";
import Icon from "./../Icon";
import { IconButton } from "./../primitives";
import ConfirmDialog from "./../providers/ConfirmDialog";
import { Banner, Button, InfoDot, Row, RowGroup, SectionHead, Toggle } from "../settings/FormKit";
import McpCatalog from "./McpCatalog";
import McpForm from "./McpForm";

type Sheet =
  | { kind: "none" }
  | { kind: "catalog" }
  | { kind: "form"; server: McpServer | null }
  | { kind: "delete"; server: McpServer };

const STATE_LABEL: Record<McpState, string> = {
  connected: "đã nối",
  connecting: "đang nối",
  failed: "hỏng",
  disabled: "đang tắt",
};

/**
 * Màn hình server MCP.
 *
 * MCP là chỗ người dùng tự tay mở rộng năng lực của trợ lý, nên nó cũng là chỗ họ tự tay
 * mở rộng bề mặt tấn công. Ba quyết định dưới đây đều đến từ chỗ đó:
 *
 *   - Câu về **nội dung không đáng tin** đứng trên đầu trang và không gập lại được. Nó là
 *     chính sách của lõi, không phải một lời khuyên; giấu nó sau một chú giải là để người
 *     dùng cắm một server lạ mà không biết mình đang cho ai nói vào cuộc hội thoại.
 *   - Server `failed` hiện `error` **nguyên văn ngay trong hàng**. Một server hỏng mà
 *     không nói vì sao là một server không sửa được, và người dùng sẽ xoá nó đi thay vì
 *     cài Node.
 *   - Danh sách tool hiện **tên đã mang tiền tố** `ext.<server>.`. Đó là tên mô hình thật
 *     sự thấy và là tên xuất hiện trong bản ghi; hiện tên từ xa thì bảng này không tra
 *     ngược được từ một lượt gọi đã xảy ra.
 */
export default function McpView() {
  const [servers, setServers] = createSignal<McpServer[]>([]);
  const [catalog, setCatalog] = createSignal<McpCatalogEntry[]>([]);
  const [ready, setReady] = createSignal(false);
  const [sheet, setSheet] = createSignal<Sheet>({ kind: "none" });
  const [busy, setBusy] = createSignal(false);
  const [reloading, setReloading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [formError, setFormError] = createSignal<string | null>(null);
  const [open, setOpen] = createSignal(new Set<string>());

  onMount(() => {
    void (async () => {
      const [list, entries] = await Promise.all([listMcpServers(), mcpCatalog()]);
      setServers(list);
      setCatalog(entries);
      setReady(true);
    })();
  });

  const act = async (what: string, run: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await run();
      setServers(await listMcpServers());
    } catch (err) {
      setError(`${what}: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const reload = async () => {
    setReloading(true);
    setError(null);
    try {
      setServers(await reloadMcpServers());
    } catch (err) {
      setError(`Không nạp lại được: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setReloading(false);
    }
  };

  const submit = async (input: McpServerInput) => {
    setBusy(true);
    setFormError(null);
    try {
      await saveMcpServer(input);
      setServers(await listMcpServers());
      setSheet({ kind: "none" });
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  // Bóc tách kiểu hợp nhất một lần, thay vì lồng hai `<Show>` chỉ để thu hẹp kiểu.
  const formSheet = () => {
    const current = sheet();
    return current.kind === "form" ? current : null;
  };
  const deleteTarget = () => {
    const current = sheet();
    return current.kind === "delete" ? current.server : null;
  };

  const toggleOpen = (name: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (!next.delete(name)) next.add(name);
      return next;
    });

  return (
    <div class="flex flex-col gap-2xl">
      <SectionHead
        icon="plug"
        title="Server đang cắm"
        desc="Cắm công cụ ngoài: kho mã, cơ sở dữ liệu."
        actions={() => (
          <>
            <Button
              label={reloading() ? "Đang nạp lại…" : "Nạp lại"}
              variant="outline"
              icon="refresh"
              busy={reloading()}
              onClick={() => void reload()}
            />
            <Button label="Thêm server" icon="plus" onClick={() => setSheet({ kind: "catalog" })} />
          </>
        )}
      />

      {/* Không gập lại được, và không phải một chú giải. Đây là chính sách của lõi. */}
      <Banner
        tone="warn"
        icon="warn"
        title="Server MCP trả về nội dung không đáng tin"
        more="Mọi thứ một server MCP trả về đều được lõi đóng khung là dữ liệu bên ngoài, và mọi tool của nó luôn bị coi là có thể thay đổi trạng thái — nên chúng đi qua bước hỏi duyệt, kể cả khi tên tool nghe như chỉ đọc. Cắm một server là cho tác giả của nó nói vào cuộc hội thoại của bạn; chỉ cắm cái bạn tin."
      >
        Chỉ cắm server bạn tin.
      </Banner>

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title="Không làm được">
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={ready()} fallback={<Skeleton />}>
        <Show
          when={servers().length > 0}
          fallback={
            <div class="flex flex-col items-start gap-md rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-2xl">
              <p class="m-0 max-w-[52ch] text-xs text-muted">
                Chưa cắm server nào — MCP thêm tool ngoài dự án.
              </p>
              <Button label="Mở danh mục" icon="plug" onClick={() => setSheet({ kind: "catalog" })} />
            </div>
          }
        >
          <RowGroup>
            {/* Keyed theo tên: danh sách được nạp lại sau mỗi thao tác, và `<For>` khớp
                theo vị trí sẽ dựng lại mọi hàng — công tắc đang giữ tiêu điểm mất nó. */}
            <Key each={servers()} by="name">
              {(entry) => (
                <ServerRow
                  server={entry()}
                  busy={busy()}
                  open={open().has(entry().name)}
                  onToggleOpen={() => toggleOpen(entry().name)}
                  onToggle={(next) =>
                    void act("Không đổi được trạng thái", () => setMcpEnabled(entry().name, next))
                  }
                  onEdit={() => {
                    setFormError(null);
                    setSheet({ kind: "form", server: entry() });
                  }}
                  onDelete={() => setSheet({ kind: "delete", server: entry() })}
                />
              )}
            </Key>
          </RowGroup>
        </Show>
      </Show>

      <Show when={sheet().kind === "catalog"}>
        <McpCatalog
          entries={catalog()}
          busy={busy()}
          error={formError()}
          onInstall={(input) => void submit(input)}
          onManual={() => {
            setFormError(null);
            setSheet({ kind: "form", server: null });
          }}
          onClose={() => setSheet({ kind: "none" })}
        />
      </Show>

      <Show when={formSheet()} keyed>
        {(form) => (
          <McpForm
            server={form.server}
            busy={busy()}
            error={formError()}
            onSubmit={(input) => void submit(input)}
            onClose={() => setSheet({ kind: "none" })}
          />
        )}
      </Show>

      <Show when={deleteTarget()} keyed>
        {(target) => (
          <ConfirmDialog
            title={`Xoá server ${target.name}?`}
            body={`Xoá hẳn cấu hình, biến môi trường và ${target.tools.length} tool.`}
            more={`Cấu hình và mọi biến môi trường của nó bị xoá khỏi máy, và ${target.tools.length} tool biến mất khỏi trợ lý. Không hoàn tác được.`}
            detail={target.target}
            confirmLabel="Xoá server"
            busy={busy()}
            onConfirm={() =>
              void act("Không xoá được server", async () => {
                await removeMcpServer(target.name);
                setSheet({ kind: "none" });
              })
            }
            onClose={() => setSheet({ kind: "none" })}
          />
        )}
      </Show>
    </div>
  );
}

/**
 * Chấm trạng thái.
 *
 * Không dùng `StateDot` của `primitives.tsx` vì bảng trạng thái ở đây có bốn giá trị và
 * cái thứ tư — `disabled` — không có trong ba giá trị của nó. Ép `disabled` thành "ok"
 * hay "warn" sẽ vẽ một server đang nằm im bằng cùng một màu với một server đang chạy.
 */
function StateDot(props: { state: McpState }) {
  return (
    <span
      role="img"
      aria-label={STATE_LABEL[props.state]}
      title={STATE_LABEL[props.state]}
      class="size-2 shrink-0 rounded-pill"
      classList={{
        "bg-success": props.state === "connected",
        // Đang nối thì thở nhẹ: một server treo và một server đã xong trông giống hệt
        // nhau nếu chấm đứng im.
        "bg-muted motion-safe:animate-pulse": props.state === "connecting",
        "bg-danger": props.state === "failed",
        "bg-line-strong": props.state === "disabled",
      }}
    />
  );
}

function ServerRow(props: {
  server: McpServer;
  busy: boolean;
  open: boolean;
  onToggleOpen: () => void;
  onToggle: (next: boolean) => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const count = () => props.server.tools.length;

  return (
    <Row
      label={props.server.name}
      labelMono
      dim={!props.server.enabled}
      lead={() => <StateDot state={props.server.state} />}
      control={() => (
        <>
          <Toggle
            label={`${props.server.enabled ? "Tắt" : "Bật"} server ${props.server.name}`}
            checked={props.server.enabled}
            busy={props.busy}
            onChange={props.onToggle}
          />
          <IconButton icon="pencil" label={`Sửa ${props.server.name}`} size="sm" onClick={props.onEdit} />
          <IconButton
            icon="trash"
            label={`Xoá ${props.server.name}`}
            size="sm"
            danger
            onClick={props.onDelete}
          />
        </>
      )}
      below={() => (
        <>
          <div class="flex min-w-0 flex-wrap items-center gap-2xs">
            <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
              <Icon name={props.server.transport === "http" ? "cloud" : "terminal"} size={10} />
              {props.server.transport === "http" ? "HTTP" : "stdio"}
            </span>
            <span
              class="inline-flex shrink-0 items-center rounded-pill px-2xs py-3xs text-2xs"
              classList={{
                "bg-accent-soft text-accent-ink": props.server.state === "connected",
                "bg-danger-soft text-danger": props.server.state === "failed",
                "bg-[var(--overlay-faint)] text-muted":
                  props.server.state !== "connected" && props.server.state !== "failed",
              }}
            >
              {STATE_LABEL[props.server.state]}
            </span>
            <Show when={props.server.state === "connected"}>
              <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs tabular-nums text-muted">
                <Icon name="tools" size={10} />
                {count()} tool
              </span>
            </Show>
            <span class="min-w-0 truncate font-mono text-2xs text-faint" title={props.server.target}>
              {props.server.target}
            </span>
          </div>

          {/* Lỗi hiện ngay trong hàng, không nằm sau cú bấm mở rộng: thứ duy nhất người
              dùng cần khi thấy một chấm đỏ là câu trả lời "vì sao". Trong bố cục hàng gọn
              nó còn gánh thêm việc của cái viền đỏ cũ — nó là dấu hiệu duy nhất còn lại. */}
          <Show when={props.server.state === "failed" && props.server.error}>
            {(message) => (
              <p
                role="alert"
                class="m-0 overflow-x-auto rounded-panel border border-danger bg-danger-soft px-sm py-2xs font-mono text-2xs whitespace-pre-wrap text-danger"
              >
                {message()}
              </p>
            )}
          </Show>

          {/* Nối được mà không có tool nào là một trạng thái im lặng: chấm xanh, không
              lỗi, và tuyệt đối không thêm gì cho trợ lý. Nói ra, nếu không người dùng sẽ
              đi tìm lý do ở phía mô hình. */}
          <Show when={props.server.state === "connected" && count() === 0}>
            <p class="m-0 inline-flex items-center gap-2xs text-2xs text-muted">
              Nối được nhưng không có tool nào.
              <InfoDot
                label="Vì sao server không có tool"
                text="Server nối được nhưng không khai báo tool nào, nên nó chưa thêm gì cho trợ lý. Kiểm tra lại tham số dòng lệnh hoặc quyền của token."
              />
            </p>
          </Show>

          <Show when={count() > 0}>
            <div class="flex flex-col gap-2xs">
              <button
                type="button"
                onClick={props.onToggleOpen}
                aria-expanded={props.open}
                class="flex items-center gap-2xs self-start rounded-btn px-2xs py-3xs text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
              >
                <Icon
                  name="chevron-right"
                  size={12}
                  class={`transition-transform duration-[var(--dur-fast)] ${props.open ? "rotate-90" : ""}`}
                />
                {props.open ? "Ẩn danh sách tool" : `Xem ${count()} tool đã cắm`}
              </button>

              <Show when={props.open}>
                <div class="overflow-x-auto rounded-panel border border-line bg-surface-soft p-sm">
                  <p class="m-0 mb-2xs text-2xs text-faint">
                    Đây là tên mô hình thấy, và tên trong bản ghi.
                  </p>
                  <ul class="m-0 flex list-none flex-col gap-3xs p-0">
                    <For each={props.server.tools}>
                      {(tool) => (
                        <li class="font-mono text-2xs whitespace-nowrap text-text">{tool}</li>
                      )}
                    </For>
                  </ul>
                </div>
              </Show>
            </div>
          </Show>
        </>
      )}
    />
  );
}

function Skeleton() {
  return (
    <div
      class="flex flex-col divide-y divide-line rounded-card border border-line bg-surface"
      aria-hidden="true"
    >
      <For each={[0, 1, 2]}>
        {() => (
          <div class="flex flex-col gap-2xs px-(--card-pad-x) py-sm">
            <span class="h-3 w-1/4 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
            <span class="h-2.5 w-2/3 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
          </div>
        )}
      </For>
    </div>
  );
}
