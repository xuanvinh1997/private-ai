import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { inTauri } from "./agent";
import { isDemo } from "./demo";
import { S, t } from "./i18n";

/** Message attachments: the OS dialog and drag-and-drop both yield absolute paths and share `resolveAttachments`.
 * A file inside the project is attached where it lies; anything from outside is copied into the session's own
 * attachment folder by the core, which is why the session id travels with the batch. */

/** TypeScript mirror of `Attachment` in `app/src/commands/attach.rs`. */
export interface Attachment {
  /** Where the composer should point: the original path for a project file, the core's copy for anything else. */
  path: string;
  error: string | null;
  /** The file was read through the document library rather than left as a path: PDFs, images and DOCX. */
  extracted: boolean;
}

/** One attached file as the composer holds it, from the moment the core placed it until the message is sent.
 * A path in the draft was never the right shape: the user cannot see what is attached without reading a path,
 * and removing one means editing text. */
export interface Attached {
  /** Where the file actually is: the original for a project file, the core's copy for anything else. */
  path: string;
  /** File name alone. A chip has no room for a path, and the full path stays in the tooltip. */
  name: string;
  /** Read through the library rather than by path: PDFs, images and DOCX. */
  extracted: boolean;
}

/** The last segment of a path, with either separator, so a chip reads as a file name on both platforms. */
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part !== "");
  return parts[parts.length - 1] ?? path;
}

export function attached(entry: Attachment): Attached {
  return { path: entry.path, name: fileName(entry.path), extracted: entry.extracted };
}

/** The message that actually goes to the model: the user's words, then the files under one heading, each with
 * the tool that opens it. The list is built at send time rather than typed into the draft, so the composer can
 * show chips while the model still receives something unambiguous. */
export function withAttachments(text: string, files: Attached[]): string {
  if (files.length === 0) return text.trim();
  const lines = files.map(
    (file) =>
      `- ${file.path} (${t(
        file.extracted ? S.chat.composer.attachedByLibrary : S.chat.composer.attachedByRead,
      )})`,
  );
  const block = `${t(S.chat.composer.attachedHeading)}\n${lines.join("\n")}`;
  const said = text.trim();
  return said === "" ? block : `${said}\n\n${block}`;
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

/** Ask the core to place these paths, copying in the ones from outside the project. Throws rather than
 * swallowing: the user is watching for the drop. */
export async function resolveAttachments(
  paths: string[],
  sessionId: string,
): Promise<Attachment[]> {
  if (isDemo() || !inTauri())
    return paths.map((path) => ({ path, error: null, extracted: false }));
  return await invoke<Attachment[]>("resolve_attachments", { paths, sessionId });
}
