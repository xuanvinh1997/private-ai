import { createSignal } from "solid-js";

/** Durable, app-lifetime notices. Toasts may disappear quickly, but their useful history remains here. */
export type AppNotificationTone = "info" | "success" | "warning" | "error" | "progress";

export interface AppNotificationProgress {
  done: number;
  total: number;
  label: string;
}

export interface AppNotification {
  id: string;
  tone: AppNotificationTone;
  title: string;
  message: string;
  detail?: string;
  progress?: AppNotificationProgress;
  dismissible: boolean;
  read: boolean;
  createdAt: number;
  updatedAt: number;
}

export type AppNotificationInput = Pick<
  AppNotification,
  "id" | "tone" | "title" | "message"
> &
  Partial<Pick<AppNotification, "detail" | "progress" | "dismissible">>;

const MAX_NOTIFICATIONS = 50;
const [appNotifications, setAppNotifications] = createSignal<AppNotification[]>([]);

export { appNotifications };

/** Add or refresh a stable notification. Progress refreshes preserve read state; a lifecycle boundary may announce again. */
export function upsertAppNotification(input: AppNotificationInput, announce = false): void {
  const now = Date.now();
  setAppNotifications((all) => {
    const existing = all.find((item) => item.id === input.id);
    const next: AppNotification = {
      ...input,
      dismissible: input.dismissible ?? input.tone !== "progress",
      read: announce ? false : (existing?.read ?? false),
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
    };
    if (existing !== undefined) {
      // A rapid progress stream must not keep moving an item under the user's pointer.
      return all.map((item) => (item.id === input.id ? next : item));
    }
    return [next, ...all].slice(0, MAX_NOTIFICATIONS);
  });
}

export function markAllAppNotificationsRead(): void {
  setAppNotifications((all) =>
    all.every((item) => item.read)
      ? all
      : all.map((item) => (item.read ? item : { ...item, read: true })),
  );
}

export function dismissAppNotification(id: string): void {
  setAppNotifications((all) => all.filter((item) => item.id !== id));
}

/** Keep running jobs visible; only notices whose outcome is already known can be cleared in bulk. */
export function clearFinishedAppNotifications(): void {
  setAppNotifications((all) => all.filter((item) => item.tone === "progress"));
}
