import { Show } from "solid-js";
import { useDragDrop } from "../../hooks/useDragDrop";
import Icon from "../Icon";
import { InfoDot } from "../settings/FormKit";
import { Button } from "../projects/DialogShell";

/**
 * Vùng thả tệp.
 *
 * Kéo thả đi qua `onDragDropEvent` của Tauri (bọc trong `useDragDrop`) chứ **không** qua
 * sự kiện drop của HTML5. Trình duyệt cố ý không cho biết vị trí thật của tệp — nó chỉ
 * đưa một `File` không có đường dẫn — mà lõi lại cần đúng đường dẫn tuyệt đối để đọc.
 * Một vùng thả dựng bằng HTML5 sẽ "nhận" được tệp rồi không nạp được tệp nào.
 *
 * Chiếm cả một khối khi thư viện còn rỗng, co lại thành một hàng nút khi đã có tài liệu:
 * lúc rỗng thì đây là việc duy nhất cần làm trên màn hình, còn sau đó cái người dùng tới
 * để xem là danh sách tài liệu, không phải cái khung mời họ nạp thêm.
 *
 * Không có trạng thái "đang rê tệp qua": `useDragDrop` chỉ phát ra lúc **thả**, và hook
 * đó thuộc về người khác. Bù lại bằng một câu hướng dẫn luôn hiện, thay vì một hiệu ứng
 * chỉ xuất hiện đúng lúc không ai nhìn.
 */
export default function DropZone(props: {
  compact?: boolean;
  busy?: boolean;
  onPaths: (paths: string[]) => void;
  onPick: () => void;
}) {
  useDragDrop((paths) => {
    if (props.busy !== true) props.onPaths(paths);
  });

  return (
    <Show
      when={props.compact}
      fallback={
        <div class="flex flex-col items-center gap-md rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-4xl text-center">
          <span class="grid size-12 place-items-center rounded-panel bg-accent-soft text-accent-ink">
            <Icon name="upload" size={24} />
          </span>
          <div class="flex flex-col items-center gap-2xs">
            <p class="m-0 flex items-center gap-2xs text-sm font-medium text-ink">
              Thư viện còn trống
              <InfoDot text="Nhận PDF, Word, Markdown, HTML, CSV và văn bản thuần — tệp gốc nằm nguyên chỗ cũ, thư viện chỉ đọc nội dung." />
            </p>
            <p class="m-0 max-w-[46ch] text-xs text-muted">
              Kéo tệp vào cửa sổ, hoặc chọn tệp từ máy.
            </p>
          </div>
          <Button variant="primary" icon="plus" disabled={props.busy} onClick={props.onPick}>
            Chọn tệp…
          </Button>
        </div>
      }
    >
      <div class="flex flex-wrap items-center gap-sm rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
        <span class="text-faint">
          <Icon name="upload" size={15} />
        </span>
        <span class="flex-1 text-xs text-muted">Kéo tệp thả vào cửa sổ để nạp thêm.</span>
        <Button variant="outline" icon="plus" disabled={props.busy} onClick={props.onPick}>
          Chọn tệp…
        </Button>
      </div>
    </Show>
  );
}
