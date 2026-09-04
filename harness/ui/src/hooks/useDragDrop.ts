import { onCleanup, onMount } from "solid-js";

interface DropOwner {
  id: number;
  onPaths: (paths: string[]) => void;
}

let nextOwner = 0;
const owners: DropOwner[] = [];

/** Files dropped on the window, with absolute paths; only Tauri's onDragDropEvent exposes the real path. */
export function useDragDrop(onPaths: (paths: string[]) => void) {
  const owner: DropOwner = { id: nextOwner++, onPaths };
  let unlisten: (() => void) | undefined;
  let disposed = false;

  onMount(async () => {
    owners.push(owner);
    try {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const stop = await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        // Tauri reports a window-level drop, so every mounted consumer hears it. The newest mounted
        // consumer is the foreground surface (for example an upload dialog); only it may claim the files.
        if (owners[owners.length - 1]?.id !== owner.id) return;
        const paths = event.payload.paths;
        if (paths.length > 0) owner.onPaths(paths);
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
    const index = owners.findIndex((entry) => entry.id === owner.id);
    if (index !== -1) owners.splice(index, 1);
    unlisten?.();
  });
}
