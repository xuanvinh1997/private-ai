import { Dialog } from "@kobalte/core/dialog";
import { AlertCircle, Check, FileUp, LoaderCircle, RotateCw, X } from "lucide-solid";
import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";
import { api } from "../api";
import { formatFileSize } from "../format";
import type { DocumentRecord, DocumentStatus } from "../types";

const ACCEPTED_EXTENSIONS = [
  ".pdf",
  ".docx",
  ".pptx",
  ".xlsx",
  ".txt",
  ".md",
  ".markdown",
  ".csv",
  ".json",
  ".yaml",
  ".yml",
  ".png",
  ".jpg",
  ".jpeg",
  ".webp",
  ".gif",
  ".bmp",
  ".tif",
  ".tiff",
] as const;
const ACCEPT = ACCEPTED_EXTENSIONS.join(",");
const MAX_FILE_SIZE = 100 * 1024 * 1024;
const PROCESSING_POLL_MS = 1_000;
const PROCESSING_TIMEOUT_MS = 5 * 60 * 1_000;

type StagedStatus = DocumentStatus | "pending" | "uploading" | "indexing" | "invalid";

type Staged = {
  file: File;
  useOcr: boolean;
  status: StagedStatus;
  progress: number;
  documentId?: string;
  error?: string;
};

const DOCUMENT_PROGRESS: Record<DocumentStatus, number> = {
  queued: 45,
  processing: 78,
  ready: 100,
  needs_ocr: 100,
  failed: 100,
};

const isInFlight = (status: StagedStatus) =>
  status === "uploading" || status === "queued" || status === "processing" || status === "indexing";

const statusLabel = (item: Staged) => {
  switch (item.status) {
    case "pending": return "Sẵn sàng tải lên";
    case "uploading": return `Đang tải lên · ${item.progress}%`;
    case "queued": return "Đã nhận tệp · đang chờ OCR";
    case "processing": return "Đang OCR và chuẩn bị lập chỉ mục";
    case "indexing": return "OCR xong · đang tạo embedding và graph memory";
    case "ready": return "Đã xử lý xong";
    case "needs_ocr": return "OCR chưa đọc được nội dung";
    case "failed": return "Xử lý thất bại";
    case "invalid": return item.error ?? "Tệp không hợp lệ";
  }
};

const documentPatch = (document: DocumentRecord): Partial<Staged> => {
  const indexing = document.status === "ready" && !document.indexed_at;
  return {
    documentId: document.id,
    status: indexing ? "indexing" : document.status,
    progress: indexing ? 92 : DOCUMENT_PROGRESS[document.status],
    error: document.error || undefined,
  };
};

const pause = (milliseconds: number) =>
  new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds));

const fileKey = (file: File) => `${file.name}:${file.size}:${file.lastModified}`;

function validateFile(file: File): string | undefined {
  const name = file.name.toLowerCase();
  if (!file.size) return "Tệp trống";
  if (file.size > MAX_FILE_SIZE) return "Vượt quá giới hạn 100 MB";
  if (!ACCEPTED_EXTENSIONS.some((extension) => name.endsWith(extension))) {
    return "Định dạng chưa được hỗ trợ";
  }
  return undefined;
}

function stageFiles(files: File[], defaultOcr: boolean, known = new Set<string>()) {
  const added: Staged[] = [];
  let skipped = 0;
  for (const file of files) {
    const key = fileKey(file);
    if (known.has(key)) {
      skipped += 1;
      continue;
    }
    known.add(key);
    const validationError = validateFile(file);
    added.push({
      file,
      useOcr: defaultOcr,
      status: validationError ? "invalid" : "pending",
      progress: 0,
      error: validationError,
    });
  }
  return { added, skipped };
}

