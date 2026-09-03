import { Key } from "@solid-primitives/keyed";
import { createSignal, For, Show } from "solid-js";
import type { McpCatalogEntry, McpServerInput } from "../../lib/protocol";
import Icon from "./../Icon";
import { Banner, Button, DialogShell, ExternalLink } from "../settings/FormKit";

/** Tên người đọc được của mấy thứ phải có sẵn trên máy. */
const REQUIRES: Record<string, string> = {
  node: "Node.js (lệnh npx)",
  python: "Python (lệnh uvx hoặc pipx)",
  docker: "Docker đang chạy",
};

/**
 * Danh mục server MCP dựng sẵn — cắm bằng một cú bấm.
 *
 * Hai điều màn hình này phải nói **trước** khi người dùng bấm, chứ không phải sau:
 *
 *   1. `requires` — server chạy bằng `npx` trên một máy không có Node sẽ hỏng, và nó hỏng
 *      *sau* hai mươi giây, dưới dạng một dòng `spawn ENOENT` mà không ai đọc được. Một
 *      câu cảnh báo trước rẻ hơn hai mươi giây đó rất nhiều.
 *   2. Biến bắt buộc còn thiếu — nút cắm bị chặn, và **nói thiếu cái nào**. "Điền đủ
 *      thông tin" là một câu không giúp ai điền thêm được gì.
 *
 * Giá trị bí mật che khi gõ và **không bao giờ hiện lại**: sau khi cắm, hộp thoại đóng và
 * state biến mất cùng nó. Không có đường nào đọc ngược một token đã lưu ra màn hình.
 */
