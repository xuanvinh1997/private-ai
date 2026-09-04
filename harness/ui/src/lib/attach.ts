import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { inTauri } from "./agent";
import { isDemo } from "./demo";
import { S, t } from "./i18n";

/** Message attachments: the OS dialog and drag-and-drop both yield absolute paths and share `resolveAttachments`. */

/** TypeScript mirror of `Attachment` in `app/src/commands/attach.rs`. */
export interface Attachment {
  path: string;
  error: string | null;
}

/** OS file dialog, unfiltered by extension. `[]` means the user cancelled; `null` means there is no dialog here. */
export async function pickFiles(): Promise<string[] | null> {
  if (!inTauri()) return null;
  const picked = await open({
    directory: false,
    multiple: true,
    title: t(S.libs.attach.pickTitle),
  });
  if (picked === null) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/** Ask the core whether these paths are attachable. Throws rather than swallowing: the user is watching for the drop. */
export async function resolveAttachments(paths: string[]): Promise<Attachment[]> {
  if (isDemo() || !inTauri()) return paths.map((path) => ({ path, error: null }));
  return await invoke<Attachment[]>("resolve_attachments", { paths });
}
