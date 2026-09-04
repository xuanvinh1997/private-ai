import {
  createEffect,
  createMemo,
  createSignal,
  createUniqueId,
  For,
  onCleanup,
  Show,
} from "solid-js";
import {
  appNotifications,
  clearFinishedAppNotifications,
  dismissAppNotification,
  markAllAppNotificationsRead,
  type AppNotification,
} from "../lib/notifications";
import { locale, S, t } from "../lib/i18n";
import Icon, { type IconName } from "./Icon";
import { IconButton } from "./primitives";

/** Global notification centre. It lives in the workspace header so every screen shares the same history. */
export default function NotificationBell() {
  const [open, setOpen] = createSignal(false);
  const id = createUniqueId();
  let popup: HTMLDivElement | undefined;
  let trigger: HTMLButtonElement | undefined;

  const unread = createMemo(() => appNotifications().filter((item) => !item.read).length);
  const hasFinished = createMemo(() =>
    appNotifications().some((item) => item.tone !== "progress"),
  );
  const buttonLabel = () =>
    unread() > 0
      ? t(S.app.notifications.openCount, { n: unread() })
      : t(S.app.notifications.open);

  const close = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) trigger?.focus();
  };

  const toggle = () => {
    setOpen((value) => !value);
  };

  // A notice arriving while the panel is open is already visible and should not light the badge again.
  createEffect(() => {
    if (open() && unread() > 0) queueMicrotask(markAllAppNotificationsRead);
  });

  const onDocPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (popup?.contains(target ?? null) || trigger?.contains(target ?? null)) return;
    setOpen(false);
  };
  document.addEventListener("pointerdown", onDocPointerDown, true);
  onCleanup(() => document.removeEventListener("pointerdown", onDocPointerDown, true));

  return (
    <div class="relative inline-flex shrink-0">
      {/* One complete phrase is announced when the count changes; the visual badge stays decorative. */}
      <span class="sr-only" role="status" aria-live="polite" aria-atomic="true">
        <Show when={unread() > 0}>{t(S.app.notifications.unreadStatus, { n: unread() })}</Show>
      </span>
      <IconButton
        ref={(element) => (trigger = element)}
        icon="bell"
        label={buttonLabel()}
        active={open()}
        expanded={open()}
        controls={id}
        hasPopup="dialog"
        onClick={toggle}
      />
      <Show when={unread() > 0}>
        <span
          aria-hidden="true"
          class="pointer-events-none absolute -top-2xs -right-2xs grid min-w-5 place-items-center rounded-pill border-2 border-bg bg-danger px-3xs text-2xs leading-4 text-white tabular-nums"
        >
          {unread() > 99 ? "99+" : unread()}
        </span>
      </Show>

      <Show when={open()}>
        <div
          ref={popup}
          id={id}
          role="dialog"
          aria-label={t(S.app.notifications.title)}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            close(true);
          }}
          class="absolute top-full right-0 z-[var(--z-popover)] mt-3xs flex max-h-[min(30rem,calc(100vh-var(--header-h)-1rem))] w-[min(23rem,calc(100vw-2rem))] flex-col overflow-hidden rounded-menu border border-line bg-surface shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        >
          <div class="flex shrink-0 items-center justify-between gap-sm border-b border-line px-md py-sm">
            <h2 class="m-0 text-sm font-medium text-ink">{t(S.app.notifications.title)}</h2>
            <Show when={hasFinished()}>
              <button
                type="button"
                onClick={clearFinishedAppNotifications}
                class="shrink-0 rounded-btn px-xs py-2xs text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
              >
                {t(S.app.notifications.clearFinished)}
              </button>
            </Show>
          </div>

          <Show
            when={appNotifications().length > 0}
            fallback={
              <div class="flex min-h-40 flex-col items-center justify-center gap-xs px-xl py-2xl text-center">
                <span class="grid size-9 place-items-center rounded-panel bg-surface-soft text-muted">
                  <Icon name="bell" size={16} />
                </span>
                <p class="m-0 text-sm font-medium text-ink">{t(S.app.notifications.empty)}</p>
                <p class="m-0 max-w-64 text-xs leading-relaxed text-muted">
                  {t(S.app.notifications.emptyMore)}
                </p>
              </div>
            }
          >
            <ul class="m-0 min-h-0 list-none overflow-y-auto p-0">
              <For each={appNotifications()}>
                {(notification) => <NotificationRow notification={notification} />}
              </For>
            </ul>
          </Show>
        </div>
      </Show>
    </div>
  );
}

