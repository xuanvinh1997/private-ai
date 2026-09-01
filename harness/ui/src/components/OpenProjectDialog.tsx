import { createSignal, Show } from "solid-js";
import { useFocusTrap } from "../hooks/useFocusTrap";
import Icon from "./Icon";

/**
 * Hộp thoại "Mở thư mục…" khi **chưa có** hộp thoại chọn thư mục của hệ điều hành.
 *
 * `@tauri-apps/plugin-dialog` chưa được cài, và tự thêm nó từ phía giao diện là sửa cả
 * `Cargo.toml` lẫn danh sách quyền — việc của phía Rust. Cho tới lúc đó thì một ô nhập
 * đường dẫn vẫn mở được dự án, còn kéo thả thư mục vào cửa sổ là lối không cần gõ. Cái
 * *không* chấp nhận được là mục "Mở thư mục…" bấm vào rồi không có gì xảy ra.
 *
 * Không kiểm đường dẫn ở đây: chỉ lõi mới biết thư mục có tồn tại và đọc được không, và
 * một luật đoán ở phía giao diện sẽ chặn nhầm đúng những đường dẫn lạ mà nó chưa từng
 * thấy. Gõ sai thì lỗi từ lõi hiện ngay dưới ô nhập.
 */
export default function OpenProjectDialog(props: {
  busy: boolean;
  error: string | null;
  onSubmit: (path: string) => void;
  onClose: () => void;
}) {
  let panel: HTMLDivElement | undefined;
  const [path, setPath] = createSignal("");

  useFocusTrap(() => panel, props.onClose);

  const submit = () => {
    const trimmed = path().trim();
    if (trimmed !== "" && !props.busy) props.onSubmit(trimmed);
  };

  return (
    <div
      class="fixed inset-0 z-50 flex items-start justify-center p-4xl"
      style={{ background: "var(--scrim)" }}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby="open-project-title"
        class="flex w-full max-w-[560px] flex-col gap-md rounded-card border border-line bg-surface p-(--dialog-pad-x) shadow-pop"
      >
        <div class="flex items-start gap-sm">
          <span class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
            <Icon name="folder-open" size={16} />
          </span>
          <div class="flex min-w-0 flex-col gap-3xs">
            <h2 id="open-project-title" class="m-0 text-md font-semibold text-ink">
              Mở thư mục dự án
            </h2>
            <p class="m-0 text-xs text-muted">
              Dán đường dẫn tuyệt đối tới thư mục. Kéo thẳng một thư mục vào cửa sổ cũng mở
              được nó.
            </p>
          </div>
        </div>

        <label class="flex flex-col gap-2xs">
          <span class="text-2xs text-faint">Đường dẫn</span>
          <input
            type="text"
            value={path()}
            spellcheck={false}
            autocapitalize="off"
            autocomplete="off"
            placeholder="/Users/ban/Workspaces/du-an"
            disabled={props.busy}
            onInput={(event) => setPath(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                submit();
              }
            }}
            class="h-(--cta-h) rounded-btn border border-line bg-bg px-sm font-mono text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:opacity-50"
          />
        </label>

        <Show when={props.error}>
          {(message) => (
            <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs text-danger" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <div class="flex items-center justify-end gap-sm">
          <Show when={props.busy}>
            <span class="mr-auto text-2xs text-muted" role="status" aria-live="polite">
              Đang mở dự án…
            </span>
          </Show>
          <button
            type="button"
            onClick={props.onClose}
            class="h-(--control-h) rounded-btn px-md text-xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
          >
            Huỷ
          </button>
          <button
            type="button"
            onClick={submit}
            disabled={props.busy || path().trim() === ""}
            class="h-(--control-h) rounded-btn bg-accent px-md text-xs font-medium text-on-accent transition-colors duration-[var(--dur-fast)] enabled:hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-40"
          >
            Mở
          </button>
        </div>
      </div>
    </div>
  );
}
