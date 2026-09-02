import { createEffect, createMemo, createResource, createSignal, Show } from "solid-js";
import { useDragDrop } from "../hooks/useDragDrop";
import { applyCompletion, completePaths, findTrigger, rankCommands } from "../lib/complete";
import CompletionPopup, { type Suggestion } from "./CompletionPopup";
import { displayMode } from "../lib/prefs";
import type { ModelChoice, ProjectKind, ToolScope } from "../lib/protocol";
import Icon from "./Icon";
import Menu from "./Menu";
import ModelPicker from "./ModelPicker";
import { Chip, IconButton } from "./primitives";

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
 * được mà không cần bấm vào đâu cả. Cùng luật đó bắt nó phải *tắt đi trông thấy* khi chưa
 * có dự án — xem `hasProject`.
 *
 * Mức đang hiện là **mức sẽ đi kèm lượt kế**, không phải một thiết lập được lưu ở đâu đó:
 * mỗi lần gửi, giá trị này đi thẳng vào `send_message` và lõi siết sổ đăng ký tool theo
 * nó cho đúng lượt ấy. Nhãn ở đây vì vậy đọc được như một lời hứa kiểm chứng được, chứ
 * không phải một cái công tắc trang trí.
 */
export default function Composer(props: {
  value: string;
  onChange: (text: string) => void;
  onSubmit: () => void;
  /**
   * Khoá cứng ô nhập. Chỉ dùng cho việc đổi dự án — lúc đó mọi thứ trên màn hình còn nói
   * về dự án cũ. **Không** dùng cho "trợ lý đang trả lời": gõ tiếp trong lúc chờ là việc
   * bình thường, và câu vừa gõ đi vào `queued`.
   */
  disabled: boolean;
  busy: boolean;
  /** Câu đang chờ lượt hiện tại kết thúc. Chuỗi rỗng nghĩa là không có gì chờ. */
  queued?: string;
  /** Bỏ câu đang chờ. */
  onUnqueue?: () => void;
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
  /**
   * Có dự án đang mở hay không.
   *
   * Không có thì lõi **không cắm tool nào** của tầng dự án, nên cả ba mức phạm vi đều
   * không có gì đằng sau. Bộ chọn phải nói ra điều đó thay vì đứng yên trông như đang bật:
   * một quyền trông như đang mở mà thực ra rỗng là kiểu nói dối tệ nhất một giao diện
   * quyền hạn làm được — người dùng dựa vào nó để quyết định có gửi câu tiếp theo không.
   */
  hasProject: boolean;
  /** Tên dự án đang mở, cho dải ngữ cảnh dưới ô soạn tin. */
  projectName?: string;
  projectKind?: ProjectKind;
  /** Số server MCP **đang nối** — không phải số server đã khai báo. */
  mcpConnected: number;
  /** Còn thứ khác nằm dưới ô soạn tin (mấy câu gợi ý), nên đáy thu lại một nấc. */
  moreBelow?: boolean;
  /** Chạy một lệnh `/`. Không truyền thì bộ lệnh không mở. */
  onCommand?: (name: string) => void;
  /**
   * Ngữ cảnh đã dùng ở bước gần nhất. `null` khi lượt nào cũng chưa chạy trong phiên này.
   *
   * `window` là `null` khi không hỏi được cửa sổ của mô hình — khi ấy chip chỉ hiện con số
   * token, không hiện phần trăm: một tỉ lệ không có mẫu số là một con số bịa.
   */
  usage?: { used: number; window: number | null } | null;
}) {
  let composing = false;
  let field: HTMLTextAreaElement | undefined;
  const [focused, setFocused] = createSignal(false);
  const [hint, setHint] = createSignal(false);

  // ---- hoàn thành `@` và `/` ------------------------------------------------
  //
  // Con trỏ giữ ở đây chứ không đọc thẳng từ `field.selectionStart` mỗi lần vẽ: đọc thẳng
  // là đọc DOM trong lúc dựng, và Solid không vẽ lại khi con trỏ dịch mà chữ không đổi.
  const [caret, setCaret] = createSignal(0);
  const [dismissed, setDismissed] = createSignal(false);
  const [cursor, setCursor] = createSignal(0);

  const trigger = createMemo(() => {
    if (dismissed()) return null;
    const found = findTrigger(props.value, caret());
    if (found?.kind === "command" && props.onCommand === undefined) return null;
    return found;
  });

  // Chỉ hỏi lõi khi đang thật sự gõ một đường dẫn. `createResource` gộp các lần gõ liên
  // tiếp: lần gọi cũ bị bỏ khi truy vấn đổi, nên gõ nhanh không xếp thành một hàng đợi.
  const [paths] = createResource(
    () => (trigger()?.kind === "path" ? trigger()!.query : null),
    (query) => completePaths(query, 8),
  );

  const items = createMemo<Suggestion[]>(() => {
    const found = trigger();
    if (!found) return [];
    if (found.kind === "command") {
      return rankCommands(found.query).map((command) => ({
        value: command.name,
        label: `/${command.name}`,
        hint:
          command.needsProject === true && !props.hasProject ? "cần một dự án" : command.hint,
        disabled: command.needsProject === true && !props.hasProject,
      }));
    }
    return (paths() ?? []).map((path) => ({ value: path }));
  });

  // Truy vấn đổi thì con trỏ về đầu: giữ nguyên chỉ số cũ là để nó trỏ vào một hàng khác
  // hẳn sau khi danh sách đã thay, và Enter chèn thứ người dùng không nhìn.
  createEffect(() => {
    items();
    setCursor(0);
  });

  const open = () => trigger() !== null && items().length > 0;

  const moveCursor = (delta: number) => {
    const count = items().length;
    if (count === 0) return;
    setCursor((current) => (current + delta + count) % count);
  };

  /** Ghi lại chỗ con trỏ sau khi trình duyệt đã dịch nó. */
  const syncCaret = (el: HTMLTextAreaElement) => setCaret(el.selectionStart ?? 0);

  const choose = (item: Suggestion) => {
    const found = trigger();
    if (!found || item.disabled === true) return;
    if (found.kind === "command") {
      props.onChange("");
      setDismissed(true);
      props.onCommand?.(item.value);
      field?.focus();
      return;
    }
    const next = applyCompletion(props.value, found, item.value);
    props.onChange(next.text);
    // Đặt lại con trỏ **sau** khi Solid ghi giá trị mới xuống DOM, nếu không trình duyệt
    // đẩy nó về cuối chuỗi và người dùng mất chỗ đang gõ giữa câu.
    queueMicrotask(() => {
      if (!field) return;
      field.setSelectionRange(next.caret, next.caret);
      setCaret(next.caret);
      field.focus();
    });
  };

  const optionId = (index: number) => `composer-opt-${index}`;

  /** Tỉ lệ lấp đầy ngữ cảnh, hoặc `null` khi chưa đáng nói ra. */
  const contextPressure = createMemo(() => {
    const counted = props.usage;
    if (!counted || counted.window === null || counted.window <= 0) return null;
    const ratio = counted.used / counted.window;
    return ratio >= 0.6 ? { ratio: Math.min(ratio, 1) } : null;
  });

  // Kéo thả lấy đường dẫn tuyệt đối — thứ HTML5 drag & drop cố ý không cho. Chèn vào
  // cuối bản nháp thay vì thay thế: người dùng thường đã gõ dở câu hỏi rồi mới thả tệp.
  useDragDrop((paths) => {
    const prefix = props.value.trim() === "" ? "" : `${props.value.replace(/\s*$/, "")}\n`;
    props.onChange(`${prefix}${paths.join("\n")}\n`);
    field?.focus();
  });

  // Không chặn khi `busy`: App nhận câu này và xếp nó vào ô chờ. Chặn ở đây thì Enter
  // giữa lượt lại không làm gì cả, đúng cái im lặng vừa bỏ đi.
  const submit = () => {
    if (props.disabled || props.value.trim() === "") return;
    props.onSubmit();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    // Bộ gõ tiếng Việt gửi Enter để chốt từ đang gõ. Không có guard này thì mỗi lần
    // chốt dấu là một lần gửi nhầm — chat_view.py:453 đã vấp đúng chỗ đó.
    if (composing || event.isComposing) return;

    // Danh sách gợi ý đang mở thì nó **giành trước** các phím điều hướng. Enter ở đây chèn
    // một gợi ý chứ không gửi tin: người vừa gõ `@sto` và thấy một danh sách đang chờ thì
    // Enter của họ nói về danh sách ấy, không nói về cả tin nhắn.
    if (open()) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        moveCursor(1);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        moveCursor(-1);
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        const item = items()[cursor()];
        if (item && item.disabled !== true) {
          event.preventDefault();
          choose(item);
          return;
        }
      }
      if (event.key === "Escape") {
        event.preventDefault();
        // Đóng danh sách, **giữ nguyên chữ**. Xoá luôn phần đã gõ là phạt người dùng vì đã
        // gõ một dấu `@` — và Esc ở mọi chỗ khác trong ứng dụng cũng chỉ đóng, không xoá.
        setDismissed(true);
        return;
      }
    }

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
      class="shrink-0 bg-bg px-(--page-pad-x) pt-sm"
      classList={{
        "pb-(--page-pad-y)": props.moreBelow !== true,
        "pb-sm": props.moreBelow === true,
      }}
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      {/* Câu đang chờ, hiện **trên** ô soạn tin và rộng đúng bằng nó.

          Xếp hàng mà không hiện ra thì Enter đọc như một cú bấm rơi vào hư không, và người
          dùng gõ lại câu đó lần nữa. Kèm nút bỏ vì đổi ý giữa lúc chờ là chuyện thường —
          câu trả lời đang chảy ngay trên kia có thể vừa trả lời xong chính nó. */}
      <Show when={(props.queued ?? "") !== ""}>
        <div
          class="mx-auto mb-xs flex w-full items-center gap-xs rounded-panel border border-line bg-surface-soft px-md py-xs"
          classList={{
            "max-w-(--reading-measure)": displayMode() === "bubble",
            "max-w-[min(100%,980px)]": displayMode() === "document",
          }}
        >
          <Icon name="clock" size={13} />
          <span class="shrink-0 text-2xs text-faint">Gửi khi xong</span>
          <span class="min-w-0 flex-1 truncate text-sm text-text">{props.queued}</span>
          <button
            type="button"
            onClick={() => props.onUnqueue?.()}
            aria-label="Bỏ câu đang chờ"
            class="shrink-0 rounded-btn px-2xs py-3xs text-2xs text-muted transition-colors hover:bg-[var(--overlay-hover)] hover:text-text"
          >
            Bỏ
          </button>
        </div>
      </Show>

      <div
        // Ô soạn tin rộng đúng bằng cột chữ phía trên nó: lệch một chút thôi là mắt đọc
        // ra hai khối không thuộc về nhau.
        class="relative mx-auto flex w-full flex-col rounded-composer border bg-surface shadow-float transition-colors duration-[var(--dur-base)]"
        classList={{
          "border-accent": focused(),
          "border-line-strong": !focused(),
          "max-w-(--reading-measure)": displayMode() === "bubble",
          "max-w-[min(100%,980px)]": displayMode() === "document",
        }}
      >
        <CompletionPopup
          items={items()}
          cursor={cursor()}
          id="composer-completions"
          optionId={optionId}
          onPick={choose}
          onHover={setCursor}
          empty={
            trigger()?.kind === "path" && !paths.loading ? "Không có tệp nào khớp." : undefined
          }
        />

        <textarea
          ref={(el) => {
            field = el;
            queueMicrotask(() => resize(el));
          }}
          rows={1}
          value={props.value}
          disabled={props.disabled}
          placeholder={
            props.busy
              ? "Gõ câu tiếp theo…  (Enter để xếp hàng, gửi khi lượt này xong)"
              : "Nhắn cho trợ lý…  (Enter để gửi, Shift+Enter xuống dòng)"
          }
          aria-label="Nội dung tin nhắn"
          aria-keyshortcuts="Enter Meta+Enter Control+Enter"
          onCompositionStart={() => (composing = true)}
          onCompositionEnd={() => (composing = false)}
          role="combobox"
          aria-expanded={open()}
          aria-controls="composer-completions"
          aria-activedescendant={open() ? optionId(cursor()) : undefined}
          aria-autocomplete="list"
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onInput={(event) => {
            props.onChange(event.currentTarget.value);
            resize(event.currentTarget);
            // Gõ tiếp sau khi đã Esc là một lời gọi mới, không phải phần đuôi của lời gọi
            // vừa bị đóng — nếu không, Esc một lần là tắt hoàn thành cho tới hết câu.
            setDismissed(false);
            syncCaret(event.currentTarget);
          }}
          onKeyUp={(event) => syncCaret(event.currentTarget)}
          onClick={(event) => syncCaret(event.currentTarget)}
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

        {/* Hai câu trạng thái nằm **cùng một hàng biết xuống dòng** chứ không chồng lên nhau
            thành hai dải: ô soạn tin chỉ cao vài chục pixel, và mỗi dòng chữ thêm vào đẩy
            hàng nút xuống một nấc. Cửa sổ hẹp thì chúng tự rơi xuống dòng dưới.

            `role="status"` chứ không `alert` ở cả hai: đây là những điều kiện đang tồn tại,
            không phải sự kiện vừa xảy ra — trình đọc màn hình nên đọc chúng khi tới lượt. */}
        <Show when={!props.hasProject || props.modelWarning}>
          <div class="flex flex-wrap items-center gap-x-md gap-y-3xs px-md pb-2xs text-2xs">
            {/* Câu chốt lại bằng "vẫn gửi được": đây là một giới hạn đang tồn tại, không
                phải một thứ vừa hỏng, và người đọc phải rời câu này với niềm tin rằng ô
                soạn tin bên dưới còn dùng được. */}
            <Show when={!props.hasProject}>
              <p class="m-0 flex items-center gap-2xs text-muted" role="status">
                <Icon name="tools" size={12} />
                Chưa mở dự án — trợ lý chưa có tool nào. Tin nhắn vẫn gửi được.
              </p>
            </Show>

            <Show when={props.modelWarning}>
              {(message) => (
                <p class="m-0 flex items-center gap-2xs text-warn" role="status">
                  <Icon name="warn" size={12} />
                  {message()}
                </p>
              )}
            </Show>
          </div>
        </Show>

        {/* Tầng dưới của ô soạn tin, xếp đúng theo hình mẫu: đính kèm ở mép trái, **quyền**
            ngay cạnh nó, và mô hình dạt sang phải cạnh nút Gửi. Thứ tự này không tuỳ tiện —
            trái sang phải là "đưa gì vào → được làm gì với nó → ai làm", và cái đắt nhất
            trong ba cái đó là quyền, nên nó đứng ở chỗ mắt chạm tới trước. */}
        <div class="flex flex-wrap items-center gap-2xs px-2xs pb-2xs">
          <IconButton
            icon="paperclip"
            label="Cách đính kèm tệp"
            active={hint()}
            onClick={() => setHint((v) => !v)}
          />

          {/* Vô hiệu chứ không ẩn hẳn: chỗ ngồi của bộ chọn giữ nguyên qua hai trạng thái,
              nên người vừa đóng dự án nhìn thấy *cái gì đã đổi* thay vì thấy một nút biến
              mất. Nó ra khỏi vòng Tab luôn — không còn lựa chọn nào để đi tới, và lý do
              nằm ở dòng chữ ngay trên, chỗ trình đọc màn hình cũng đọc được.

              Nói "chưa dùng được" bằng chữ chứ **không gạch ngang** cái nhãn: chữ bị gạch
              đọc ra là một thứ vừa hỏng hoặc vừa bị bỏ đi, mà đây là một quyền đang tắt vì
              chưa có gì để cấp. "Chưa xong" và "hỏng" là hai trạng thái. */}
          <Show
            when={props.hasProject}
            fallback={
              <span
                aria-hidden="true"
                class="flex h-(--control-h) items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-sm text-2xs text-faint"
              >
                <Icon name="hand" size={13} />
                {SCOPE_LABEL[props.scope]}
                <span class="opacity-70">· chưa dùng được</span>
              </span>
            }
          >
            {/* Bàn tay thay cho cái cờ lê: cờ lê nói "có công cụ", còn hàng này nói **được
                phép làm tới đâu** — cùng một hình cho hai ý khác nhau là chỗ người ta đọc
                lướt qua rồi tưởng mình đã hiểu. */}
            <Menu
              variant="pill"
              placement="up"
              align="left"
              icon="hand"
              text={SCOPE_LABEL[props.scope]}
              tone={props.scope === "shell" ? "warn" : "neutral"}
              label={`Phạm vi tool: ${SCOPE_LABEL[props.scope]}`}
              items={(["read", "write", "shell"] as ToolScope[]).map((scope) => ({
                id: scope,
                label: SCOPE_LABEL[scope],
                icon: "hand" as const,
                onSelect: () => props.onPickScope(scope),
              }))}
            />
          </Show>

          <span class="flex-1" />

          <ModelPicker
            value={props.model}
            models={props.models}
            onPick={props.onPickModel}
            onManageProviders={props.onManageProviders}
          />

          {/* Không còn nhãn "↵ gửi" cạnh nút: placeholder của ô nhập đã nói "Enter để gửi",
              và nhắc lại nó ngay cạnh nút Gửi chỉ thêm một mẩu chữ vào một hàng đã chật. */}
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

      {/* Dải ngữ cảnh: **lượt sắp gửi sẽ chạy với những gì**.

          Nó ở ngoài viền ô soạn tin chứ không trong: mọi thứ trong viền đều bấm được và đổi
          được, còn ba mảnh này chỉ báo cáo lại trạng thái đã chọn ở nơi khác. Trộn hai loại
          vào một khung là mời người dùng bấm vào một cái nhãn.

          Ba mảnh, ba câu hỏi mà người ta thật sự hỏi trước khi bấm Gửi: nó đọc thư mục nào,
          nó có thêm tool nào ngoài tool dựng sẵn, và ai trả lời. */}
      <div
        role="group"
        aria-label="Lượt kế sẽ chạy với"
        class="mx-auto mt-2xs flex w-full flex-wrap items-center gap-2xs px-2xs"
        classList={{
          "max-w-(--reading-measure)": displayMode() === "bubble",
          "max-w-[min(100%,980px)]": displayMode() === "document",
        }}
      >
        <Chip tone={props.hasProject ? "accent" : "neutral"}>
          <Icon name={props.hasProject ? "folder-open" : "folder"} size={11} />
          <Show when={props.hasProject} fallback="Chưa có dự án">
            {props.projectName ?? "Dự án"}
            <span class="opacity-70">
              · {props.projectKind === "docs" ? "tài liệu" : "mã nguồn"}
            </span>
          </Show>
        </Chip>

        {/* Số server, không phải tên: cột này chỉ cần trả lời "có thêm tool không". Ai muốn
            biết những tool nào thì đã có trang Server MCP, và nhét bốn cái tên vào đây là
            đẩy dòng ngữ cảnh dài hơn cả câu người dùng sắp gõ. */}
        <Chip tone={props.mcpConnected > 0 ? "accent" : "neutral"}>
          <Icon name="plug" size={11} />
          {props.mcpConnected} server MCP
        </Chip>

        {/* Áp lực ngữ cảnh — **chỉ hiện khi đã đáng lo**.

            Ngưỡng 60% là có chủ ý: dưới mức đó con số không đổi được quyết định nào của
            người dùng, và một chip đứng đó suốt phiên chỉ dạy mắt bỏ qua đúng chỗ mà về
            sau nó cần nhìn. Trên mức đó thì nó trả lời một câu thật: còn bao nhiêu chỗ
            trước khi phần đầu cuộc trò chuyện bị rút gọn.

            Mẫu số là cửa sổ mà **plugin nén** dùng làm ngưỡng, không phải cửa sổ của mô
            hình — nên chip đầy đúng lúc nén sắp chạy, chứ không sau đó. */}
        <Show when={contextPressure()}>
          {(pressure) => (
            <Chip tone={pressure().ratio >= 0.85 ? "warn" : "neutral"}>
              <Icon name="model" size={11} />
              Ngữ cảnh {Math.round(pressure().ratio * 100)}%
            </Chip>
          )}
        </Show>

        {/* Cố ý **không** có chip mô hình ở đây. Tên mô hình đã nằm trong bộ chọn ngay
            phía trên, cách chưa tới ba mươi pixel; lặp lại nó chỉ làm dài thêm một dòng
            vốn tồn tại để trả lời những câu bộ chọn *không* trả lời được — chạy trong thư
            mục nào, và có tool nào từ ngoài cắm vào. */}
      </div>
    </form>
  );
}
