import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { probeProvider, suggestedEmbeddingModel } from "../../lib/providers";
import type {
  ModelChoice,
  Provider,
  ProviderInput,
  ProviderKind,
  ProviderProbe,
} from "../../lib/protocol";
import Icon from "./../Icon";
import { Banner, Button, DialogShell, PillChoice, TextField } from "../settings/FormKit";

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
  // Có setter, khác bản trước. Trước đây tên mô hình chỉ đến từ mục dựng sẵn và biểu mẫu
  // không có đường nào đặt nó — người vừa nối tới LM Studio nhìn thấy danh sách mô hình
  // của mình rồi vẫn phải rời hộp thoại, kích hoạt provider, và đi tìm một ô chọn khác.
  const [model, setModel] = createSignal<string | null>(start?.model ?? null);
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

  /**
   * Một số thứ tự cho mỗi lần hỏi máy chủ.
   *
   * Người dùng sửa URL nhanh hơn mạng trả lời, nên hai lần hỏi có thể đang bay cùng lúc và
   * cái **về sau** không nhất thiết là cái hỏi **sau cùng**. Không có con số này thì câu
   * trả lời của một địa chỉ đã bị xoá đi vẫn ghi đè lên danh sách của địa chỉ đang gõ dở,
   * và người dùng nhìn thấy mô hình của một máy chủ mà họ vừa bỏ.
   */
  let ticket = 0;
  let timer: number | undefined;

  /**
   * `auto` phân biệt lần nạp do máy tự chạy với lần do người bấm nút.
   *
   * Lần tự chạy **không** xoá kết quả cũ trước khi hỏi: xoá nó thì mỗi ký tự gõ thêm vào ô
   * URL làm cả khối kết quả nháy một cái rồi hiện lại: một màn hình co giật theo nhịp gõ.
   * Lần do người bấm thì ngược lại — họ vừa cố ý yêu cầu một câu trả lời mới, nên câu cũ
   * phải biến mất ngay để không ai đọc nhầm câu cũ thành câu mới.
   */
  const runProbe = async (auto: boolean) => {
    const mine = ++ticket;
    setProbing(true);
    if (!auto) {
      setProbe(null);
      setProbeError(null);
    }
    try {
      const result = await probeProvider(draft());
      if (mine !== ticket) return;
      setProbe(result);
      setProbeError(null);
      // Máy chủ chỉ khai đúng một mô hình thì không có gì để chọn — `llama-server` là
      // trường hợp điển hình, nó phục vụ đúng cái được truyền vào lúc khởi động. Bắt người
      // dùng bấm vào lựa chọn duy nhất là bắt họ xác nhận một việc không có phương án hai.
      const only = result.models.length === 1 ? (result.models[0]?.id ?? null) : null;
      if (model() === null && only !== null) setModel(only);
    } catch (err) {
      if (mine !== ticket) return;
      setProbe(null);
      setProbeError(err instanceof Error ? err.message : String(err));
    } finally {
      if (mine === ticket) setProbing(false);
    }
  };

  /**
   * Tự hỏi máy chủ khi địa chỉ đứng yên được một nhịp — không chờ ai bấm nút.
   *
   * Liệt kê mô hình là một `GET` không tốn gì với máy chủ chạy tại chỗ (LM Studio,
   * Ollama, llama.cpp), mà đó lại đúng là thứ người dùng đang muốn biết khi họ vừa dán một
   * địa chỉ vào: *máy chủ này có những gì*. Bắt họ tìm thêm một nút tên là "Thử kết nối"
   * để trả lời câu hỏi ấy là dựng một bước thừa ngay giữa lúc họ chờ đợi nhất.
   *
   * 700ms tính từ lần gõ cuối: đủ để một người gõ hết `http://localhost:1234/v1` mà chỉ
   * bị hỏi một lần, thay vì một lần cho mỗi ký tự.
   *
   * Đọc cả `keyMode`/`keyText` chứ không chỉ URL: một khoá vừa dán vào đổi hẳn câu trả lời
   * của máy chủ từ "từ chối" sang một danh sách thật.
   */
  createEffect(() => {
    const url = baseUrl().trim();
    // Đọc để đăng ký phụ thuộc — đổi loại API là đổi cả endpoint được gọi.
    void kind();
    void (keyMode() === "set" ? keyText().trim() : keyMode());
    if (url === "") return;
    clearTimeout(timer);
    timer = window.setTimeout(() => void runProbe(true), 700);
  });

  onCleanup(() => clearTimeout(timer));

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
            label={probing() ? "Đang thử…" : "Thử lại"}
            variant="outline"
            icon="plug"
            busy={probing()}
            disabled={!complete()}
            onClick={() => void runProbe(false)}
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
          { id: "lmstudio", label: "LM Studio", icon: "plug" },
          { id: "openai", label: "Tương thích OpenAI", icon: "cloud" },
        ]}
        hint="LM Studio có mục riêng vì kho mô hình của nó nói được mô hình nào đang nạp và
              gọi được công cụ; chọn “Tương thích OpenAI” cho nó thì mất phần đó. llama.cpp,
              vLLM và phần lớn máy chủ còn lại thì nói giọng OpenAI."
      />

      <TextField
        label="Base URL"
        value={baseUrl()}
        onInput={setBaseUrl}
        mono
        placeholder={
          kind() === "ollama"
            ? "http://127.0.0.1:11434"
            : kind() === "lmstudio"
              ? "http://127.0.0.1:1234"
              : "https://api.openai.com/v1"
        }
      />

      <KeySection
        hadKey={hadKey}
        kind={kind()}
        mode={keyMode()}
        text={keyText()}
        onMode={setKeyMode}
        onText={setKeyText}
      />

      {/* Trạng thái kết nối và danh sách mô hình đứng **ngay dưới** những ô quyết định
          chúng — địa chỉ và khoá. Đặt chúng ở cuối hộp thoại như bản trước thì câu trả lời
          nằm cách câu hỏi ba ô nhập, và người dùng sửa URL xong phải cuộn xuống mới biết
          mình vừa sửa đúng hay sai. */}
      <ProbeResult busy={probing()} probe={probe()} error={probeError()} />

      <ModelSection
        value={model()}
        onPick={setModel}
        models={probe()?.models ?? []}
        busy={probing()}
        touched={probe() !== null || probeError() !== null}
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
  // Hai loại chạy tại chỗ đều không đòi khoá theo mặc định.
  const optional = () => props.kind === "ollama" || props.kind === "lmstudio";

  return (
    <div class="flex flex-col gap-2xs rounded-panel border border-line bg-surface-soft px-sm py-2xs">
      <div class="flex items-center gap-2xs text-2xs text-faint">
        <Icon name="key" size={12} />
        Khoá API
        <Show when={optional()}>
          <span class="text-faint">— máy chủ chạy tại chỗ thường không cần</span>
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
            <p class="m-0 text-xs">{probe().message}</p>
          </Banner>
        )}
      </Show>
    </div>
  );
}


