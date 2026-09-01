import { createMemo, createResource, createSignal, Match, Switch } from "solid-js";
import { diagramKind, isDark, renderDiagram } from "../../lib/mermaid";
import { CopyButton } from "../primitives";

type Mode = "figure" | "source";

/**
 * Thẻ chứa một sơ đồ mermaid.
 *
 * Luôn có hai lối vào cùng một nội dung: **hình** và **mã nguồn**. Không phải để chiều
 * người thích đọc mã — SVG mermaid sinh ra là một đống `<path>` và `<text>` rời rạc,
 * trình đọc màn hình đi qua nó chỉ nghe được vài mẩu nhãn không thứ tự. `role="img"` cắt
 * đống đó xuống còn một nhãn duy nhất, và lối xem mã nguồn là cách trung thực nhất ta có
 * để một người không nhìn được hình vẫn đọc được sơ đồ.
 */
export default function Diagram(props: { source: string }) {
  const [mode, setMode] = createSignal<Mode>("figure");
  const kind = createMemo(() => diagramKind(props.source));

  // Đọc `isDark()` ngay trong nguồn của resource: mermaid nướng màu thẳng vào SVG, nên
  // đổi sáng/tối là phải vẽ lại, không phải đổi một lớp CSS bên ngoài.
  const [result] = createResource(
    () => ({ source: props.source, dark: isDark() }),
    (input) => renderDiagram(input.source),
  );

  const failure = createMemo(() => {
    const value = result();
    return value !== undefined && !value.ok ? value.message : null;
  });
  const svg = createMemo(() => {
    const value = result();
    return value !== undefined && value.ok ? value.svg : null;
  });

  return (
    <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
      <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
        <figcaption class="min-w-0 truncate text-2xs text-muted">Sơ đồ · {kind()}</figcaption>
        <div class="flex items-center gap-3xs">
          <div role="group" aria-label="Cách xem sơ đồ" class="flex items-center gap-3xs">
            <ModeButton
              label="Hình"
              active={mode() === "figure"}
              onPick={() => setMode("figure")}
            />
            <ModeButton
              label="Mã nguồn"
              active={mode() === "source"}
              onPick={() => setMode("source")}
            />
          </div>
          <CopyButton text={() => props.source} label="Chép mã nguồn sơ đồ" />
        </div>
      </div>

      <Switch>
        {/* Cú pháp hỏng thì hiện **cả** thông điệp lẫn mã nguồn, bất kể đang ở lối xem
            nào: một ô đỏ trống không nói được mô hình sai ở dòng nào, và người dùng cần
            đúng thông tin đó để bảo mô hình sửa. */}
        <Match when={failure()}>
          {(message) => (
            <div class="flex flex-col gap-2xs px-sm py-2xs">
              <p class="m-0 flex items-start gap-2xs text-2xs text-danger">
                <span class="shrink-0">Không vẽ được:</span>
                <span class="min-w-0 whitespace-pre-wrap">{message()}</span>
              </p>
              <Source code={props.source} />
            </div>
          )}
        </Match>

        <Match when={mode() === "source"}>
          <div class="px-sm py-2xs">
            <Source code={props.source} />
          </div>
        </Match>

        <Match when={result.loading}>
          <p class="m-0 px-sm py-md text-2xs text-faint" aria-busy="true">
            Đang vẽ sơ đồ…
          </p>
        </Match>

        <Match when={svg()}>
          {(markup) => (
            <div
              role="img"
              aria-label={`${kind()} do trợ lý vẽ — chuyển sang lối xem "Mã nguồn" để đọc bằng chữ`}
              /* Cuộn ngang nằm trong khung này, và SVG co xuống theo bề rộng khung
                 (`useMaxWidth`): một sơ đồ hai chục đỉnh không được đẩy cả bản ghi rộng ra. */
              class="overflow-x-auto px-sm py-sm [&_svg]:h-auto [&_svg]:max-w-full"
              innerHTML={markup()}
            />
          )}
        </Match>
      </Switch>
    </figure>
  );
}

function ModeButton(props: { label: string; active: boolean; onPick: () => void }) {
  return (
    <button
      type="button"
      onClick={props.onPick}
      aria-pressed={props.active}
      class="rounded-btn px-2xs py-3xs text-2xs transition-colors duration-[var(--dur-fast)]"
      classList={{
        "text-muted hover:bg-[var(--overlay-hover)] hover:text-ink": !props.active,
        "bg-accent-soft text-accent-ink": props.active,
      }}
    >
      {props.label}
    </button>
  );
}

function Source(props: { code: string }) {
  return (
    <div class="overflow-x-auto rounded-panel bg-surface-soft">
      <pre class="m-0 w-max min-w-full px-sm py-2xs font-mono text-2xs leading-[1.55] text-text">
        {props.code}
      </pre>
    </div>
  );
}
