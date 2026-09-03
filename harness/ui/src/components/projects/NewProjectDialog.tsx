import { createSignal, For, Show } from "solid-js";
import { useDragDrop } from "../../hooks/useDragDrop";
import { isDemo } from "../../lib/demo";
import { demoCreatedProject } from "../../lib/fixtures/projects";
import { createProject, pickDirectory } from "../../lib/projects";
import type { Project, ProjectKind } from "../../lib/protocol";
import Icon, { type IconName } from "../Icon";
import { InfoDot } from "../settings/FormKit";
import DialogShell, { Button } from "./DialogShell";

/**
 * Chọn loại dự án, rồi chọn thư mục.
 *
 * Loại đứng **trước** đường dẫn vì nó là lựa chọn khó rút lại: đổi loại là tạo lại dự án,
 * còn đổi đường dẫn chỉ là gõ lại một dòng. Đặt nó sau ô đường dẫn thì người dùng đã trả
 * lời xong câu dễ rồi mới gặp câu khó, và thường bấm qua nó.
 *
 * Ba lối vào đường dẫn cùng tồn tại, không phải vì thừa: hộp thoại của hệ điều hành là
 * lối chính, kéo thả là lối nhanh nhất khi cửa sổ Finder đang mở sẵn, và ô gõ tay là lối
 * duy nhất còn lại khi chạy ngoài Tauri hoặc khi đường dẫn được dán từ terminal.
 */
export default function NewProjectDialog(props: {
  /** Loại chọn sẵn — ba nút trên màn hình dự án đã nói người dùng muốn gì. */
  kind?: ProjectKind;
  onCreated: (project: Project) => void;
  onClose: () => void;
}) {
  const [kind, setKind] = createSignal<ProjectKind>(props.kind ?? "code");
  const [path, setPath] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // Thả một thư mục vào cửa sổ trong lúc hộp thoại đang mở là điền vào ô, không phải mở
  // ngay: loại dự án vẫn đang chờ được chọn, và mở ngay là bỏ qua đúng câu hỏi khó.
  useDragDrop((paths) => {
    const first = paths[0];
    if (first !== undefined && !busy()) {
      setPath(first);
      setError(null);
    }
  });

  const choose = async () => {
    setError(null);
    try {
      const picked = await pickDirectory(
        kind() === "code" ? "Chọn thư mục mã nguồn" : "Chọn thư mục tài liệu",
      );
      if (picked !== null) setPath(picked);
    } catch (err) {
      setError(`Không mở được hộp thoại chọn thư mục: ${err}`);
    }
  };

  const submit = async () => {
    const trimmed = path().trim();
    if (trimmed === "" || busy()) return;
    setBusy(true);
    setError(null);
    try {
      const project = isDemo()
        ? await new Promise<Project>((resolve) =>
            setTimeout(() => resolve(demoCreatedProject(trimmed, kind())), 600),
          )
        : await createProject(trimmed, kind());
      props.onCreated(project);
    } catch (err) {
      // Nguyên văn từ lõi: chỉ nó biết thư mục có tồn tại, có đọc được, hay đã là dự án.
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell
      icon="plus"
      title="Dự án mới"
      desc="Trỏ vào một thư mục đã có trên máy."
      more="Không có tệp nào bị tạo hay sửa trong thư mục đó."
      busy={busy()}
      width="lg"
      onClose={() => {
        if (!busy()) props.onClose();
      }}
      footer={() => (
        <>
          <Show when={busy()}>
            <span class="mr-auto text-2xs text-muted" role="status" aria-live="polite">
              Đang mở dự án…
            </span>
          </Show>
          <Button onClick={props.onClose} disabled={busy()}>
            Huỷ
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={busy() || path().trim() === ""}
          >
            Tạo dự án
          </Button>
        </>
      )}
    >
      <div role="radiogroup" aria-label="Loại dự án" class="grid gap-sm sm:grid-cols-2">
        <For each={KINDS}>
          {(option) => (
            <button
              type="button"
              role="radio"
              aria-checked={kind() === option.id}
              disabled={busy()}
              onClick={() => setKind(option.id)}
              class="flex flex-col gap-2xs rounded-card border p-(--card-pad-x) text-left transition-colors duration-[var(--dur-fast)] disabled:opacity-50"
              classList={{
                "border-line bg-surface-soft hover:border-line-strong": kind() !== option.id,
                "border-accent bg-accent-soft": kind() === option.id,
              }}
            >
              <span class="flex items-center gap-2xs text-sm font-medium text-ink">
                <Icon name={option.icon} size={15} />
                {option.label}
              </span>
              <span class="text-2xs text-muted">{option.can}</span>
              <span class="text-2xs text-faint">{option.cannot}</span>
            </button>
          )}
        </For>
      </div>

      {/* Câu này ở lại ngoài hai thẻ chọn: nó nói về *hậu quả của việc chọn sai*, thứ
          không thuộc về riêng thẻ nào và cũng là thứ người dùng chỉ hiểu ra rất muộn. */}
      <p class="m-0 flex items-center gap-2xs text-2xs text-faint">
        Đổi loại sau này nghĩa là tạo lại dự án.
        <InfoDot text="Chọn nhầm loại thì trợ lý sẽ không sửa được tệp mà không nói rõ vì sao. Đổi loại sau này nghĩa là tạo lại dự án — thư mục thì vẫn nguyên." />
      </p>

      <div class="flex flex-col gap-2xs">
        <label class="flex flex-col gap-2xs">
          <span class="text-2xs text-faint">Thư mục</span>
          <div class="flex gap-sm">
            <input
              type="text"
              value={path()}
              spellcheck={false}
              autocapitalize="off"
              autocomplete="off"
              placeholder="/Users/ban/Workspaces/du-an"
              disabled={busy()}
              onInput={(event) => setPath(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void submit();
                }
              }}
              class="h-(--cta-h) min-w-0 flex-1 rounded-btn border border-line bg-bg px-sm font-mono text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50"
            />
            <Button icon="folder-open" variant="outline" disabled={busy()} onClick={() => void choose()}>
              Chọn…
            </Button>
          </div>
        </label>
        <p class="m-0 text-2xs text-faint">
          Kéo một thư mục vào cửa sổ cũng điền được.
        </p>
      </div>

      <Show when={error()}>
        {(message) => (
          <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
            {message()}
          </p>
        )}
      </Show>
    </DialogShell>
  );
}

/**
 * Hai loại, và mô tả của chúng nói bằng **việc trợ lý làm được**, không bằng tên loại.
 *
 * "Dự án mã nguồn" và "thư viện tài liệu" là hai cái tên nghe rõ ràng mà không nói gì về
 * hậu quả. Cái người dùng cần biết ở đúng khoảnh khắc này là: bên nào sửa được tệp.
 */
const KINDS: { id: ProjectKind; label: string; icon: IconName; can: string; cannot: string }[] = [
  {
    id: "code",
    label: "Mã nguồn",
    icon: "code",
    can: "Trợ lý đọc, sửa tệp và chạy lệnh.",
    cannot: "Mỗi thao tác ghi đều hỏi ý bạn trước.",
  },
  {
    id: "docs",
    label: "Thư viện tài liệu",
    icon: "library",
    can: "Trợ lý tìm và đọc tài liệu để trả lời.",
    cannot: "Không sửa tệp, không chạy lệnh.",
  },
];
