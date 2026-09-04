import { describe, expect, it } from "vitest";
import {
  appNotifications,
  clearFinishedAppNotifications,
  dismissAppNotification,
  markAllAppNotificationsRead,
  upsertAppNotification,
} from "./notifications";

describe("app notification lifecycle", () => {
  it("keeps progress under one id and announces lifecycle boundaries", () => {
    const id = "notification-progress";
    upsertAppNotification(
      {
        id,
        tone: "progress",
        title: "Indexing",
        message: "Reading",
        progress: { done: 1, total: 3, label: "1/3 files" },
      },
      true,
    );
    markAllAppNotificationsRead();

    upsertAppNotification({
      id,
      tone: "progress",
      title: "Indexing",
      message: "Reading",
      progress: { done: 2, total: 3, label: "2/3 files" },
    });
    expect(appNotifications().find((item) => item.id === id)).toMatchObject({
      read: true,
      progress: { done: 2, total: 3 },
    });

    upsertAppNotification(
      {
        id,
        tone: "success",
        title: "Indexing",
        message: "Completed",
      },
      true,
    );
    expect(appNotifications().find((item) => item.id === id)).toMatchObject({
      read: false,
      tone: "success",
      dismissible: true,
    });
    dismissAppNotification(id);
  });

  it("clears finished notices without hiding running work", () => {
    const running = "notification-running";
    const finished = "notification-finished";
    upsertAppNotification({
      id: running,
      tone: "progress",
      title: "OCR",
      message: "Running",
    });
    upsertAppNotification({
      id: finished,
      tone: "error",
      title: "Upload",
      message: "Failed",
    });

    clearFinishedAppNotifications();
    expect(appNotifications().some((item) => item.id === running)).toBe(true);
    expect(appNotifications().some((item) => item.id === finished)).toBe(false);
    dismissAppNotification(running);
  });
});