/** Files are staged first so each one can carry its own OCR choice before anything uploads. */
export function UploadDialog(props: {
  open: boolean;
  workspaceId: string;
  workspaceName: string;
  defaultOcr: boolean;
  initialFiles?: File[];
  onClose: () => void;
  onCompleted: (result: {
    uploaded: number;
    ready: number;
    failed: number;
    pending: number;
  }) => void;
}) {
  const [staged, setStaged] = createSignal<Staged[]>([]);
  const [dragDepth, setDragDepth] = createSignal(0);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [notice, setNotice] = createSignal("");
  const [progress, setProgress] = createSignal({ current: 0, total: 0 });
  let picker!: HTMLInputElement;

  const add = (files: File[]) => {
    if (!files.length) return;
    const known = new Set(staged().map((item) => fileKey(item.file)));
    const { added, skipped } = stageFiles(files, props.defaultOcr, known);
    if (added.length) setStaged((current) => [...current, ...added]);
    setNotice(skipped ? `${skipped} tệp trùng đã được bỏ qua.` : "");
  };

  // Only reset on the closed → open transition. Tracking staged() here would reset the queue
  // after every edit and could repeatedly retrigger the effect.
  createEffect(on(
    () => props.open,
    (open) => {
      if (!open) return;
      const { added, skipped } = stageFiles(props.initialFiles ?? [], props.defaultOcr);
      setError("");
      setNotice(skipped ? `${skipped} tệp trùng đã được bỏ qua.` : "");
      setProgress({ current: 0, total: 0 });
      setDragDepth(0);
      setStaged(added);
    },
  ));

  const update = (index: number, patch: Partial<Staged>) =>
    setStaged((current) =>
      current.map((item, position) => (position === index ? { ...item, ...patch } : item)),
    );

  const actionable = createMemo(() =>
    staged().filter((item) =>
      item.status === "pending" || item.status === "failed" || item.status === "needs_ocr"
    ),
  );
  const failedCount = createMemo(() =>
    staged().filter((item) => item.status === "failed" || item.status === "needs_ocr").length,
  );
  const invalidCount = createMemo(() =>
    staged().filter((item) => item.status === "invalid").length,
  );

  const confirm = async () => {
    if (busy()) return;
    if (!props.workspaceId) {
      setError("Hãy tạo một không gian làm việc trước khi thêm tài liệu.");
      return;
    }
    const queue = staged()
      .map((item, index) => ({ item, index }))
      .filter(({ item }) =>
        item.status === "pending" || item.status === "failed" || item.status === "needs_ocr"
      );
    if (!queue.length) return;

    setBusy(true);
    setError("");
    setNotice("");
    setProgress({ current: 0, total: queue.length });
    let uploaded = 0;
    for (const { index, item } of queue) {
      try {
        let document: DocumentRecord;
        if (item.documentId) {
          update(index, { status: "queued", progress: 45, error: undefined });
          await api.processDocument(item.documentId, item.useOcr);
          document = await api.document(item.documentId);
        } else {
          update(index, { status: "uploading", progress: 1, error: undefined });
          document = await api.uploadDocument(
            item.file,
            props.workspaceId,
            item.useOcr,
            (percent) => update(index, {
              progress: Math.max(1, Math.round(percent * 0.35)),
            }),
          );
          uploaded += 1;
        }
        update(index, documentPatch(document));
      } catch (cause) {
        update(index, {
          status: "failed",
          progress: 100,
          error: cause instanceof Error ? cause.message : "Không thể tải lên",
        });
      }
    }

    const deadline = Date.now() + PROCESSING_TIMEOUT_MS;
    let consecutivePollErrors = 0;
    while (Date.now() < deadline) {
      const active = queue.filter(({ index }) => isInFlight(staged()[index]?.status ?? "failed"));
      const completed = queue.length - active.length;
      setProgress({ current: completed, total: queue.length });
      if (!active.length) break;

      await pause(PROCESSING_POLL_MS);
      try {
        const page = await api.documents(props.workspaceId, 100, 0);
        const byId = new Map(page.items.map((document) => [document.id, document]));
        const missing = active.filter(({ index }) => {
          const id = staged()[index]?.documentId;
          return id && !byId.has(id);
        });
        const missingDocuments = await Promise.all(
          missing.map(({ index }) => api.document(staged()[index]!.documentId!)),
        );
        missingDocuments.forEach((document) => byId.set(document.id, document));
        for (const { index } of active) {
          const id = staged()[index]?.documentId;
          const document = id ? byId.get(id) : undefined;
          if (document) update(index, documentPatch(document));
        }
        consecutivePollErrors = 0;
      } catch (cause) {
        consecutivePollErrors += 1;
        if (consecutivePollErrors < 3) continue;
        setError(
          cause instanceof Error
            ? `Không đọc được trạng thái xử lý: ${cause.message}`
            : "Không đọc được trạng thái xử lý tài liệu.",
        );
        break;
      }
    }

    setBusy(false);
    const result = queue.map(({ index }) => staged()[index]).filter(Boolean);
    const ready = result.filter((item) => item.status === "ready").length;
    const failed = result.filter((item) =>
      item.status === "failed" || item.status === "needs_ocr"
    ).length;
    const pending = result.filter((item) => isInFlight(item.status)).length;
    setProgress({ current: ready + failed, total: queue.length });
    props.onCompleted({ uploaded, ready, failed, pending });
    if (failed) {
      setError(`${failed} tệp xử lý chưa thành công. Xem lỗi ngay dưới tên tệp rồi thử lại.`);
      return;
    }
    if (pending) {
      setNotice("Tệp vẫn đang được xử lý nền. Trạng thái sẽ tiếp tục cập nhật trong Thư viện.");
      return;
    }
    if (staged().every((item) => item.status === "ready")) {
      props.onClose();
    } else if (invalidCount()) {
      setNotice("Các tệp hợp lệ đã xử lý xong. Bỏ hoặc thay thế tệp không hợp lệ còn lại.");
    }
  };

  const actionLabel = () => {
    if (busy()) return `Đang xử lý ${progress().current}/${progress().total}`;
    if (failedCount()) return `Thử lại ${actionable().length} tệp`;
    return `Tải lên${actionable().length ? ` ${actionable().length} tệp` : ""}`;
  };

  return (
    <Dialog open={props.open} onOpenChange={(open) => !open && !busy() && props.onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay class="dialog-overlay" />
        <div class="dialog-positioner">
          <Dialog.Content class="dialog-content upload-dialog">
            <div class="dialog-mark"><FileUp size={22} aria-hidden="true" /></div>
            <Dialog.Title>Thêm tài liệu</Dialog.Title>
            <Dialog.Description>
              <Show
                when={props.workspaceId}
                fallback="Tạo một không gian làm việc trước, sau đó quay lại để thêm tài liệu."
              >
                Tải vào <strong>{props.workspaceName}</strong>. Có thể bật OCR riêng cho từng tệp.
              </Show>
            </Dialog.Description>

            <Show when={!props.workspaceId}>
              <div class="upload-blocked" role="alert">
                <AlertCircle size={19} aria-hidden="true" />
                <span><strong>Chưa có nơi lưu tài liệu</strong>Đóng hộp thoại và tạo một không gian làm việc ở thanh bên.</span>
              </div>
            </Show>

            <button
              type="button"
              classList={{ "upload-drop": true, active: dragDepth() > 0 }}
              disabled={busy() || !props.workspaceId}
              aria-describedby="upload-help upload-status"
              onClick={() => picker.click()}
              onDragEnter={(event) => {
                if (!event.dataTransfer?.types.includes("Files")) return;
                event.preventDefault();
                setDragDepth((depth) => depth + 1);
              }}
              onDragOver={(event) => {
                if (event.dataTransfer?.types.includes("Files")) event.preventDefault();
              }}
              onDragLeave={() => setDragDepth((depth) => Math.max(0, depth - 1))}
              onDrop={(event) => {
                const files = Array.from(event.dataTransfer?.files ?? []);
                if (!files.length) return;
                event.preventDefault();
                setDragDepth(0);
                add(files);
              }}
            >
              <FileUp size={27} aria-hidden="true" />
              <strong>{dragDepth() > 0 ? "Thả tệp vào đây" : "Chọn tệp từ máy"}</strong>
              <small id="upload-help">Hoặc kéo thả · PDF, Office, ảnh và văn bản · tối đa 100 MB/tệp</small>
            </button>

            <div id="upload-status" class="sr-only" aria-live="polite" aria-atomic="true">
              {busy()
                ? `Đã xử lý ${progress().current} trên ${progress().total} tệp`
                : staged().length
                  ? `Đã chọn ${staged().length} tệp, ${invalidCount()} tệp không hợp lệ`
                  : "Chưa chọn tệp"}
            </div>

            <Show when={staged().length > 0}>
              <div class="staged-heading">
                <strong>{staged().length} tệp đã chọn</strong>
                <Show when={invalidCount()}><span>{invalidCount()} cần bỏ hoặc thay thế</span></Show>
              </div>
              <ul class="staged-list">
                <For each={staged()}>{(item, index) => (
                  <li
                    classList={{ [`staged-${item.status}`]: true }}
                    aria-busy={isInFlight(item.status)}
                  >
                    <span class="staged-state" aria-hidden="true">
                      <Show when={item.status === "ready"}><Check size={15} /></Show>
                      <Show when={isInFlight(item.status)}><LoaderCircle size={15} /></Show>
                      <Show when={item.status === "failed" || item.status === "needs_ocr" || item.status === "invalid"}><AlertCircle size={15} /></Show>
                    </span>
                    <div class="staged-identity">
                      <strong title={item.file.name}>{item.file.name}</strong>
                      <small>{formatFileSize(item.file.size)} · {statusLabel(item)}</small>
                      <Show when={item.error && item.status !== "invalid"}>
                        <small class="staged-error" role="alert">{item.error}</small>
                      </Show>
                      <Show when={item.status !== "pending" && item.status !== "invalid"}>
                        <span
                          classList={{
                            "staged-progress": true,
                            active: isInFlight(item.status),
                            failed: item.status === "failed" || item.status === "needs_ocr",
                          }}
                          role="progressbar"
                          aria-label={`Tiến độ ${item.file.name}`}
                          aria-valuemin="0"
                          aria-valuemax="100"
                          aria-valuenow={item.progress}
                          aria-valuetext={statusLabel(item)}
                        >
                          <i style={{ transform: `scaleX(${item.progress / 100})` }} />
                        </span>
                      </Show>
                    </div>
                    <label class="staged-ocr" title="Đọc chữ trong ảnh hoặc tài liệu scan">
                      <input
                        type="checkbox"
                        checked={item.useOcr}
                        disabled={busy() || isInFlight(item.status) || item.status === "ready" || item.status === "invalid"}
                        aria-label={`Dùng OCR cho ${item.file.name}`}
                        onChange={(event) => update(index(), { useOcr: event.currentTarget.checked })}
                      />
                      OCR
                    </label>
                    <button
                      type="button"
                      class="staged-remove"
                      disabled={busy() || isInFlight(item.status)}
                      aria-label={`Bỏ ${item.file.name} khỏi danh sách`}
                      onClick={() => setStaged((current) =>
                        current.filter((_, position) => position !== index()),
                      )}
                    ><X size={16} aria-hidden="true" /></button>
                  </li>
                )}</For>
              </ul>
            </Show>

            <Show when={notice()}><p class="field-status" role="status">{notice()}</p></Show>
            <Show when={error()}><p class="field-error" role="alert">{error()}</p></Show>
            <Show when={busy()}>
              <div
                class="upload-progress"
                role="progressbar"
                aria-label="Tiến độ tải tài liệu"
                aria-valuemin="0"
                aria-valuemax={progress().total}
                aria-valuenow={progress().current}
                aria-valuetext={`Đã xử lý ${progress().current} trên ${progress().total} tệp`}
              >
                <div>
                  <span>Hoàn tất</span>
                  <strong>{progress().current}/{progress().total} tệp</strong>
                </div>
                <span class="upload-progress-track" aria-hidden="true">
                  <i style={{ transform: `scaleX(${progress().total ? progress().current / progress().total : 0})` }} />
                </span>
              </div>
            </Show>

            <div class="dialog-actions upload-actions">
              <button
                class="button button-secondary"
                type="button"
                disabled={busy()}
                onClick={props.onClose}
              >Hủy</button>
              <button
                class="button button-primary"
                type="button"
                disabled={busy() || actionable().length === 0 || !props.workspaceId}
                onClick={() => void confirm()}
              >
                <Show when={failedCount() && !busy()}><RotateCw size={17} aria-hidden="true" /></Show>
                <Show when={busy()}><LoaderCircle class="upload-spinner" size={17} aria-hidden="true" /></Show>
                {actionLabel()}
              </button>
            </div>

            <input
              ref={picker}
              class="sr-only"
              type="file"
              multiple
              accept={ACCEPT}
              onChange={(event) => {
                add(Array.from(event.currentTarget.files ?? []));
                event.currentTarget.value = "";
              }}
            />
            <Dialog.CloseButton class="icon-button dialog-close" disabled={busy()} aria-label="Đóng">
              <X size={20} aria-hidden="true" />
            </Dialog.CloseButton>
          </Dialog.Content>
        </div>
      </Dialog.Portal>
    </Dialog>
  );
}
