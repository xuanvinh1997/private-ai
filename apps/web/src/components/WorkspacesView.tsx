import { LayoutGrid, MessageSquareText, Pencil, Plus, Search, Trash2 } from "lucide-solid";
import { For, Match, Show, Switch, createMemo, createSignal } from "solid-js";
import { api } from "../api";
import { WorkspaceDialog } from "./DataViews";
import { formatRelativeTime } from "../format";
import type { WorkspaceRecord } from "../types";

export function WorkspacesView(props: {
  workspaces: WorkspaceRecord[];
  activeId: string;
  loading: boolean;
  onOpen: (id: string) => void;
  onSaved: (workspace: WorkspaceRecord, created: boolean) => void;
  onDeleted: (id: string) => void;
}) {
  const [search, setSearch] = createSignal("");
  const [confirmDelete, setConfirmDelete] = createSignal("");
  const [workingId, setWorkingId] = createSignal("");
  const [error, setError] = createSignal("");

  const filtered = createMemo(() => {
    const term = search().trim().toLowerCase();
    if (!term) return props.workspaces;
    return props.workspaces.filter((workspace) =>
      `${workspace.name} ${workspace.description}`.toLowerCase().includes(term),
    );
  });

  const totalConversations = createMemo(() =>
    props.workspaces.reduce((sum, workspace) => sum + workspace.conversation_count, 0),
  );

  const remove = async (workspace: WorkspaceRecord) => {
    if (confirmDelete() !== workspace.id) {
      setConfirmDelete(workspace.id);
      return;
    }
    setWorkingId(workspace.id);
    setError("");
    try {
      await api.deleteWorkspace(workspace.id);
      setConfirmDelete("");
      props.onDeleted(workspace.id);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không thể xóa không gian làm việc");
    } finally {
      setWorkingId("");
    }
  };

  return (
    <section class="page-view">
      <div class="page-heading page-heading-row">
        <div>
          <span>Không gian làm việc</span>
          <h1>Quản lý không gian</h1>
          <p class="workspace-page-stats">
            <Show
              when={props.workspaces.length > 0}
              fallback="Tạo không gian đầu tiên để nhóm các cuộc trò chuyện và tài liệu."
            >
              <strong>{props.workspaces.length}</strong> không gian
              <em>·</em> {totalConversations()} cuộc trò chuyện
            </Show>
          </p>
        </div>
        <WorkspaceDialog
          trigger="add"
          triggerClass="button button-primary"
          triggerLabel="Tạo không gian làm việc"
          triggerContent={<><Plus size={18} /> Tạo không gian</>}
          onSaved={props.onSaved}
        />
      </div>

      <Show when={error()}>
        <div class="inline-error page-error" role="alert">{error()}</div>
      </Show>

      <Show when={props.workspaces.length > 0}>
        <div class="library-toolbar">
          <div class="library-search">
            <Search size={16} />
            <input
              type="search"
              value={search()}
              placeholder="Tìm theo tên hoặc mô tả"
              aria-label="Tìm không gian làm việc"
              onInput={(event) => setSearch(event.currentTarget.value)}
            />
          </div>
        </div>
      </Show>

      <Switch>
        <Match when={props.loading && props.workspaces.length === 0}>
          <div class="loading-row"><i />Đang đọc danh sách không gian…</div>
        </Match>
        <Match when={props.workspaces.length === 0}>
          <div class="library-empty">
            <LayoutGrid size={26} />
            <strong>Chưa có không gian làm việc</strong>
            <span>Mỗi không gian giữ riêng cuộc trò chuyện và tài liệu của một dự án.</span>
          </div>
        </Match>
        <Match when={filtered().length === 0}>
          <div class="library-empty">
            <Search size={26} />
            <strong>Không có không gian nào khớp</strong>
            <span>Thử từ khóa khác.</span>
            <button class="button button-secondary" onClick={() => setSearch("")}>Xóa tìm kiếm</button>
          </div>
        </Match>
        <Match when={filtered().length > 0}>
          <div class="workspace-grid">
            <For each={filtered()}>{(workspace) => (
              // The whole card opens the workspace; only the edit and delete controls opt out,
              // so there is no dead space where a click looks ignored.
              <article
                classList={{ "workspace-card": true, active: props.activeId === workspace.id }}
                onClick={() => props.onOpen(workspace.id)}
              >
                <div class="workspace-card-top">
                  <div class="workspace-card-name">
                    <button
                      class="document-open"
                      title={`Mở ${workspace.name}`}
                    >{workspace.name}</button>
                    <span>{workspace.id.slice(0, 8)}</span>
                  </div>
                  <Show when={props.activeId === workspace.id}>
                    <span class="workspace-card-badge">Đang dùng</span>
                  </Show>
                </div>
                <p classList={{ muted: !workspace.description }}>
                  {workspace.description || "Chưa có mô tả"}
                </p>
                <div class="workspace-card-meta">
                  <MessageSquareText size={14} />
                  {workspace.conversation_count} cuộc trò chuyện
                  <em>·</em> cập nhật {formatRelativeTime(workspace.updated_at)}
                </div>
                <div class="workspace-card-actions" onClick={(event) => event.stopPropagation()}>
                  <WorkspaceDialog
                    workspace={workspace}
                    trigger="edit"
                    triggerClass="icon-action"
                    triggerLabel={`Sửa ${workspace.name}`}
                    triggerContent={<Pencil size={17} />}
                    onSaved={props.onSaved}
                    onDeleted={props.onDeleted}
                  />
                  <Show
                    when={confirmDelete() === workspace.id}
                    fallback={
                      <button
                        class="icon-action"
                        disabled={workingId() === workspace.id}
                        onClick={() => void remove(workspace)}
                        aria-label={`Xóa ${workspace.name}`}
                      ><Trash2 size={17} /></button>
                    }
                  >
                    <div class="confirm-delete">
                      <button
                        class="confirm-yes"
                        disabled={workingId() === workspace.id}
                        onClick={() => void remove(workspace)}
                      >Xóa hẳn</button>
                      <button class="confirm-no" onClick={() => setConfirmDelete("")}>Hủy</button>
                    </div>
                  </Show>
                </div>
              </article>
            )}</For>
          </div>
        </Match>
      </Switch>
    </section>
  );
}
