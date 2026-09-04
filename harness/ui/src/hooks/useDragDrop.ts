import { onCleanup, onMount } from "solid-js";

/** Files dropped on the window, with absolute paths; only Tauri's onDragDropEvent exposes the real path. */
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
      // The component may have unmounted during the await; registering late without unlistening leaks.
      if (disposed) stop();
      else unlisten = stop;
    } catch {
      /* outside Tauri: no drag & drop, which is a valid state */
    }
  });

  onCleanup(() => {
    disposed = true;
    unlisten?.();
  });
}
