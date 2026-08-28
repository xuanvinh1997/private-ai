import { Dialog } from "@kobalte/core/dialog";
import {
  BrainCircuit,
  ChevronLeft,
  ChevronRight,
  Download,
  FileUp,
  Pencil,
  Plus,
  RotateCw,
  Search,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-solid";
import type { JSX } from "solid-js";
import { For, Match, Show, Switch, createResource, createSignal, onCleanup } from "solid-js";
import { api } from "../api";
import { Markdown } from "./Markdown";
import { formatFileSize, formatRelativeTime } from "../format";
import type {
  DocumentRecord,
  DocumentStatus,
  MemoryRecord,
  MemoryType,
  WorkspaceRecord,
} from "../types";

interface WorkspaceDialogProps {
  workspace?: WorkspaceRecord;
  onSaved: (workspace: WorkspaceRecord) => void;
  onDeleted?: (id: string) => void;
  trigger: "add" | "edit";
  /** Cho phép màn hình khác dùng lại hộp thoại này với nút bấm riêng. */
  triggerClass?: string;
  triggerLabel?: string;
  triggerContent?: JSX.Element;
}

export function WorkspaceDialog(props: WorkspaceDialogProps) {
  const [open, setOpen] = createSignal(false);
  const [name, setName] = createSignal("");
  const [description, setDescription] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [confirmDelete, setConfirmDelete] = createSignal(false);
  const [error, setError] = createSignal("");

  const prepare = (nextOpen: boolean) => {
    setOpen(nextOpen);
    if (nextOpen) {
      setName(props.workspace?.name ?? "");
      setDescription(props.workspace?.description ?? "");
      setConfirmDelete(false);
      setError("");
    }
  };

  const save = async () => {
    if (!name().trim()) {
      setError("Tên không gian không được để trống.");
      return;
    }
    setSaving(true);
    setError("");
    try {
      const workspace = props.workspace
        ? await api.updateWorkspace(props.workspace.id, name().trim(), description().trim())
        : await api.createWorkspace(name().trim(), description().trim());
      props.onSaved(workspace);
      setOpen(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không thể lưu không gian làm việc");
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!props.workspace || !props.onDeleted) return;
    if (!confirmDelete()) {
      setConfirmDelete(true);
      return;
    }
    setSaving(true);
    try {
      await api.deleteWorkspace(props.workspace.id);
      props.onDeleted(props.workspace.id);
      setOpen(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không thể xóa không gian làm việc");
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open()} onOpenChange={prepare}>
      <Dialog.Trigger
        class={props.triggerClass ?? (props.trigger === "add" ? "section-action" : "context-action")}
        aria-label={props.triggerLabel ?? (props.trigger === "add" ? "Thêm không gian làm việc" : "Sửa không gian làm việc")}
      >
        {props.triggerContent ?? (props.trigger === "add" ? <Plus size={18} /> : <Pencil size={18} />)}
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay class="dialog-overlay" />
        <div class="dialog-positioner">
          <Dialog.Content class="dialog-content">
            <Dialog.Title>{props.workspace ? "Sửa không gian làm việc" : "Không gian làm việc mới"}</Dialog.Title>
            <Dialog.Description>
              Nhóm các cuộc trò chuyện cùng dự án để tìm lại dễ dàng hơn.
            </Dialog.Description>
            <label class="field-label" for="workspace-name">Tên</label>
            <input id="workspace-name" class="text-input" value={name()} onInput={(event) => setName(event.currentTarget.value)} />
            <label class="field-label field-spaced" for="workspace-description">Mô tả</label>
            <textarea id="workspace-description" class="text-area" value={description()} onInput={(event) => setDescription(event.currentTarget.value)} rows={3} />
            <Show when={error()}><p class="field-error">{error()}</p></Show>
            <div class="dialog-actions dialog-actions-split">
              <Show when={props.workspace}>
                <button class="button button-danger" type="button" disabled={saving()} onClick={remove}>
                  <Trash2 size={17} /> {confirmDelete() ? "Bấm lần nữa để xóa" : "Xóa"}
                </button>
              </Show>
              <div>
                <Dialog.CloseButton class="button button-secondary">Hủy</Dialog.CloseButton>
                <button class="button button-primary" type="button" disabled={saving()} onClick={save}>
                  {saving() ? "Đang lưu…" : "Lưu"}
                </button>
              </div>
            </div>
            <Dialog.CloseButton class="icon-button dialog-close" aria-label="Đóng"><X size={20} /></Dialog.CloseButton>
          </Dialog.Content>
        </div>
      </Dialog.Portal>
    </Dialog>
  );
}

const STATUS_LABELS: Record<DocumentStatus, string> = {
  queued: "Đang chờ",
  processing: "Đang xử lý",
  ready: "Sẵn sàng",
  needs_ocr: "Cần OCR",
  failed: "Lỗi",
};

const STATUS_FILTERS: { value: string; label: string }[] = [
  { value: "", label: "Tất cả" },
  { value: "ready", label: "Sẵn sàng" },
  { value: "processing", label: "Đang xử lý" },
  { value: "needs_ocr", label: "Cần OCR" },
  { value: "failed", label: "Lỗi" },
];

function fileKind(filename: string): string {
  const extension = filename.split(".").pop() ?? "";
  if (!extension || extension === filename) return "TXT";
  return extension.slice(0, 4).toUpperCase();
}

const documentIsBusy = (document: DocumentRecord) =>
  document.status === "queued" || document.status === "processing";

export function DocumentViewer(props: {
  documentId: string;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [document, { refetch }] = createResource(() => props.documentId, api.document);
  const [working, setWorking] = createSignal(false);
  const [error, setError] = createSignal("");

  const readWithOcr = async () => {
    setWorking(true);
    setError("");
    try {
      await api.processDocument(props.documentId, true);
      // Extraction runs in the background, so give it a moment before reading back.
      window.setTimeout(() => void refetch(), 900);
      props.onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không thể đọc lại tài liệu");
    } finally {
      setWorking(false);
    }
  };

  return (
    <Dialog open onOpenChange={(open) => !open && props.onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay class="dialog-overlay" />
        <div class="dialog-positioner">
          <Dialog.Content class="dialog-content document-viewer">
            <Dialog.Title>{document()?.filename ?? "Đang mở tài liệu"}</Dialog.Title>
            <Dialog.Description>
              <Show when={document()} fallback="Đang đọc nội dung đã trích xuất…">
                {STATUS_LABELS[document()!.status]}
                <em> · </em>{formatFileSize(document()!.byte_size)}
                <em> · </em>{document()!.use_ocr === false ? "không dùng OCR" : "OCR theo mặc định"}
              </Show>
            </Dialog.Description>

            <Show when={error()}><p class="field-error">{error()}</p></Show>

            <div class="document-body">
              <Switch>
                <Match when={document.loading}>
                  <div class="loading-row"><i />Đang đọc nội dung…</div>
                </Match>
                <Match when={document.error}>
                  <div class="library-empty">
                    <strong>Không đọc được tài liệu</strong>
                    <span>{(document.error as Error)?.message}</span>
                  </div>
                </Match>
                <Match when={document()?.extracted_text}>
                  <Markdown content={document()!.extracted_text ?? ""} />
                </Match>
                <Match when={document()}>
                  <div class="library-empty">
                    <strong>Chưa có nội dung nào được trích xuất</strong>
                    <span>{document()!.error ?? "Tài liệu có thể là bản scan chưa qua OCR."}</span>
                    <button
                      class="button button-secondary"
                      disabled={working()}
                      onClick={() => void readWithOcr()}
                    >{working() ? "Đang đọc lại…" : "Đọc lại có OCR"}</button>
                  </div>
                </Match>
              </Switch>
            </div>

            <Dialog.CloseButton class="icon-button dialog-close" aria-label="Đóng tài liệu">
              <X size={20} />
            </Dialog.CloseButton>
          </Dialog.Content>
        </div>
      </Dialog.Portal>
    </Dialog>
  );
}

export function LibraryView(props: {
  documents: DocumentRecord[];
  total: number;
  summary: { total: number; byte_size: number; pending: number; indexing: number; failed: number };
  page: number;
  pageSize: number;
  pageCount: number;
  onPageChange: (page: number) => void;
  search: string;
  status: string;
  onFilterChange: (search: string, status: string) => void;
  workspaceName: string;
  loading: boolean;
  onUpload: () => void;
  onRefresh: () => void;
}) {
  const [workingId, setWorkingId] = createSignal("");
  const [viewing, setViewing] = createSignal("");
  const [confirmDelete, setConfirmDelete] = createSignal("");
  const [error, setError] = createSignal("");
  const [draftSearch, setDraftSearch] = createSignal(props.search);
  let searchTimer: number | undefined;

  const filtering = () => Boolean(props.search || props.status);
  const rangeStart = () => props.page * props.pageSize + 1;
  const rangeEnd = () => props.page * props.pageSize + props.documents.length;

  // Debounce so each keystroke does not become its own request.
  const queueSearch = (value: string) => {
    setDraftSearch(value);
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => props.onFilterChange(value.trim(), props.status), 250);
  };
  onCleanup(() => window.clearTimeout(searchTimer));

  const clearFilters = () => {
    setDraftSearch("");
    props.onFilterChange("", "");
  };

  const retry = async (id: string, useOcr?: boolean) => {
    setWorkingId(id);
    setError("");
    try {
      await api.processDocument(id, useOcr);
      props.onRefresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không thể xử lý lại tài liệu");
    } finally {
      setWorkingId("");
    }
  };

  const remove = async (id: string) => {
    if (confirmDelete() !== id) {
      setConfirmDelete(id);
      return;
    }
    setWorkingId(id);
    try {
      await api.deleteDocument(id);
      setConfirmDelete("");
      props.onRefresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không thể xóa tài liệu");
    } finally {
      setWorkingId("");
    }
  };

  return (
    <section class="page-view">
      <div class="page-heading page-heading-row">
        <div>
          <span>Thư viện riêng</span>
          <h1>{props.workspaceName}</h1>
          <p class="library-stats">
            <Show when={props.summary.total > 0} fallback="Chưa có tài liệu nào trong không gian này.">
              <strong>{props.summary.total}</strong> tài liệu
              <em>·</em> {formatFileSize(props.summary.byte_size)}
              <Show when={props.summary.pending > 0}>
                <em>·</em> <span class="stat-pending">{props.summary.pending} đang xử lý</span>
              </Show>
              <Show when={props.summary.indexing > 0}>
                <em>·</em> <span class="stat-pending">{props.summary.indexing} đang lập chỉ mục</span>
              </Show>
              <Show when={props.summary.failed > 0}>
                <em>·</em> <span class="stat-failed">{props.summary.failed} cần xem lại</span>
              </Show>
            </Show>
          </p>
        </div>
        <button class="button button-primary" onClick={props.onUpload}>
          <FileUp size={18} /> Thêm tài liệu
        </button>
      </div>

      <Show when={error()}>
        <div class="inline-error page-error" role="alert">{error()}</div>
      </Show>

      <Show when={props.summary.total > 0}>
        <div class="library-toolbar">
          <div class="library-search">
            <Search size={16} />
            <input
              type="search"
              value={draftSearch()}
              placeholder="Tìm theo tên tệp"
              aria-label="Tìm tài liệu theo tên tệp"
              onInput={(event) => queueSearch(event.currentTarget.value)}
            />
          </div>
          <div class="library-filters" role="group" aria-label="Lọc theo trạng thái">
            <For each={STATUS_FILTERS}>{(filter) => (
              <button
                type="button"
                classList={{ active: props.status === filter.value }}
                aria-pressed={props.status === filter.value}
                onClick={() => props.onFilterChange(props.search, filter.value)}
              >{filter.label}</button>
            )}</For>
          </div>
        </div>
      </Show>

      <Switch>
        <Match when={props.loading && props.documents.length === 0}>
          <div class="loading-row"><i />Đang đọc thư viện…</div>
        </Match>
        <Match when={props.documents.length === 0 && !filtering()}>
          <div class="library-empty">
            <FileUp size={26} />
            <strong>Chưa có tài liệu nào</strong>
            <span>Thêm tệp từ đây, hoặc kéo thả vào cột ngữ cảnh ở màn trò chuyện.</span>
            <button class="button button-secondary" onClick={props.onUpload}>Thêm tài liệu</button>
          </div>
        </Match>
        <Match when={props.documents.length === 0 && filtering()}>
          <div class="library-empty">
            <Search size={26} />
            <strong>Không có tài liệu nào khớp</strong>
            <span>Thử từ khóa khác hoặc bỏ bộ lọc trạng thái.</span>
            <button class="button button-secondary" onClick={clearFilters}>Xóa bộ lọc</button>
          </div>
        </Match>
        <Match when={props.documents.length > 0}>
          <div classList={{ "document-list": true, refreshing: props.loading }}>
            <For each={props.documents}>{(document) => (
              <article class="document-row" aria-busy={documentIsBusy(document)}>
                <div class="document-icon" aria-hidden="true">{fileKind(document.filename)}</div>
                <div class="document-copy">
                  <button
                    class="document-open"
                    disabled={documentIsBusy(document)}
                    title={documentIsBusy(document)
                      ? `${document.filename} vẫn đang xử lý`
                      : `Xem nội dung ${document.filename}`}
                    onClick={() => !documentIsBusy(document) && setViewing(document.id)}
                  >{document.filename}</button>
                  <span>
                    {formatFileSize(document.byte_size)}
                    <em>·</em> {formatRelativeTime(document.created_at)}
                    <Show when={document.error}><em>·</em> <b>{document.error}</b></Show>
                  </span>
                </div>
                <span classList={{ "document-status": true, [`document-${document.status}`]: true }}>
                  <i aria-hidden="true" />{STATUS_LABELS[document.status]}
                  <Show when={documentIsBusy(document)}>
                    <span class="document-status-progress" aria-hidden="true" />
                  </Show>
                </span>
                <div class="document-actions">
                  <Show when={document.status === "needs_ocr"}>
                    <button
                      class="text-action"
                      disabled={workingId() === document.id}
                      onClick={() => void retry(document.id, true)}
                    >Đọc lại có OCR</button>
                  </Show>
                  <Show when={document.status === "failed" || document.status === "needs_ocr"}>
                    <button
                      class="icon-action"
                      disabled={workingId() === document.id}
                      onClick={() => void retry(document.id)}
                      aria-label={`Xử lý lại ${document.filename}`}
                    ><RotateCw size={17} /></button>
                  </Show>
                  <Show
                    when={confirmDelete() === document.id}
                    fallback={
                      <button
                        class="icon-action"
                        disabled={workingId() === document.id}
                        onClick={() => void remove(document.id)}
                        aria-label={`Xóa ${document.filename}`}
                      ><Trash2 size={17} /></button>
                    }
                  >
                    <div class="confirm-delete">
                      <button
                        class="confirm-yes"
                        disabled={workingId() === document.id}
                        onClick={() => void remove(document.id)}
                      >Xóa hẳn</button>
                      <button class="confirm-no" onClick={() => setConfirmDelete("")}>Hủy</button>
                    </div>
                  </Show>
                </div>
              </article>
            )}</For>
          </div>

          <nav class="pager" aria-label="Phân trang tài liệu">
            <span class="pager-range">
              {rangeStart()}–{rangeEnd()} trong {props.total}
              <Show when={filtering()}> (đã lọc)</Show>
            </span>
            <Show when={props.pageCount > 1}>
              <div class="pager-controls">
                <button
                  type="button"
                  disabled={props.page === 0}
                  onClick={() => props.onPageChange(props.page - 1)}
                  aria-label="Trang trước"
                ><ChevronLeft size={17} /></button>
                <span>Trang {props.page + 1}/{props.pageCount}</span>
                <button
                  type="button"
                  disabled={props.page >= props.pageCount - 1}
                  onClick={() => props.onPageChange(props.page + 1)}
                  aria-label="Trang sau"
                ><ChevronRight size={17} /></button>
              </div>
            </Show>
          </nav>
        </Match>
      </Switch>

      <Show when={viewing()}>
        <DocumentViewer
          documentId={viewing()}
          onClose={() => setViewing("")}
          onChanged={props.onRefresh}
        />
      </Show>
    </section>
  );
}

export function MemoryView(props: { embedded?: boolean; profileId?: string } = {}) {
  // Keyed on the profile so switching accounts reloads the list instead of showing the
  // previous person's memories.
  const [memories, { refetch }] = createResource(
    () => props.profileId ?? "",
    () => api.memories(),
  );
  const [open, setOpen] = createSignal(false);
  const [type, setType] = createSignal<MemoryType>("preference");
  const [content, setContent] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [editing, setEditing] = createSignal<MemoryRecord>();
  const [confirmDelete, setConfirmDelete] = createSignal("");
  const [error, setError] = createSignal("");

  const saveMemory = async () => {
    if (!content().trim()) return;
    setSaving(true);
    try {
      const current = editing();
      if (current) {
        await api.updateMemory(current.id, type(), content().trim());
      } else {
        await api.createMemory(type(), content().trim());
      }
      setContent("");
      setEditing(undefined);
      setOpen(false);
      refetch();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Không thể lưu bộ nhớ");
    } finally {
      setSaving(false);
    }
  };

  const disable = async (id: string) => {
    await api.disableMemory(id);
    refetch();
  };

  const enable = async (id: string) => {
    await api.enableMemory(id);
    refetch();
  };

  const openEditor = (memory?: MemoryRecord) => {
    setEditing(memory);
    setType(memory?.type ?? "preference");
    setContent(memory?.content ?? "");
    setOpen(true);
  };

  const remove = async (id: string) => {
    if (confirmDelete() !== id) {
      setConfirmDelete(id);
      return;
    }
    await api.deleteMemory(id);
    setConfirmDelete("");
    refetch();
  };

  const typeLabel = (value: MemoryType) => ({ preference: "Sở thích", fact: "Thông tin", episodic: "Phiên làm việc" })[value];

  const exportMemories = () => {
    const payload = {
      exported_at: new Date().toISOString(),
      memories: memories() ?? [],
    };
    const url = URL.createObjectURL(new Blob(
      [JSON.stringify(payload, null, 2)],
      { type: "application/json" },
    ));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `private-ai-memories-${new Date().toISOString().slice(0, 10)}.json`;
    anchor.click();
    URL.revokeObjectURL(url);
  };

  return (
    <section classList={{ "page-view": !props.embedded, "settings-panel": Boolean(props.embedded) }}>
      <div class="page-heading page-heading-row">
        <div><span>Bộ nhớ cá nhân</span><h1>Điều Private AI ghi nhớ</h1><p>Bạn kiểm soát từng mục đã lưu và có thể tắt hoặc xóa bất cứ lúc nào.</p></div>
        <div class="page-heading-actions">
          <button class="button button-secondary" disabled={!memories()?.length} onClick={exportMemories}><Download size={18} /> Xuất JSON</button>
          <button class="button button-primary" onClick={() => openEditor()}><Plus size={18} /> Thêm bộ nhớ</button>
        </div>
      </div>
      <Show when={error()}><div class="inline-error page-error">{error()}</div></Show>
      <Switch>
        <Match when={memories.loading}><div class="loading-row"><i />Đang đọc bộ nhớ…</div></Match>
        <Match when={(memories()?.length ?? 0) === 0}>
          <div class="memory-empty"><BrainCircuit size={30} /><strong>Chưa có thông tin nào được lưu</strong><span>Thêm sở thích hoặc thông tin bạn muốn AI ghi nhớ.</span></div>
        </Match>
        <Match when={(memories()?.length ?? 0) > 0}>
          <div class="memory-list"><For each={memories()}>{(memory) => (
            <article classList={{ "memory-row": true, disabled: !memory.enabled }}>
              <div class="memory-type"><ShieldCheck size={19} />{typeLabel(memory.type)}</div>
              <p>{memory.content}</p>
              <div class="memory-actions">
                <button onClick={() => openEditor(memory)}>Sửa</button>
                <Show when={memory.enabled}><button onClick={() => void disable(memory.id)}>Tắt</button></Show>
                <Show when={!memory.enabled}><button onClick={() => void enable(memory.id)}>Bật</button></Show>
                <button classList={{ danger: confirmDelete() === memory.id }} onClick={() => void remove(memory.id)}>{confirmDelete() === memory.id ? "Xác nhận xóa" : "Xóa"}</button>
              </div>
            </article>
          )}</For></div>
        </Match>
      </Switch>

      <Dialog open={open()} onOpenChange={setOpen}>
        <Dialog.Portal>
          <Dialog.Overlay class="dialog-overlay" />
          <div class="dialog-positioner"><Dialog.Content class="dialog-content">
            <Dialog.Title>{editing() ? "Sửa bộ nhớ" : "Thêm bộ nhớ"}</Dialog.Title>
            <Dialog.Description>Chỉ lưu những điều bạn chủ động nhập tại đây.</Dialog.Description>
            <label class="field-label" for="memory-type">Loại</label>
            <select id="memory-type" class="text-input" value={type()} onChange={(event) => setType(event.currentTarget.value as MemoryType)}>
              <option value="preference">Sở thích</option><option value="fact">Thông tin</option><option value="episodic">Phiên làm việc</option>
            </select>
            <label class="field-label field-spaced" for="memory-content">Nội dung</label>
            <textarea id="memory-content" class="text-area" value={content()} onInput={(event) => setContent(event.currentTarget.value)} rows={4} />
            <div class="dialog-actions"><Dialog.CloseButton class="button button-secondary">Hủy</Dialog.CloseButton><button class="button button-primary" disabled={saving()} onClick={saveMemory}>Lưu</button></div>
            <Dialog.CloseButton class="icon-button dialog-close" aria-label="Đóng"><X size={20} /></Dialog.CloseButton>
          </Dialog.Content></div>
        </Dialog.Portal>
      </Dialog>
    </section>
  );
}
