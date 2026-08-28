import { Toast, toaster } from "@kobalte/core/toast";
import { AlertCircle, CheckCircle2, Info, X } from "lucide-solid";

export type ToastTone = "success" | "error" | "info";

const TOAST_REGION_ID = "app-notifications";

export function notify(props: {
  tone: ToastTone;
  title: string;
  description: string;
  duration?: number;
}) {
  return toaster.show(
    (toast) => (
      <Toast
        toastId={toast.toastId}
        class={`app-toast toast-${props.tone}`}
        priority={props.tone === "error" ? "high" : "low"}
        duration={props.duration}
      >
        <span class="toast-icon" aria-hidden="true">
          {props.tone === "success"
            ? <CheckCircle2 size={20} />
            : props.tone === "error"
              ? <AlertCircle size={20} />
              : <Info size={20} />}
        </span>
        <div class="toast-copy">
          <Toast.Title class="toast-title">{props.title}</Toast.Title>
          <Toast.Description class="toast-description">{props.description}</Toast.Description>
        </div>
        <Toast.CloseButton class="toast-close" aria-label="Đóng thông báo">
          <X size={17} aria-hidden="true" />
        </Toast.CloseButton>
        <Toast.ProgressTrack class="toast-progress">
          <Toast.ProgressFill />
        </Toast.ProgressTrack>
      </Toast>
    ),
    { region: TOAST_REGION_ID },
  );
}

export function ToastViewport() {
  return (
    <Toast.Region
      regionId={TOAST_REGION_ID}
      aria-label="Thông báo ({hotkey})"
      duration={4_500}
      limit={3}
      pauseOnInteraction
      pauseOnPageIdle
      swipeDirection="right"
      topLayer
    >
      <Toast.List class="toast-list" />
    </Toast.Region>
  );
}
