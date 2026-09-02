import { Show } from "solid-js";
import { BrandMark } from "./Brand";
import { IconButton } from "./primitives";

/**
 * Thanh trên của khu làm việc. Cố ý mỏng: tiêu đề, trạng thái lượt, và đúng một nút.
 *
 * Bản trước còn mang tên mô hình và đường dẫn tệp đang mở. Cả hai đã đi chỗ khác — mô hình
 * xuống ô soạn tin (nó là thuộc tính của tin nhắn, không phải của cửa sổ), còn tệp thì
 * không còn màn hình nào để mở. Cái còn lại đúng bằng thứ ChatGPT giữ trên thanh này.
 *
 * Nút duy nhất ở bên phải là công tắc bảng thay đổi — đối chiếu với "diff panel toggle"
 * của Codex, thứ nó cũng để ở đây. Nó chỉ xuất hiện khi có bảng để bật.
 *
 * Tên dự án đứng trước tiêu đề như một mẩu đường dẫn, và **chỉ khi có dự án đang mở**.
 * Không có thì cả mẩu ấy biến mất chứ không để lại một dấu gạch chéo trơ ra — một nhãn
 * rỗng nằm cạnh tiêu đề đọc ra là lỗi vẽ chứ không đọc ra là "chưa có gì". Chỗ nói về
 * trạng thái không-dự-án vẫn là ô soạn tin và màn hình trống, nơi còn đủ chỗ để nói cả
 * *vì sao*. Cùng lẽ đó, chỗ gọi bỏ `onToggleChangesPanel` khi không có dự án: không tool
 * nào chạm được tới đĩa thì bảng ấy vĩnh viễn trống.
 *
 * Khi thanh bên thu lại, dấu hiệu thương hiệu chuyển sang đứng ở đây. Nó sống ở đầu thanh
 * bên, nên thanh bên đóng lại là cả cửa sổ không còn chỗ nào xưng tên ứng dụng — và đó
 * đúng là lúc người dùng dễ quên mình đang ở cửa sổ nào nhất, vì cột trái vừa biến mất.
 *
 * Cả dải là vùng kéo cửa sổ vì cửa sổ mở ở chế độ "Overlay" — không có thanh tiêu đề nào
 * khác để kéo. Mọi control bên trong tự khai `no-drag` qua luật chung trong app.css, nên
 * thêm nút vào đây không kéo theo việc phải nhớ luật đó.
 */
export default function WorkspaceHeader(props: {
  title: string;
  /** Dự án đang mở. Vắng mặt nghĩa là chưa mở dự án nào — không phải một chuỗi rỗng. */
  scope?: string;
  busy: boolean;
  /** Chữ đứng cạnh tiêu đề lúc bận. Mặc định là lượt đang chạy. */
  busyLabel?: string;
  /**
   * Thanh bên đang mở hay không.
   *
   * Đóng thì thanh này phải tự chừa chỗ cho ba nút giao thông của macOS: chúng nằm đè lên
   * góc trên trái của cửa sổ, và khi thanh bên biến mất thì góc đó là của thanh này. Không
   * chừa thì nút đóng cửa sổ đè thẳng lên nút mở lại thanh bên.
   */
  sidebarOpen: boolean;
  onOpenSidebar: () => void;
  /** Công tắc bảng thay đổi. Vắng mặt nghĩa là màn hình này không có bảng nào để bật. */
  changesPanelOpen?: boolean;
  changeCount?: number;
  onToggleChangesPanel?: () => void;
}) {
  return (
    <header
      class="flex h-(--header-h) shrink-0 items-center gap-sm bg-bg px-md"
      classList={{ "pl-(--traffic-lights-w)": !props.sidebarOpen }}
      data-tauri-drag-region
    >
      <Show when={!props.sidebarOpen}>
        <IconButton icon="panel-left" label="Hiện thanh bên" onClick={props.onOpenSidebar} />
        <BrandMark size={22} class="shrink-0 text-accent" />
      </Show>

      <div class="flex min-w-0 flex-1 items-baseline gap-xs">
        {/* Tên dự án nhường chỗ trước: cắt cụt cái tiêu đề mà người dùng vừa bấm để mở là
            cắt mất thứ họ đang tìm, còn tên dự án thì họ đã đọc ở cột trái rồi. */}
        <Show when={props.scope}>
          {(scope) => (
            <>
              <span class="min-w-0 max-w-40 shrink truncate text-sm text-muted">{scope()}</span>
              <span class="shrink-0 text-sm text-faint" aria-hidden="true">
                /
              </span>
            </>
          )}
        </Show>
        {/* `text-lg` chứ không `text-base`: thanh này cao 56px và chỉ mang đúng một dòng
            chữ, nên một tiêu đề cỡ chữ thân bài đứng trong đó đọc ra là một mẩu nhãn bị bỏ
            quên chứ không ra là tên của thứ đang mở. */}
        <h1 class="m-0 min-w-0 truncate text-lg font-semibold text-ink">{props.title}</h1>
        <Show when={props.busy}>
          <span class="shrink-0 text-2xs text-accent" role="status" aria-live="polite">
            {props.busyLabel ?? "đang chạy…"}
          </span>
        </Show>
      </div>

      <Show when={props.onToggleChangesPanel}>
        {(toggle) => (
          <span class="relative inline-flex">
            <IconButton
              icon="panel-right"
              label={props.changesPanelOpen ? "Đóng bảng thay đổi" : "Mở bảng thay đổi"}
              active={props.changesPanelOpen}
              onClick={() => toggle()()}
            />
            {/* Số tệp đã đổi nằm trên nút vì bảng thường đóng: không có nó thì thay đổi của
                một lượt dài chỉ tồn tại nếu người dùng nhớ đi mở bảng ra xem. */}
            <Show when={(props.changeCount ?? 0) > 0 && props.changesPanelOpen !== true}>
              <span
                aria-hidden="true"
                class="pointer-events-none absolute -top-3xs -right-3xs grid min-w-4 place-items-center rounded-pill bg-accent px-3xs text-[10px] leading-4 text-on-accent tabular-nums"
              >
                {props.changeCount}
              </span>
            </Show>
          </span>
        )}
      </Show>
    </header>
  );
}
