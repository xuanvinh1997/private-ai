import { Popover } from "@kobalte/core/popover";
import { AlertTriangle, Bell, CheckCircle2, Info } from "lucide-solid";
import { For, Show, createMemo, createSignal } from "solid-js";
import { formatRelativeTime } from "../format";

export type NoticeTone = "alert" | "warn" | "info";

export interface Notice {
  id: string;
  tone: NoticeTone;
  title: string;
  detail: string;
  /** Set for one-off events; status alerts stay unread while the problem lasts. */
  at?: string;
  actionLabel?: string;
  onAction?: () => void;
}

const SEEN_KEY = "private-ai-notifications-seen";

const readSeen = () => {
  if (typeof window === "undefined") return "";
  return window.localStorage.getItem(SEEN_KEY) ?? "";
};

const noticeIcon = (tone: NoticeTone) =>
  tone === "alert" ? <AlertTriangle size={17} /> : tone === "warn" ? <Info size={17} /> : <CheckCircle2 size={17} />;

export function NotificationsMenu(props: { notices: Notice[]; onOpen: () => void }) {
  const [seenAt, setSeenAt] = createSignal(readSeen());
  const isUnread = (notice: Notice) =>
    notice.at ? notice.at > seenAt() : notice.tone === "alert";
  const unread = createMemo(() => props.notices.filter(isUnread).length);

  // Marking on close keeps the unread highlight visible while the panel is being read.
  const handleOpenChange = (open: boolean) => {
    if (open) {
      props.onOpen();
      return;
    }
    const now = new Date().toISOString();
    window.localStorage.setItem(SEEN_KEY, now);
    setSeenAt(now);
  };

  return (
    <Popover placement="bottom-end" gutter={8} onOpenChange={handleOpenChange}>
      <Popover.Trigger
        type="button"
        class="icon-button notification-trigger"
        aria-label={unread() ? `Thông báo, ${unread()} mục mới` : "Thông báo"}
      >
        <Bell size={19} />
        <Show when={unread()}>
          <span class="notification-badge">{unread() > 9 ? "9+" : unread()}</span>
        </Show>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="notification-panel">
          <Popover.Title class="notification-heading">Thông báo</Popover.Title>
          <Show
            when={props.notices.length > 0}
            fallback={
              <p class="notification-empty">
                Mọi thứ đang chạy bình thường. Chưa có gì cần bạn để mắt tới.
              </p>
            }
          >
            <ul class="notification-list">
              <For each={props.notices}>{(notice) => (
                <li classList={{ "notification-item": true, unread: isUnread(notice) }}>
                  <span class={`notification-icon tone-${notice.tone}`} aria-hidden="true">
                    {noticeIcon(notice.tone)}
                  </span>
                  <div class="notification-copy">
                    <strong>{notice.title}</strong>
                    <small>{notice.detail}</small>
                    <Show when={notice.onAction && notice.actionLabel}>
                      <button class="notification-action" type="button" onClick={notice.onAction}>{notice.actionLabel}</button>
                    </Show>
                  </div>
                  <Show when={notice.at}>
                    <time datetime={notice.at}>{formatRelativeTime(notice.at!)}</time>
                  </Show>
                </li>
              )}</For>
            </ul>
          </Show>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  );
}
