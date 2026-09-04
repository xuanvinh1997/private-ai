import { describe, expect, it, vi } from "vitest";
import {
  dismissDocumentTask,
  documentTasks,
  runDocumentTask,
  stopDocumentTask,
} from "./docs";
import type { IngestProgress } from "./protocol";

const initial = (path = "/docs"): IngestProgress => ({
  path,
  stage: "preparing",
  done: 0,
  total: 0,
  finished: false,
  error: null,
});

describe("document task lifecycle", () => {
  it("keeps live progress and the completed result outside the screen component", async () => {
    const scope = "task-progress";
    await runDocumentTask(scope, "add", initial("guide.pdf"), async (note) => {
      note({ ...initial("guide.pdf"), stage: "reading", total: 1 });
      note({ ...initial("guide.pdf"), stage: "stored", done: 1, total: 1 });
      note({
        ...initial("/docs"),
        stage: "embedding",
        error: "Qdrant unavailable; keyword search remains available",
      });
      return [];
    });

    expect(documentTasks()[scope]).toMatchObject({
      kind: "add",
      state: "completed",
      stored: 1,
      warning: "Qdrant unavailable; keyword search remains available",
      documents: [],
    });
    dismissDocumentTask(scope);
    expect(documentTasks()[scope]).toBeUndefined();
  });

  it("reuses the running task instead of starting a conflicting pass", async () => {
    const scope = "task-single-flight";
    let finish: ((documents: []) => void) | undefined;
    const firstExecute = vi.fn(
      () => new Promise<[]>((resolve) => {
        finish = resolve;
      }),
    );
    const secondExecute = vi.fn(async () => []);

    const first = runDocumentTask(scope, "sync", initial(), firstExecute);
    const second = runDocumentTask(scope, "reprocess", initial(), secondExecute);
    expect(first).toBe(second);
    expect(firstExecute).toHaveBeenCalledOnce();
    expect(secondExecute).not.toHaveBeenCalled();

    finish?.([]);
    await first;
    dismissDocumentTask(scope);
  });

  it("keeps cancellation separate from failure and ignores later progress", async () => {
    const scope = "task-cancel";
    let finish: ((documents: []) => void) | undefined;
    let noteProgress: ((progress: IngestProgress) => void) | undefined;
    const running = runDocumentTask(
      scope,
      "add",
      initial("large.pdf"),
      (note) => {
        noteProgress = note;
        return new Promise<[]>((resolve) => {
          finish = resolve;
        });
      },
    );

    expect(await stopDocumentTask(scope)).toBe(true);
    noteProgress?.({ ...initial("large.pdf"), stage: "stored", done: 1, total: 1 });
    finish?.([]);
    await running;

    expect(documentTasks()[scope]).toMatchObject({
      state: "cancelled",
      stored: 0,
      error: null,
      documents: [],
    });
    dismissDocumentTask(scope);
  });
});
