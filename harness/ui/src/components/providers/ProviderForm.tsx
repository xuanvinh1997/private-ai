import { createSignal, For, Show } from "solid-js";
import { probeProvider, suggestedEmbeddingModel } from "../../lib/providers";
import type { Provider, ProviderInput, ProviderKind, ProviderProbe } from "../../lib/protocol";
import Icon from "./../Icon";
import { Banner, Button, DialogShell, PillChoice, TextField } from "./FormKit";

/**
 * Ba trạng thái của ô khoá API, và **chỉ** ba.
 *
 *   - `keep`  — provider đã có khoá, không đụng tới. Gửi `apiKey: null`.
 *   - `set`   — đang gõ một khoá mới. Gửi chính chuỗi đó.
 *   - `clear` — cố ý xoá. Gửi `apiKey: ""`.
 *
 * Hợp đồng phân biệt `null` với `""`, và cả màn hình này tồn tại để người dùng không bao
 * giờ phải biết điều đó: một trong ba từ trên luôn hiện trên màn hình, bằng tiếng Việt,
 * cạnh chính cái ô đang nói về.
 */
type KeyMode = "keep" | "set" | "clear";

/**
 * Biểu mẫu thêm/sửa một provider.
 *
 * Điểm tinh tế nhất là ô khoá API. Lõi **không bao giờ** trả khoá về giao diện — chỉ trả
 * `hasKey` — nên ô khoá không thể hiện lại giá trị cũ. Hai cách làm sai đều tự nhiên và
 * đều tai hại: hiện ô trống thì người dùng đổi tên provider rồi lưu, và tưởng khoá vẫn
 * còn (hoặc tưởng đã mất); hiện một dãy dấu sao giả thì họ xoá nó đi để gõ khoá mới, và
 * cái họ vừa xoá không phải khoá mà là một trang trí.
 *
 * Nên ô khoá của một provider đã có khoá **không phải là ô nhập**: nó là một dòng trạng
 * thái "Đã đặt" cộng hai nút — thay, hoặc xoá. Không có đường nào đi qua đây mà làm mất
 * khoá một cách im lặng.
 */
