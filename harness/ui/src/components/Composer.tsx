import { createSignal, Show } from "solid-js";
import { useDragDrop } from "../hooks/useDragDrop";
import { displayMode } from "../lib/prefs";
import type { ModelChoice } from "../lib/protocol";
import Icon from "./Icon";
import Menu from "./Menu";
import ModelPicker from "./ModelPicker";
import { IconButton } from "./primitives";

/** Phạm vi tool cho lượt kế. Chỉ là ý định của người dùng — lõi vẫn canh lại lúc gọi. */
export type ToolScope = "read" | "write" | "shell";

const SCOPE_LABEL: Record<ToolScope, string> = {
  read: "Chỉ đọc",
  write: "Đọc và ghi",
  shell: "Đọc, ghi và chạy lệnh",
};

/**
 * Ô soạn tin: một khối bo tròn nằm giữa dưới, và **mọi** nút nằm trong viền của nó.
 *
 * Được điều khiển từ ngoài (`value`/`onChange`) vì kéo thả tệp phải chèn được đường dẫn
 * vào bản nháp, và bản nháp phải sống sót qua việc đổi phiên.
 *
 * Hàng công cụ nằm *trong* khung viền chứ không ở trên nó: mô hình và phạm vi tool là
 * thuộc tính của tin nhắn sắp gửi, không phải của cả màn hình, và đặt chúng ngoài khung
 * là mời người dùng quên mất chúng tồn tại. Cùng lý do khiến ChatGPT dời bộ chọn mô hình
 * từ thanh trên xuống đây.
 *
 * Phạm vi tool thì **không** giấu sau menu "+" như ChatGPT giấu tool của nó: chọn "chạy
 * lệnh" là cấp cho mô hình quyền chạy lệnh trên máy này, và một quyền đang mở phải đọc
 * được mà không cần bấm vào đâu cả.
 */