export default function McpCatalog(props: {
  entries: McpCatalogEntry[];
  busy: boolean;
  error: string | null;
  onInstall: (input: McpServerInput) => void;
  onManual: () => void;
  onClose: () => void;
}) {
  const [picked, setPicked] = createSignal<McpCatalogEntry | null>(null);
  const [values, setValues] = createSignal<Record<string, string>>({});

  const missing = () => {
    const entry = picked();
    if (entry === null) return [];
    return entry.env
      .filter((variable) => variable.required && (values()[variable.key] ?? "").trim() === "")
      .map((variable) => variable.label);
  };

  const install = () => {
    const entry = picked();
    if (entry === null || missing().length > 0 || props.busy) return;
    const env: Record<string, string> = {};
    for (const variable of entry.env) {
      const value = (values()[variable.key] ?? "").trim();
      if (value !== "") env[variable.key] = value;
    }
    props.onInstall({
      name: entry.id,
      transport: "stdio",
      command: entry.command,
      args: [...entry.args],
      env,
      cwd: null,
      url: "",
      headers: {},
      enabled: true,
    });
  };

  return (
    <DialogShell
      icon="plug"
      title={picked() === null ? "Danh mục server MCP" : `Cắm ${picked()?.name}`}
      desc={
        picked() === null
          ? "Mỗi mục thêm một bộ tool có tiền tố ext.<server>."
          : "Điền các biến server cần, rồi cắm."
      }
      more={
        picked()?.env.some((variable) => variable.secret) === true
          ? "Giá trị bí mật đi thẳng vào lõi và không hiện lại. Sau khi cắm, hộp thoại đóng và không có đường nào đọc ngược ra màn hình."
          : undefined
      }
      wide
      onClose={props.onClose}
      onSubmit={install}
      footer={() => (
        <>
          <Show
            when={picked() !== null}
            fallback={
              <Button label="Tự khai báo…" variant="outline" icon="plus" onClick={props.onManual} />
            }
          >
            <Button
              label="Quay lại danh mục"
              variant="ghost"
              onClick={() => {
                setPicked(null);
                setValues({});
              }}
            />
          </Show>
          <span class="flex-1" />
          <Button label="Đóng" variant="ghost" onClick={props.onClose} />
          <Show when={picked()}>
            <Button
              label="Cắm server"
              type="submit"
              busy={props.busy}
              disabled={missing().length > 0}
            />
          </Show>
        </>
      )}
    >
      <Show
        when={picked()}
        fallback={
          <Show
            when={props.entries.length > 0}
            fallback={
              <p class="m-0 text-xs text-faint">
                Chưa có mục dựng sẵn nào; tự khai báo bên dưới.
              </p>
            }
          >
            <ul class="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-sm p-0">
              <For each={props.entries}>
                {(entry) => (
                  <li class="min-w-0">
                    <div class="flex h-full flex-col gap-2xs rounded-card border border-line bg-surface p-sm transition-colors duration-[var(--dur-fast)] hover:border-accent">
                      <button
                        type="button"
                        onClick={() => {
                          setPicked(entry);
                          setValues({});
                        }}
                        class="flex min-w-0 flex-1 flex-col items-start gap-2xs text-left"
                      >
                        <span class="flex w-full min-w-0 items-center gap-2xs">
                          <span class="grid size-6 shrink-0 place-items-center rounded-icon bg-accent-soft text-accent-ink">
                            <Icon name="plug" size={13} />
                          </span>
                          <span class="min-w-0 flex-1 truncate text-xs font-semibold text-ink">
                            {entry.name}
                          </span>
                        </span>
                        <span class="text-2xs text-muted">{entry.summary}</span>
                        {/* Mục chạy từ xa: nói ra rằng **không phải cài gì**. Đây là câu
                            trả lời cho đúng nỗi ngần ngại giữ người dùng lại ở màn hình
                            này — cắm một server nghĩa là cài thêm bao nhiêu thứ nữa. */}
                        <Show when={entry.url !== null}>
                          <span class="inline-flex items-center gap-3xs rounded-pill bg-accent-soft px-2xs py-3xs text-2xs text-accent-ink">
                            <Icon name="cloud" size={10} />
                            Chạy từ xa — không cần cài gì
                          </span>
                        </Show>
                        <Show when={entry.requires.length > 0}>
                          <span class="flex flex-wrap gap-3xs">
                            <For each={entry.requires}>
                              {(need) => (
                                <span class="inline-flex items-center gap-3xs rounded-pill bg-warn-soft px-2xs py-3xs text-2xs text-warn">
                                  <Icon name="warn" size={10} />
                                  {REQUIRES[need] ?? need}
                                </span>
                              )}
                            </For>
                          </span>
                        </Show>
                        <Show when={entry.env.some((variable) => variable.required)}>
                          <span class="inline-flex items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
                            <Icon name="key" size={10} />
                            Cần điền {entry.env.filter((variable) => variable.required).length} biến
                          </span>
                        </Show>
                      </button>
                      <div class="flex items-center justify-between gap-sm border-t border-line pt-2xs">
                        <span class="min-w-0 truncate font-mono text-2xs text-faint">
                          {[entry.command, ...entry.args].join(" ")}
                        </span>
                        <ExternalLink href={entry.homepage}>Tài liệu</ExternalLink>
                      </div>
                    </div>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        }
      >
        {(entry) => (
          <>
            <p class="m-0 text-xs text-muted">{entry().summary}</p>

            <Show when={entry().requires.length > 0}>
              <Banner
                tone="warn"
                icon="warn"
                title="Máy này phải có sẵn"
                more="Thiếu một trong số này thì server sẽ cắm hỏng, và thông điệp lỗi là một dòng của hệ điều hành chứ không phải một câu tiếng Việt."
              >
                <ul class="m-0 list-disc pl-lg">
                  <For each={entry().requires}>
                    {(need) => <li>{REQUIRES[need] ?? need}</li>}
                  </For>
                </ul>
                Thiếu một thứ thì server cắm hỏng.
              </Banner>
            </Show>

            <p class="m-0 overflow-x-auto rounded-panel border border-line bg-surface-soft px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
              {[entry().command, ...entry().args].join(" ")}
            </p>

            <Show
              when={entry().env.length > 0}
              fallback={
                <p class="m-0 text-2xs text-faint">Server này không cần biến môi trường nào.</p>
              }
            >
              {/* `<Key>` theo tên biến: danh sách này không đổi thứ tự, nhưng `<For>` khớp
                  theo vị trí sẽ dựng lại ô nhập nếu lõi trả về một danh mục khác giữa
                  chừng — và người đang gõ token mất tiêu điểm giữa dòng. */}
              <Key each={entry().env} by="key">
                {(variable) => (
                  <div class="flex min-w-0 flex-col gap-2xs">
                    <label class="flex items-center gap-2xs text-2xs text-faint">
                      {variable().label}
                      <span class="font-mono">{variable().key}</span>
                      <Show
                        when={variable().required}
                        fallback={<span class="text-faint">— tuỳ chọn</span>}
                      >
                        <span class="text-warn">— bắt buộc</span>
                      </Show>
                      <Show when={variable().secret}>
                        <span class="inline-flex items-center gap-3xs text-muted">
                          <Icon name="key" size={10} />
                          không hiện lại sau khi cắm
                        </span>
                      </Show>
                    </label>
                    <input
                      type={variable().secret ? "password" : "text"}
                      value={values()[variable().key] ?? ""}
                      spellcheck={false}
                      autocapitalize="off"
                      autocomplete="off"
                      aria-label={variable().label}
                      aria-required={variable().required}
                      onInput={(event) => {
                        const next = event.currentTarget.value;
                        setValues((current) => ({ ...current, [variable().key]: next }));
                      }}
                      class="h-(--control-h) w-full rounded-btn border border-line bg-bg px-sm font-mono text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] focus:border-accent"
                    />
                  </div>
                )}
              </Key>
            </Show>

            {/* Chặn nút *và* nói thiếu cái gì. Chặn mà không nói là để người dùng nhìn một
                cái nút xám mà không biết phải làm gì để nó sáng lên. */}
            <Show when={missing().length > 0}>
              <Banner tone="info" icon="warn" role="status">
                Còn thiếu: <b>{missing().join(", ")}</b> — điền xong thì nút sáng lên.
              </Banner>
            </Show>

            <Show when={props.error}>
              {(message) => (
                <Banner tone="danger" icon="warn" role="alert" title="Không cắm được">
                  {message()}
                </Banner>
              )}
            </Show>
          </>
        )}
      </Show>
    </DialogShell>
  );
}