export default function ProviderForm(props: {
  /** `null` là thêm mới. Có giá trị là sửa — và chỉ khi đó ô khoá mới có ba trạng thái. */
  provider: Provider | null;
  /** Giá trị điền sẵn khi đi từ một mục dựng sẵn. */
  preset?: { name: string; kind: ProviderKind; baseUrl: string; model: string | null } | null;
  busy: boolean;
  error: string | null;
  onSubmit: (input: ProviderInput) => void;
  onClose: () => void;
}) {
  const start = props.provider ?? props.preset ?? null;

  const [name, setName] = createSignal(start?.name ?? "");
  const [kind, setKind] = createSignal<ProviderKind>(start?.kind ?? "ollama");
  const [baseUrl, setBaseUrl] = createSignal(start?.baseUrl ?? "");
  const [model] = createSignal(start?.model ?? null);
  const [enabled, setEnabled] = createSignal(props.provider?.enabled ?? true);
  // Đọc từ `props.provider` chứ không từ `start`: một mục dựng sẵn không mang mô hình
  // nhúng, và ghép hai nguồn vào một biểu thức chỉ để tiết kiệm một dòng thì kiểu của
  // trường này phụ thuộc vào nhánh nào đang chạy.
  const [embeddingModel, setEmbeddingModel] = createSignal(props.provider?.embeddingModel ?? "");

  const hadKey = props.provider?.hasKey === true;
  const [keyMode, setKeyMode] = createSignal<KeyMode>(hadKey ? "keep" : "set");
  const [keyText, setKeyText] = createSignal("");

  const [probing, setProbing] = createSignal(false);
  const [probe, setProbe] = createSignal<ProviderProbe | null>(null);
  const [probeError, setProbeError] = createSignal<string | null>(null);

  /** `null` = giữ nguyên khoá đã lưu, `""` = xoá, chuỗi khác = đặt mới. */
  const apiKey = (): string | null => {
    if (keyMode() === "clear") return "";
    if (keyMode() === "keep") return null;
    const typed = keyText().trim();
    // Gõ dở rồi xoá sạch cũng là "không đổi gì", không phải "xoá khoá" — xoá phải là một
    // cú bấm cố ý vào đúng cái nút mang chữ đó.
    return typed === "" ? null : typed;
  };

  const draft = (): ProviderInput => ({
    id: props.provider?.id ?? null,
    name: name().trim(),
    kind: kind(),
    baseUrl: baseUrl().trim(),
    apiKey: apiKey(),
    enabled: enabled(),
    model: model(),
    // Chuỗi rỗng là "chưa đặt", nên gửi `null` — để `""` đi qua thì lõi lưu một tên mô
    // hình rỗng và nó sẽ trông y hệt một tên hợp lệ ở mọi chỗ đọc lại nó.
    embeddingModel: embeddingModel().trim() === "" ? null : embeddingModel().trim(),
  });

  const complete = () => name().trim() !== "" && baseUrl().trim() !== "";

  const runProbe = async () => {
    if (probing()) return;
    setProbing(true);
    setProbe(null);
    setProbeError(null);
    try {
      setProbe(await probeProvider(draft()));
    } catch (err) {
      setProbeError(err instanceof Error ? err.message : String(err));
    } finally {
      setProbing(false);
    }
  };

  return (
    <DialogShell
      icon={props.provider === null ? "plus" : "pencil"}
      title={props.provider === null ? "Thêm nhà cung cấp" : `Sửa ${props.provider.name}`}
      desc="Base URL quyết định dữ liệu đi tới đâu. Loopback thì nó không rời khỏi máy này."
      onClose={props.onClose}
      onSubmit={() => {
        if (complete() && !props.busy) props.onSubmit(draft());
      }}
      footer={() => (
        <>
          <Button
            label={probing() ? "Đang thử…" : "Thử kết nối"}
            variant="outline"
            icon="plug"
            busy={probing()}
            disabled={!complete()}
            onClick={() => void runProbe()}
          />
          <span class="flex-1" />
          <Button label="Huỷ" variant="ghost" onClick={props.onClose} />
          <Button
            label={props.provider === null ? "Thêm" : "Lưu"}
            type="submit"
            busy={props.busy}
            disabled={!complete()}
          />
        </>
      )}
    >
      <TextField
        label="Tên"
        value={name()}
        onInput={setName}
        placeholder="Ollama trên máy"
        ref={(el) => queueMicrotask(() => el.focus())}
      />

      <PillChoice<ProviderKind>
        label="Loại API"
        value={kind()}
        onPick={setKind}
        options={[
          { id: "ollama", label: "Ollama", icon: "plug" },
          { id: "openai", label: "Tương thích OpenAI", icon: "cloud" },
        ]}
        hint="LM Studio, llama.cpp, vLLM và phần lớn máy chủ khác đều nói giọng OpenAI."
      />

      <TextField
        label="Base URL"
        value={baseUrl()}
        onInput={setBaseUrl}
        mono
        placeholder={kind() === "ollama" ? "http://127.0.0.1:11434" : "https://api.openai.com/v1"}
      />

      {/* Ô này **không phải** nơi giao vai nhúng.
          Vai đó được chọn ở màn hình mô hình nhúng, cạnh câu nói rõ tài liệu sẽ đi tới
          đâu; đặt một cái công tắc vai ở đây thì một người đang sửa base URL có thể vô
          tình chuyển chỗ nhận toàn văn tài liệu của mình mà không đọc câu nào cả. */}
      <TextField
        label="Mô hình nhúng của provider này"
        value={embeddingModel()}
        onInput={setEmbeddingModel}
        mono
        placeholder={suggestedEmbeddingModel(kind())}
        hint="Chỉ có tác dụng nếu provider này được giao vai nhúng ở mục Mô hình nhúng. Để trống cũng được."
      />

      <KeySection
        hadKey={hadKey}
        kind={kind()}
        mode={keyMode()}
        text={keyText()}
        onMode={setKeyMode}
        onText={setKeyText}
      />

      <label class="flex items-center gap-sm text-xs text-text">
        <input
          type="checkbox"
          checked={enabled()}
          onChange={(event) => setEnabled(event.currentTarget.checked)}
          class="size-4 accent-[var(--accent)]"
        />
        Bật provider này
        <span class="text-2xs text-faint">Tắt thì nó vẫn nằm trong danh sách nhưng không được gọi.</span>
      </label>

      <ProbeResult busy={probing()} probe={probe()} error={probeError()} />

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert">
            {message()}
          </Banner>
        )}
      </Show>
    </DialogShell>
  );
}