function NotificationRow(props: { notification: AppNotification }) {
  const icon = (): IconName => {
    if (props.notification.tone === "success") return "check";
    if (props.notification.tone === "error" || props.notification.tone === "warning") return "warn";
    if (props.notification.tone === "progress") return "clock";
    return "info";
  };
  const title = () =>
    props.notification.title ||
    t(
      props.notification.tone === "error"
        ? S.app.notifications.error
        : S.app.notifications.info,
    );
  const progress = () => props.notification.progress;
  const determinate = () => (progress()?.total ?? 0) > 0;
  const ratio = () => {
    const value = progress();
    return value === undefined || value.total <= 0
      ? 0.35
      : Math.min(1, Math.max(0, value.done / value.total));
  };
  const time = () =>
    new Intl.DateTimeFormat(locale() === "vi" ? "vi-VN" : "en-US", {
      hour: "2-digit",
      minute: "2-digit",
    }).format(props.notification.updatedAt);

  return (
    <li class="flex gap-sm border-b border-line px-md py-sm last:border-b-0">
      <span
        class="mt-3xs grid size-7 shrink-0 place-items-center rounded-panel"
        classList={{
          "bg-accent-soft text-accent-ink": props.notification.tone === "progress",
          "bg-success-soft text-success": props.notification.tone === "success",
          "bg-warn-soft text-warn": props.notification.tone === "warning",
          "bg-danger-soft text-danger": props.notification.tone === "error",
          "bg-surface-soft text-muted": props.notification.tone === "info",
        }}
      >
        <Icon
          name={icon()}
          size={14}
          class={
            props.notification.tone === "progress"
              ? "motion-safe:animate-pulse motion-reduce:animate-none"
              : undefined
          }
        />
      </span>

      <div class="flex min-w-0 flex-1 flex-col gap-2xs">
        <div class="flex min-w-0 items-baseline justify-between gap-sm">
          <p class="m-0 min-w-0 truncate text-xs font-medium text-ink" title={title()}>
            {title()}
          </p>
          <time class="shrink-0 text-2xs text-faint tabular-nums">
            {time()}
          </time>
        </div>
        <p class="m-0 text-xs leading-relaxed break-words text-text">
          {props.notification.message}
        </p>
        <Show when={props.notification.detail}>
          {(detail) => (
            <p class="m-0 truncate text-2xs text-muted" title={detail()}>
              {detail()}
            </p>
          )}
        </Show>
        <Show when={progress()}>
          {(value) => (
            <div class="mt-3xs flex flex-col gap-3xs">
              <div
                role="progressbar"
                aria-label={value().label}
                aria-valuemin={determinate() ? 0 : undefined}
                aria-valuemax={determinate() ? value().total : undefined}
                aria-valuenow={determinate() ? value().done : undefined}
                aria-valuetext={value().label}
                class="h-1 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
              >
                <div
                  class="h-full w-full origin-left rounded-pill bg-accent transition-transform duration-[var(--dur-base)] motion-reduce:transition-none"
                  classList={{ "motion-safe:animate-pulse": !determinate() }}
                  style={{ transform: `scaleX(${ratio()})` }}
                />
              </div>
              <Show when={determinate()}>
                <span class="text-right text-2xs text-muted tabular-nums">
                  {value().done}/{value().total}
                </span>
              </Show>
            </div>
          )}
        </Show>
      </div>

      <Show when={props.notification.dismissible}>
        <button
          type="button"
          aria-label={t(S.app.notifications.dismiss)}
          onClick={() => dismissAppNotification(props.notification.id)}
          class="grid size-7 shrink-0 place-items-center rounded-icon text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
        >
          <Icon name="x" size={13} />
        </button>
      </Show>
    </li>
  );
}
