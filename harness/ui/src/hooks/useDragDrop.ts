import { onCleanup, onMount } from "solid-js";

/**
 * Nhận tệp thả vào cửa sổ, kèm **đường dẫn tuyệt đối**.
 *
 * HTML5 drag & drop cố ý không cho biết đường dẫn thật của tệp — trình duyệt chỉ đưa
 * một `File` không có vị trí. Với một coding agent thì đường dẫn *chính là* thứ cần,
 * nên phải đi qua `onDragDropEvent` của Tauri, tức là qua tầng hệ điều hành.
 *
 * Import động và bọc try/catch vì `npm run dev` trong trình duyệt thường không có
 * runtime Tauri: ở đó việc kéo thả đơn giản là không có, chứ không phải là màn hình trắng.
 */
export function useDragDrop(onPaths: (paths: string[]) => void) {
  let unlisten: (() => void) | undefined;
  let disposed = false;

  onMount(async () => {
    try {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const stop = await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const paths = event.payload.paths;
        if (paths.length > 0) onPaths(paths);
      });
      // Component có thể đã bị gỡ trong lúc chờ await; đăng ký muộn mà không gỡ là rò rỉ.
      if (disposed) stop();
      else unlisten = stop;
    } catch {
      /* ngoài Tauri: không có kéo thả, và đó là trạng thái hợp lệ */
    }
  });

  onCleanup(() => {
    disposed = true;
    unlisten?.();
  });
}