/** Ô khoá API — ba trạng thái, mỗi trạng thái nói thẳng ra điều sẽ xảy ra khi bấm Lưu. */
function KeySection(props: {
  hadKey: boolean;
  kind: ProviderKind;
  mode: KeyMode;
  text: string;
  onMode: (mode: KeyMode) => void;
  onText: (text: string) => void;
}) {
  const optional = () => props.kind === "ollama";

  return (
    <div class="flex flex-col gap-2xs rounded-panel border border-line bg-surface-soft px-sm py-2xs">
      <div class="flex items-center gap-2xs text-2xs text-faint">
        <Icon name="key" size={12} />
        Khoá API
        <Show when={optional()}>
          <span class="text-faint">— Ollama chạy tại chỗ thường không cần</span>
        </Show>
      </div>

      <Show when={props.hadKey && props.mode === "keep"}>
        <div class="flex flex-wrap items-center gap-sm">
          <span class="inline-flex items-center gap-2xs rounded-pill bg-accent-soft px-2xs py-3xs text-2xs text-accent-ink">
            <Icon name="check" size={11} />
            Đã đặt
          </span>
          <span class="min-w-0 flex-1 text-2xs text-muted">
            Khoá đang nằm trong lõi và không được gửi ngược ra màn hình. Lưu biểu mẫu này
            sẽ <b>giữ nguyên</b> nó.
          </span>
          <Button label="Thay khoá" variant="outline" onClick={() => props.onMode("set")} />
          <Button label="Xoá khoá" variant="ghost" icon="trash" onClick={() => props.onMode("clear")} />
        </div>
      </Show>

      <Show when={props.mode === "clear"}>
        <div class="flex flex-wrap items-center gap-sm">
          <div class="min-w-0 flex-1">
            <Banner tone="danger" icon="warn">
              Bấm Lưu sẽ <b>xoá</b> khoá đã lưu. Provider này sẽ không gọi được cho tới khi
              có khoá mới.
            </Banner>
          </div>
          <Button label="Hoàn tác" variant="outline" onClick={() => props.onMode("keep")} />
        </div>
      </Show>

      <Show when={props.mode === "set"}>
        <div class="flex flex-col gap-2xs">
          <TextField
            label={props.hadKey ? "Khoá mới" : "Khoá"}
            type="password"
            value={props.text}
            onInput={props.onText}
            mono
            placeholder={optional() ? "để trống nếu máy chủ không yêu cầu" : "sk-…"}
            hint={
              props.hadKey
                ? "Để trống rồi bấm Lưu thì khoá cũ được giữ nguyên — đây không phải cách xoá khoá."
                : "Khoá đi thẳng vào lõi và không bao giờ được đọc ngược ra giao diện."
            }
          />
          <Show when={props.hadKey}>
            <div class="flex gap-sm">
              <Button label="Giữ khoá cũ" variant="ghost" onClick={() => props.onMode("keep")} />
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
}

/**
 * Kết quả thử kết nối.
 *
 * `message` hiện **nguyên văn**. Ba kiểu hỏng — không nối được, khoá bị từ chối, nối được
 * nhưng chưa có mô hình nào — dẫn tới ba việc phải làm khác hẳn nhau, và lõi đã phân biệt
 * sẵn trong câu chữ của nó. Gói cả ba vào một câu "Không kết nối được" là ném đi đúng
 * phần thông tin khiến người dùng sửa được.
 *
 * `role="status"` vì khối này xuất hiện *sau một hành động*: người dùng bàn phím bấm
 * "Thử kết nối" rồi không nhìn thấy gì nếu nó chỉ được vẽ ra một cách im lặng.
 */
function ProbeResult(props: { busy: boolean; probe: ProviderProbe | null; error: string | null }) {
  // "Nối được nhưng không có mô hình nào" đi qua `ok: true`, nhưng với một coding agent
  // thì nó chưa dùng được — nên nó mang sắc cảnh báo, không mang sắc thành công.
  const tone = () => {
    if (props.error !== null) return "danger" as const;
    const probe = props.probe;
    if (probe === null) return "info" as const;
    if (!probe.ok) return "danger" as const;
    return probe.models.length === 0 ? ("warn" as const) : ("accent" as const);
  };

  return (
    <div role="status" aria-live="polite" aria-busy={props.busy} class="flex flex-col gap-2xs">
      <Show when={props.busy}>
        <Banner tone="info" icon="refresh">
          Đang thử gọi tới máy chủ…
        </Banner>
      </Show>

      <Show when={props.error}>
        {(message) => (
          <Banner tone="danger" icon="warn" title="Không thử được">
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={!props.busy && props.probe}>
        {(probe) => (
          <Banner
            tone={tone()}
            icon={probe().ok ? "check" : "warn"}
            title={probe().ok ? "Máy chủ trả lời" : "Không dùng được"}
          >
            <p class="m-0 text-2xs">{probe().message}</p>
            <Show when={probe().models.length > 0}>
              {/* Chỉ liệt kê tên. Một lần thử **không** hỏi năng lực gọi tool của từng mô
                  hình — lõi trả `tools: false` cho tất cả để khỏi tốn một vòng gọi nữa —
                  nên dán nhãn "không gọi được tool" ở đây là dán sai lên cả danh sách.
                  Cờ có thẩm quyền nằm ở bộ chọn mô hình, nơi nó đến từ `list_models`. */}
              <ul class="m-0 mt-2xs flex max-h-32 list-none flex-col gap-3xs overflow-y-auto p-0">
                <For each={probe().models}>
                  {(choice) => (
                    <li class="min-w-0 truncate font-mono text-2xs">{choice.id}</li>
                  )}
                </For>
              </ul>
              <p class="m-0 mt-2xs text-2xs opacity-80">
                Đây là tên mô hình máy chủ đang có. Mô hình nào gọi được tool thì bộ chọn
                mô hình ở màn hình chính mới nói được, sau khi provider này được dùng.
              </p>
            </Show>
          </Banner>
        )}
      </Show>
    </div>
  );
}
