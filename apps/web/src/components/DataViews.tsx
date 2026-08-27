import { Dialog } from "@kobalte/core/dialog";
import {
  BrainCircuit,
  Download,
  FileText,
  FileUp,
  Pencil,
  Plus,
  RotateCw,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-solid";
import { For, Match, Show, Switch, createResource, createSignal } from "solid-js";
import { api } from "../api";
import type { DocumentRecord, MemoryRecord, MemoryType, WorkspaceRecord } from "../types";

interface WorkspaceDialogProps {
  workspace?: WorkspaceRecord;
  onSaved: (workspace: WorkspaceRecord) => void;
  onDeleted?: (id: string) => void;
  trigger: "add" | "edit";
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
        class={props.trigger === "add" ? "section-action" : "context-action"}
        aria-label={props.trigger === "add" ? "Thêm không gian làm việc" : "Sửa không gian làm việc"}
      >
        {props.trigger === "add" ? <Plus size={18} /> : <Pencil size={18} />}
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

export function LibraryView(props: {
  documents: DocumentRecord[] | undefined;
  loading: boolean;
  uploading: boolean;
  onUpload: () => void;
  onRefresh: () => void;
}) {
  const [workingId, setWorkingId] = createSignal("");
  const [confirmDelete, setConfirmDelete] = createSignal("");
  const [error, setError] = createSignal("");

  const retry = async (id: string) => {
    setWorkingId(id);
    setError("");
    try {
      await api.processDocument(id);
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

  const statusLabel = (status: DocumentRecord["status"]) => ({
    queued: "Đang chờ",
    processing: "Đang xử lý",
    ready: "Sẵn sàng",
    needs_ocr: "Cần OCR",
    failed: "Lỗi",
  })[status];

  return (
    <section class="page-view">
      <div class="page-heading page-heading-row">
        <div><span>Thư viện riêng</span><h1>Tài liệu của bạn</h1><p>Tài liệu được trích xuất trên máy và sẵn sàng cho truy xuất.</p></div>
        <button class="button button-primary" onClick={props.onUpload}><FileUp size={18} /> Thêm tài liệu</button>
      </div>
      <Show when={error()}><div class="inline-error page-error" role="alert">{error()}</div></Show>
      <Switch>
        <Match when={props.loading}><div class="loading-row"><i />Đang đọc thư viện…</div></Match>
        <Match when={(props.documents?.length ?? 0) === 0}>
          <button class="large-upload" onClick={props.onUpload} disabled={props.uploading}>
            <FileUp size={30} /><strong>{props.uploading ? "Đang nhập tài liệu…" : "Chọn tài liệu từ máy"}</strong>
            <span>PDF, Office, JPG, PNG, WebP, Markdown và văn bản · tối đa 100 MB</span>
          </button>
        </Match>
        <Match when={(props.documents?.length ?? 0) > 0}>
          <div class="document-list">
            <For each={props.documents}>{(document) => (
              <article class="document-row">
                <div class="document-icon"><FileText size={22} /></div>
                <div class="document-copy"><strong>{document.filename}</strong><span>{(document.byte_size / 1024 / 1024).toFixed(1)} MB · {document.error || "Đã lưu cục bộ"}</span></div>
                <span class={`document-status document-${document.status}`}>{statusLabel(document.status)}</span>
                <div class="document-actions">
                  <Show when={document.status === "failed" || document.status === "needs_ocr"}>
                    <button disabled={workingId() === document.id} onClick={() => void retry(document.id)} aria-label="Xử lý lại"><RotateCw size={18} /></button>
                  </Show>
                  <button classList={{ danger: confirmDelete() === document.id }} disabled={workingId() === document.id} onClick={() => void remove(document.id)} aria-label={confirmDelete() === document.id ? "Bấm lại để xác nhận xóa" : "Xóa tài liệu"}><Trash2 size={18} /></button>
                </div>
              </article>
            )}</For>
          </div>
        </Match>
      </Switch>
    </section>
  );
}

export function MemoryView() {
  const [memories, { refetch }] = createResource(api.memories);
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
    <section class="page-view">
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
