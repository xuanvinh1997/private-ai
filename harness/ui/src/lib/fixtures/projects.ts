import type { CloneProgress, Project, ProjectKind } from "../protocol";

/** Sample data for the projects screen under `?demo=1`, chosen by which states must be visible: one open project,
 * one cloned (origin badge), one document library, and an older code project so the kind filter has work. */

/** The project the core would return after creation, so the demo dialog has something to hand back. */
export function demoCreatedProject(path: string, kind: ProjectKind): Project {
  const name = path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || path;
  return {
    id: `demo-${path}`,
    name,
    path,
    lastOpenedAt: Date.now(),
    isCurrent: true,
    kind,
    origin: null,
  };
}

/** A fake clone reproducing the awkward part: the first two phases have no `percent`, where a bar stuck at 0% looks hung. */
export function demoCloneFrames(url: string, path: string): CloneProgress[] {
  const step = (
    phase: string,
    percent: number | null,
    line: string | null,
  ): CloneProgress => ({ phase, percent, line, finished: false, path: null, error: null });

  return [
    step("Đang kết nối", null, `Cloning into '${path}'...`),
    step("Đang đếm đối tượng", null, "remote: Enumerating objects: 4821, done."),
    step("Đang nhận đối tượng", 12, "Receiving objects:  12% (579/4821)"),
    step("Đang nhận đối tượng", 48, "Receiving objects:  48% (2314/4821)"),
    step("Đang nhận đối tượng", 91, "Receiving objects:  91% (4387/4821)"),
    step("Đang giải nén", 100, "Resolving deltas: 100% (2140/2140), done."),
    { phase: "Xong", percent: 100, line: `Đã clone ${url}`, finished: true, path, error: null },
  ];
}
