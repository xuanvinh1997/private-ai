import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { inTauri } from "./agent";
import type { CloneProgress, DirEntry, Project, ProjectKind } from "./protocol";

/** Project and clone commands, split by error handling: `listProjects` runs at startup and swallows errors,
 * while everything else throws, because silence after a click is indistinguishable from slowness. */

/** One level of the project tree, never recursive: an unexpanded branch costs nothing. Failures read as "empty". */
export async function listDir(path: string): Promise<DirEntry[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<DirEntry[]>("list_dir", { path });
  } catch (err) {
    console.error("failed to read directory", path, err);
    return [];
  }
}

/** Copy user-selected files into the open project's root; the core rejects directories and collisions. */
export function importProjectFiles(paths: string[]): Promise<string[]> {
  return invoke<string[]>("import_project_files", { paths });
}

/** Permanently remove a file from the open document project and its derived index. */
export function deleteProjectDocument(path: string): Promise<void> {
  return invoke("delete_project_document", { path });
}

/** OS file dialog for project import; `null` is normalized to an empty batch. */
export async function pickProjectFiles(title?: string): Promise<string[]> {
  if (!inTauri()) return [];
  const picked = await open({ directory: false, multiple: true, title });
  if (picked === null) return [];
  return Array.isArray(picked) ? picked : [picked];
}

export async function listProjects(): Promise<Project[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<Project[]>("list_projects");
  } catch (err) {
    console.error("failed to list projects", err);
    return [];
  }
}

/** Open a directory: add it to the list if new, then switch to it. */
export function openProject(path: string): Promise<Project> {
  return invoke<Project>("open_project", { path });
}

/** Remove a project *from the list*; the directory on disk is untouched, and the wording must say so. */
export function removeProject(id: string): Promise<void> {
  return invoke("remove_project", { id });
}

/** Delete a project for good: its conversations and its indexed library, then its row. The folder on disk is
 * never touched. Slower than `removeProject` — dropping a library starts the document service — so callers
 * must show a busy state rather than assume this returns at once. */
export function deleteProject(id: string): Promise<void> {
  return invoke("delete_project", { id });
}

/** Close the open project and drop its disk-touching plugins; no row leaves the list, and the list comes back. */
export function closeProject(): Promise<Project[]> {
  return invoke<Project[]>("close_project");
}

/** Change a project's kind; the kind is set once at registration, so without this a mis-typed folder is a dead end. */
export function setProjectKind(id: string, kind: ProjectKind): Promise<Project[]> {
  return invoke<Project[]>("set_project_kind", { id, kind });
}

/** Display name derived from a path, used before the core returns a real `Project`. */
export function folderName(path: string): string {
  const parts = path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || path;
}




/** Create a project from an existing directory; the kind is chosen, never guessed, since guessing grants shell tools. */
export function createProject(path: string, kind: ProjectKind): Promise<Project> {
  return invoke<Project>("create_project", { path, kind });
}

export interface CloneRequest {
  url: string;
  parent: string;
  /** Target directory name; absent, the core derives it from the URL. */
  name?: string;
  /** `1` fetches only the most recent history; absent means a full clone. */
  depth?: number;
  /** Project kind after cloning, default `"code"` because the only entry point is the code group's clone button. */
  kind?: ProjectKind;
}

/** Clone a repo and open it as a project; progress rides a per-clone `Channel`, and git's own failure text is thrown. */
export function cloneProject(
  req: CloneRequest,
  onProgress: (p: CloneProgress) => void,
): Promise<Project> {
  const channel = new Channel<CloneProgress>();
  channel.onmessage = onProgress;
  // Flattened, not wrapped in `req`: the core takes one argument each, and a nested object would arrive unread.
  return invoke<Project>("clone_project", {
    url: req.url,
    parent: req.parent,
    name: req.name,
    depth: req.depth,
    kind: req.kind ?? "code",
    onProgress: channel,
  });
}

/** Cancel the running clone; errors are swallowed, as in `cancelTurn`, since `cloneProject` still has the last word. */
export async function cancelClone(): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("cancel_clone");
  } catch (err) {
    console.error("failed to cancel clone", err);
  }
}

/** OS directory dialog; `null` means cancelled, and also means "no dialog here" outside Tauri. Real plugin errors throw. */
export async function pickDirectory(title?: string): Promise<string | null> {
  if (!inTauri()) return null;
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

/** Host of an origin URL, for a badge; scp form (`git@host:user/repo.git`) is matched first since `new URL` rejects it. */
export function originHost(origin: string): string {
  const scp = /^[^@/]+@([^:/]+):/.exec(origin);
  if (scp?.[1] !== undefined) return scp[1];
  try {
    return new URL(origin).host || origin;
  } catch {
    return origin;
  }
}

/** Directory name derived from a repo URL, used as the default suggestion in the clone dialog. */
export function repoNameFromUrl(url: string): string {
  const cleaned = url.trim().replace(/\/+$/, "").replace(/\.git$/i, "");
  const tail = cleaned.split(/[/:]/).pop() ?? "";
  return tail;
}