/**
 * Mô hình hội thoại của provider này — gõ được, và chọn được từ chính danh sách máy chủ
 * vừa khai.
 *
 * Bản trước không có khối này, và đó là chỗ luồng gãy: hộp thoại đã hỏi máy chủ, đã nhận
 * về đúng những cái tên người dùng cần, rồi in chúng ra dưới dạng chữ chết. Người vừa nối
 * tới LM Studio nhìn thấy `qwen3-coder-30b` trên màn hình vẫn phải **nhớ** cái tên ấy,
 * đóng hộp thoại, kích hoạt provider, rồi tìm một ô chọn khác ở một hàng khác. Danh sách
 * đó là lúc ý định của người dùng rõ ràng nhất trong cả màn hình; không gắn hành động vào
 * đúng lúc ấy là ném đi cơ hội tốt nhất.
 *
 * Vẫn giữ một ô gõ tay bên cạnh, không thay bằng một `<select>` thuần: `llama-server`
 * phục vụ đúng một mô hình và có bản không khai tên nào ra `/v1/models`, còn vài cổng
 * trung chuyển chỉ nhận đúng một chuỗi định danh mà chúng tự đặt. Một ô chỉ cho chọn
 * trong danh sách là một ô nói rằng những trường hợp đó không tồn tại.
 *
 * `tools` **không** được vẽ ở đây dù `ModelChoice` có mang cờ ấy: một lần thử cố ý không
 * trả tiền hỏi năng lực từng mô hình nên lõi trả `false` cho tất cả, và dán "không gọi
 * được tool" lên cả danh sách là nói sai. Cờ có thẩm quyền đến sau, ở bộ chọn mô hình
 * trong ô soạn tin, nơi nó đi ra từ `list_models`.
 */