export default function Composer(props: {
  value: string;
  onChange: (text: string) => void;
  onSubmit: () => void;
  disabled: boolean;
  busy: boolean;
  onStop: () => void;
  model: string;
  models: ModelChoice[];
  onPickModel: (model: string) => void;
  /** Mở cài đặt → nhà cung cấp mô hình, từ trong bộ chọn mô hình. */
  onManageProviders: () => void;
  /** Câu cảnh báo dưới ô chọn mô hình. `undefined` khi không có gì phải nói. */
  modelWarning?: string;
  scope: ToolScope;
  onPickScope: (scope: ToolScope) => void;
}) {
  let composing = false;
  let field: HTMLTextAreaElement | undefined;
  const [focused, setFocused] = createSignal(false);
  const [hint, setHint] = createSignal(false);

  // Kéo thả lấy đường dẫn tuyệt đối — thứ HTML5 drag & drop cố ý không cho. Chèn vào
  // cuối bản nháp thay vì thay thế: người dùng thường đã gõ dở câu hỏi rồi mới thả tệp.
  useDragDrop((paths) => {
    const prefix = props.value.trim() === "" ? "" : `${props.value.replace(/\s*$/, "")}\n`;
    props.onChange(`${prefix}${paths.join("\n")}\n`);
    field?.focus();
  });

  const submit = () => {
    if (props.disabled || props.value.trim() === "") return;
    props.onSubmit();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    // Bộ gõ tiếng Việt gửi Enter để chốt từ đang gõ. Không có guard này thì mỗi lần
    // chốt dấu là một lần gửi nhầm — chat_view.py:453 đã vấp đúng chỗ đó.
    if (composing || event.isComposing) return;
    const chord = event.metaKey || event.ctrlKey;
    if (event.key === "Enter" && (chord || !event.shiftKey)) {
      event.preventDefault();
      submit();
    }
  };

  // Ô nhập cao theo nội dung, tối đa ~10 dòng. Đo bằng `scrollHeight` sau khi ép về 0:
  // không ép thì ô đã cao rồi sẽ không bao giờ thấp xuống lại khi người dùng xoá bớt.
  const resize = (el: HTMLTextAreaElement) => {
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  };

  return (
    <form
      class="shrink-0 bg-bg px-(--page-pad-x) pt-sm pb-(--page-pad-y)"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <div
        // Ô soạn tin rộng đúng bằng cột chữ phía trên nó: lệch một chút thôi là mắt đọc
        // ra hai khối không thuộc về nhau.
        class="mx-auto flex w-full flex-col rounded-composer border bg-surface shadow-float transition-colors duration-[var(--dur-base)]"
        classList={{
          "border-accent": focused(),
          "border-line-strong": !focused(),
          "max-w-(--reading-measure)": displayMode() === "bubble",
          "max-w-[min(100%,980px)]": displayMode() === "document",
        }}
      >
        <textarea
          ref={(el) => {
            field = el;
            queueMicrotask(() => resize(el));
          }}
          rows={1}
          value={props.value}
          disabled={props.disabled}
          placeholder="Nhắn cho trợ lý…  (Enter để gửi, Shift+Enter xuống dòng)"
          aria-label="Nội dung tin nhắn"
          aria-keyshortcuts="Enter Meta+Enter Control+Enter"
          onCompositionStart={() => (composing = true)}
          onCompositionEnd={() => (composing = false)}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onInput={(event) => {
            props.onChange(event.currentTarget.value);
            resize(event.currentTarget);
          }}
          onKeyDown={onKeyDown}
          class="max-h-[220px] w-full resize-none bg-transparent px-md pt-md pb-2xs text-base text-text outline-none placeholder:text-faint"
        />

        {/* Đính kèm đi qua kéo thả chứ không qua hộp thoại chọn tệp: chỉ có tầng hệ điều
            hành mới đưa được **đường dẫn tuyệt đối**, mà đường dẫn mới là thứ trợ lý cần.
            Nút này nói ra điều đó thay vì mở một hộp thoại trả về thứ vô dụng. */}
        <Show when={hint()}>
          <p class="flex items-center gap-2xs px-md pb-2xs text-2xs text-muted" role="status">
            <Icon name="paperclip" size={12} />
            Kéo tệp thả vào cửa sổ để chèn đường dẫn tuyệt đối vào tin nhắn.
          </p>
        </Show>

        <Show when={props.modelWarning}>
          {(message) => (
            // `role="status"` chứ không `alert`: đây là một điều kiện đang tồn tại, không
            // phải một sự kiện vừa xảy ra — trình đọc màn hình nên đọc nó khi tới lượt.
            <p class="flex items-center gap-2xs px-md pb-2xs text-2xs text-warn" role="status">
              <Icon name="warn" size={12} />
              {message()}
            </p>
          )}
        </Show>

        <div class="flex flex-wrap items-center gap-2xs px-2xs pb-2xs">
          <IconButton
            icon="paperclip"
            label="Cách đính kèm tệp"
            active={hint()}
            onClick={() => setHint((v) => !v)}
          />

          <ModelPicker
            value={props.model}
            models={props.models}
            onPick={props.onPickModel}
            onManageProviders={props.onManageProviders}
          />

          <Menu
            variant="pill"
            placement="up"
            align="left"
            icon="tools"
            text={SCOPE_LABEL[props.scope]}
            tone={props.scope === "shell" ? "warn" : "neutral"}
            label={`Phạm vi tool: ${SCOPE_LABEL[props.scope]}`}
            items={(["read", "write", "shell"] as ToolScope[]).map((scope) => ({
              id: scope,
              label: SCOPE_LABEL[scope],
              icon: "tools" as const,
              onSelect: () => props.onPickScope(scope),
            }))}
          />

          <span class="flex-1" />

          <span class="hidden items-center gap-3xs pr-2xs text-2xs text-faint sm:flex" aria-hidden="true">
            <Icon name="enter" size={12} />
            gửi
          </span>

          <Show
            when={props.busy}
            fallback={
              <button
                type="submit"
                disabled={props.disabled || props.value.trim() === ""}
                class="flex h-(--control-h) items-center gap-2xs rounded-pill bg-accent px-md text-sm font-medium text-on-accent transition-colors duration-[var(--dur-fast)] hover:bg-accent-hover disabled:opacity-40"
              >
                <Icon name="send" size={14} />
                Gửi
              </button>
            }
          >
            <button
              type="button"
              onClick={props.onStop}
              class="flex h-(--control-h) items-center gap-2xs rounded-pill border border-line-strong px-md text-sm font-medium text-text transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
            >
              <Icon name="stop" size={14} />
              Dừng
            </button>
          </Show>
        </div>
      </div>
    </form>
  );
}
