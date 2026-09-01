import { Show } from "solid-js";
import { Chip, IconButton } from "./primitives";

/**
 * Đầu khu làm việc.
 *
 * Cả dải là vùng kéo cửa sổ vì cửa sổ mở ở chế độ "Overlay" — không có thanh tiêu đề
 * nào khác để kéo. Mọi control bên trong tự khai `no-drag` qua luật chung trong app.css,
 * nên thêm nút vào đây không kéo theo việc phải nhớ luật đó.
 */
export default function WorkspaceHeader(props: {
  title: string;
  model?: string;
  scope?: string;
  busy: boolean;
  /** Chữ đứng cạnh tiêu đề lúc bận. Mặc định là lượt đang chạy. */
  busyLabel?: string;
  sessionPanelOpen: boolean;
  changesPanelOpen: boolean;
  changeCount: number;
  onToggleSessionPanel: () => void;
  onToggleChangesPanel: () => void;
  onSearch: () => void;
}) {
  return (
    <header
      class="flex h-(--header-h) shrink-0 items-center gap-sm border-b border-line bg-bg px-md"
      data-tauri-drag-region
    >
      <IconButton
        icon="panel-left"
        label={props.sessionPanelOpen ? "Ẩn danh sách phiên" : "Hiện danh sách phiên"}
        active={props.sessionPanelOpen}
        onClick={props.onToggleSessionPanel}
      />

      <div class="flex min-w-0 flex-1 flex-col">
        <div class="flex min-w-0 items-center gap-sm">
          <h1 class="m-0 min-w-0 truncate text-base font-semibold text-ink">{props.title}</h1>
          <Show when={props.busy}>
            <span class="shrink-0 text-2xs text-accent" role="status" aria-live="polite">
              {props.busyLabel ?? "đang chạy…"}
            </span>
          </Show>
        </div>
        <div class="flex min-w-0 items-center gap-2xs">
          <Show when={props.model}>{(model) => <Chip>{model()}</Chip>}</Show>
          <Show when={props.scope}>
            {(scope) => (
              <span class="min-w-0 truncate font-mono text-2xs text-faint" title={scope()}>
                {scope()}
              </span>
            )}
          </Show>
        </div>
      </div>

      <IconButton icon="search" label="Tìm phiên" keys="Meta+K Control+K" onClick={props.onSearch} />
      <span class="relative inline-flex">
        <IconButton
          icon="panel-right"
          label={props.changesPanelOpen ? "Đóng bảng thay đổi" : "Mở bảng thay đổi"}
          active={props.changesPanelOpen}
          onClick={props.onToggleChangesPanel}
        />
        {/* Số tệp đã đổi nằm trên nút vì bảng thường đóng: không có nó thì thay đổi của
            một lượt dài chỉ tồn tại nếu người dùng nhớ đi mở bảng ra xem. */}
        <Show when={props.changeCount > 0 && !props.changesPanelOpen}>
          <span
            aria-hidden="true"
            class="pointer-events-none absolute -top-3xs -right-3xs grid min-w-4 place-items-center rounded-pill bg-accent px-3xs text-[10px] leading-4 text-on-accent tabular-nums"
          >
            {props.changeCount}
          </span>
        </Show>
      </span>
    </header>
  );
}