function ModelSection(props: {
  value: string | null;
  onPick: (model: string | null) => void;
  models: ModelChoice[];
  busy: boolean;
  /** Đã hỏi máy chủ ít nhất một lần chưa — để phân biệt "chưa hỏi" với "hỏi rồi, rỗng". */
  touched: boolean;
}) {
  return (
    <div class="flex flex-col gap-2xs">
      <TextField
        label="Mô hình hội thoại"
        value={props.value ?? ""}
        onInput={(value) => props.onPick(value.trim() === "" ? null : value)}
        mono
        placeholder="chọn bên dưới, hoặc gõ tên mô hình"
        hint="Để trống cũng lưu được — nhưng provider chưa chọn mô hình thì chưa trò chuyện được."
      />

      <Show when={props.busy && props.models.length === 0}>
        <p class="m-0 text-xs text-muted" role="status" aria-busy="true">
          Đang hỏi máy chủ xem có những mô hình nào…
        </p>
      </Show>

      <Show when={props.models.length > 0}>
        {/* `radiogroup` chứ không phải một danh sách nút rời: đây là **một** lựa chọn có
            đúng một giá trị đang đúng, và trình đọc màn hình phải nghe được "3 trên 12"
            chứ không phải mười hai cái nút không liên quan gì tới nhau. */}
        <div
          role="radiogroup"
          aria-label="Mô hình máy chủ đang có"
          class="flex max-h-40 flex-col gap-3xs overflow-y-auto rounded-panel border border-line bg-surface-soft p-2xs"
        >
          <For each={props.models}>
            {(choice) => (
              <button
                type="button"
                role="radio"
                aria-checked={props.value === choice.id}
                onClick={() => props.onPick(choice.id)}
                class="flex min-w-0 items-center gap-2xs rounded-btn px-2xs py-3xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
                classList={{ "bg-accent-soft text-accent-ink": props.value === choice.id }}
              >
                {/* Ô đánh dấu giữ chỗ kể cả khi chưa chọn: không giữ thì mọi hàng nhích
                    ngang một nhịp mỗi lần đổi lựa chọn. */}
                <span class="w-3 shrink-0">
                  <Show when={props.value === choice.id}>
                    <Icon name="check" size={12} />
                  </Show>
                </span>
                <span class="min-w-0 flex-1 truncate font-mono text-xs">{choice.id}</span>
                <Show when={choice.contextWindow !== null}>
                  <span class="shrink-0 text-2xs text-faint">
                    {Intl.NumberFormat("vi-VN").format(choice.contextWindow ?? 0)} token
                  </span>
                </Show>
              </button>
            )}
          </For>
        </div>
      </Show>

      <Show when={props.touched && !props.busy && props.models.length === 0}>
        <p class="m-0 text-xs text-muted">
          Máy chủ chưa khai mô hình nào, nên không có gì để chọn. Gõ thẳng tên mô hình vào ô
          trên nếu bạn biết máy chủ này nhận tên gì.
        </p>
      </Show>
    </div>
  );
}
