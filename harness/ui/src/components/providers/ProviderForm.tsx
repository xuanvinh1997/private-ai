import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { probeProvider, suggestedEmbeddingModel } from "../../lib/providers";
import type {
  ModelChoice,
  Provider,
  ProviderInput,
  ProviderKind,
  ProviderPreset,
  ProviderProbe,
} from "../../lib/protocol";
import ModelField from "./ModelField";
import Icon from "./../Icon";
import {
  Banner,
  Button,
  DialogShell,
  ExternalLink,
  InfoDot,
  PillChoice,
  TextField,
} from "../settings/FormKit";

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
  /**
   * Mục danh mục người dùng vừa bấm "Nối".
   *
   * Nhận nguyên hàng chứ không nhận một bản rút gọn: `needsKey` và `onDevice` quyết định
   * hộp thoại mở ra ở trạng thái nào, và một bản rút gọn ở chỗ gọi nghĩa là chỗ gọi phải
   * biết trước hộp thoại cần gì — thứ sẽ lệch ngay lần sửa sau.
   */
  preset?: ProviderPreset | null;
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
  const [model, setModel] = createSignal<string | null>(
    props.provider?.model ?? props.preset?.defaultModel ?? null,
  );
  const [enabled, setEnabled] = createSignal(props.provider?.enabled ?? true);
  // Đọc từ `props.provider` chứ không từ `start`: một mục dựng sẵn không mang mô hình
  // nhúng, và ghép hai nguồn vào một biểu thức chỉ để tiết kiệm một dòng thì kiểu của
  // trường này phụ thuộc vào nhánh nào đang chạy.
  const [embeddingModel, setEmbeddingModel] = createSignal(props.provider?.embeddingModel ?? "");

  const hadKey = props.provider?.hasKey === true;
  const [keyMode, setKeyMode] = createSignal<KeyMode>(hadKey ? "keep" : "set");
  const [keyText, setKeyText] = createSignal("");

  /**
   * Địa chỉ đã có sẵn câu trả lời chưa.
   *
   * Đúng với hai lối vào: bấm "Nối" trên một hàng danh mục, và sửa một provider đã lưu.
   * Chỉ nhánh tự khai báo mới thật sự chưa biết gì.
   */
  const known = props.provider !== null || (props.preset ?? null) !== null;

  /**
   * Khối tên + loại + địa chỉ có đang mở không.
   *
   * Mở sẵn **chỉ khi chưa biết gì**. Bản trước mở cả bốn ô cho mọi lối vào, nên người
   * bấm "Nối" trên hàng Ollama gặp ô đầu tiên — cái được trao tiêu điểm — là "Tên", tức
   * là ô ít quan trọng nhất trong hộp thoại, trong khi câu hỏi thật sự còn lại của họ chỉ
   * là *dùng mô hình nào*.
   */
  const [open, setOpen] = createSignal(!known);

  /**
   * Ô khoá đứng ngoài khối thu gọn khi provider **cần khoá mà chưa có**.
   *
   * Đó là lúc ô khoá chính là câu hỏi duy nhất còn lại, và giấu câu hỏi duy nhất sau một
   * nút "Sửa" là bắt người dùng đi tìm thứ họ vừa được hứa là sẽ được hỏi.
   */
  const needsKeyNow = () => props.preset?.needsKey === true && !hadKey;

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
      // Máy chủ không trả lời thì địa chỉ và khoá đúng là thứ phải sửa, nên mở khối ấy
      // ra. Chỉ mở, không bao giờ tự thu lại: người dùng mở nó ra là có việc.
      if (!result.ok) setOpen(true);
      // Máy chủ chỉ khai đúng một mô hình thì không có gì để chọn — `llama-server` là
      // trường hợp điển hình, nó phục vụ đúng cái được truyền vào lúc khởi động. Bắt người
      // dùng bấm vào lựa chọn duy nhất là bắt họ xác nhận một việc không có phương án hai.
      const only = result.models.length === 1 ? (result.models[0]?.id ?? null) : null;
      if (model() === null && only !== null) setModel(only);
    } catch (err) {
      if (mine !== ticket) return;
      setProbe(null);
      setProbeError(err instanceof Error ? err.message : String(err));
      setOpen(true);
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
      icon={props.provider !== null ? "pencil" : ((props.preset?.onDevice ?? false) ? "plug" : "cloud")}
      title={
        props.provider !== null
          ? `Sửa ${props.provider.name}`
          : props.preset !== null && props.preset !== undefined
            ? `Nối tới ${props.preset.name}`
            : "Tự khai báo nhà cung cấp"
      }
      desc={props.preset?.hint ?? "Base URL quyết định dữ liệu đi tới đâu."}
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
      {/* Tên, loại và địa chỉ nằm trong một khối **thu được**.
          Bấm "Nối" trên một hàng danh mục là đã trả lời cả ba câu ấy rồi, và mở sẵn cả ba
          ô ở đây thì câu hỏi thật sự còn lại — *dùng mô hình nào* — bị đẩy xuống dưới ba
          thứ người dùng không cần đọc. Khối tự mở ra khi máy chủ không trả lời, vì khi đó
          nó lại đúng là chỗ phải sửa. */}
      <Show
        when={open()}
        fallback={
          <Summary
            name={name()}
            baseUrl={baseUrl()}
            hadKey={hadKey}
            kind={kind()}
            onOpen={() => setOpen(true)}
          />
        }
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
          hint="Mục LM Studio đọc được mô hình nào đang nạp."
          more="LM Studio có mục riêng vì kho mô hình của nó nói được mô hình nào đang nạp và gọi được công cụ; chọn “Tương thích OpenAI” cho nó thì mất phần đó. llama.cpp, vLLM và phần lớn máy chủ còn lại thì nói giọng OpenAI."
        />

        <TextField
          label="Base URL"
          value={baseUrl()}
          onInput={setBaseUrl}
          mono
          hint="Loopback thì dữ liệu không rời khỏi máy này."
          more="Base URL quyết định dữ liệu đi tới đâu. Loopback thì nó không rời khỏi máy này."
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
      </Show>

      {/* Khối trên thu lại mà provider vẫn cần khoá thì ô khoá đứng riêng ra ngoài: đó là
          câu hỏi duy nhất còn lại, và nó được trao tiêu điểm ngay. */}
      <Show when={!open() && needsKeyNow()}>
        <KeySection
          hadKey={hadKey}
          kind={kind()}
          mode={keyMode()}
          text={keyText()}
          onMode={setKeyMode}
          onText={setKeyText}
          focus
        />
      </Show>

      {/* Link trang chủ đứng đúng **một chỗ** trong cả luồng: ngay cạnh ô khoá, tức là
          đúng lúc câu hỏi trong đầu người dùng là "lấy cái khoá này ở đâu". Ở hàng danh
          mục thì nó chỉ là một liên kết lặp xuống mọi hàng mà chưa ai cần tới. */}
      <Show when={needsKeyNow() && props.preset}>
        {(preset) => (
          <p class="m-0 text-2xs text-faint">
            Lấy khoá ở <ExternalLink href={preset().homepage}>{preset().name}</ExternalLink>.
          </p>
        )}
      </Show>

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
          tình chuyển chỗ nhận toàn văn tài liệu của mình mà không đọc câu nào cả.

          Danh sách lấy từ chính lần thử vừa chạy ở trên — không tốn thêm một lời gọi nào,
          và đó là lý do nó đứng được ở đây: người dùng vừa dán một địa chỉ vào và máy chủ
          vừa đọc ra nó có gì, nên bắt họ nhớ tên mô hình nhúng ngay lúc ấy là bắt họ nhớ
          một thứ đang hiện trên màn hình. */}
      <ModelField
        role="embedding"
        label="Mô hình nhúng của provider này"
        showLabel
        models={probe()?.models ?? []}
        value={embeddingModel()}
        onInput={setEmbeddingModel}
        placeholder={suggestedEmbeddingModel(kind())}
        hint="Để trống được; chỉ dùng khi provider giữ vai nhúng."
        more="Chỉ có tác dụng nếu provider này được giao vai nhúng ở mục Mô hình nhúng. Để trống cũng được."
      />

      {/* Chỉ khi **sửa**. Không ai thêm một provider để nó nằm im, nên ở lối thêm mới thì
          ô này là một câu hỏi có đúng một câu trả lời — và một câu hỏi như thế chỉ tổ làm
          hộp thoại dài thêm một dòng. Tắt một provider thì gạt công tắc ở hàng của nó. */}
      <Show when={props.provider !== null}>
        <label class="flex items-center gap-sm text-xs text-text">
          <input
            type="checkbox"
            checked={enabled()}
            onChange={(event) => setEnabled(event.currentTarget.checked)}
            class="size-4 accent-[var(--accent)]"
          />
          Bật provider này
          <span class="text-2xs text-faint">Tắt thì vẫn trong danh sách nhưng không được gọi.</span>
        </label>
      </Show>

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

/**
 * Khối kết nối lúc **thu lại**: một dòng nói đủ nó đang trỏ đi đâu, và một nút mở ra.
 *
 * Ba thứ trên dòng này là ba thứ người dùng cần đối chiếu trước khi bấm Lưu — tên, địa
 * chỉ, và khoá có hay không. Loại API thì không: nó đã hiện thành biểu tượng ổ cắm hay
 * đám mây, và cái tên "Tương thích OpenAI" chẳng nói thêm gì cho người vừa bấm "Nối" trên
 * hàng LM Studio.
 *
 * Địa chỉ để `font-mono` và **không** bị cắt: một base URL sai thường sai ở cổng hay ở
 * đuôi `/v1`, tức là đúng đoạn mà một dòng `truncate` ăn mất. Nó xuống dòng thay vì bị cắt.
 */
function Summary(props: {
  name: string;
  baseUrl: string;
  hadKey: boolean;
  kind: ProviderKind;
  onOpen: () => void;
}) {
  const local = () => props.kind === "ollama" || props.kind === "lmstudio";
  return (
    <div class="flex flex-wrap items-center gap-sm rounded-panel border border-line bg-surface-soft px-sm py-2xs">
      <span
        class="grid size-7 shrink-0 place-items-center rounded-panel"
        classList={{
          "bg-accent-soft text-accent-ink": local(),
          "bg-[var(--overlay-faint)] text-muted": !local(),
        }}
      >
        <Icon name={local() ? "plug" : "cloud"} size={14} />
      </span>

      <span class="flex min-w-0 flex-1 flex-col gap-3xs">
        <span class="truncate text-xs font-medium text-ink">{props.name}</span>
        <span class="font-mono text-2xs break-all text-faint">{props.baseUrl}</span>
      </span>

      <Show when={props.hadKey}>
        <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-accent-soft px-2xs py-3xs text-2xs text-accent-ink">
          <Icon name="key" size={10} />
          Đã có khoá
        </span>
      </Show>

      <Button label="Sửa" variant="ghost" icon="pencil" onClick={props.onOpen} />
    </div>
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
  /** Trao tiêu điểm cho ô khoá — dùng khi khoá là câu hỏi duy nhất của hộp thoại. */
  focus?: boolean;
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
          <span class="flex min-w-0 flex-1 items-center gap-2xs text-2xs text-muted">
            Lưu biểu mẫu này sẽ <b>giữ nguyên</b> khoá đã lưu.
            <InfoDot
              label="Khoá được giữ ở đâu"
              text="Khoá đang nằm trong lõi và không được gửi ngược ra màn hình. Lưu biểu mẫu này sẽ giữ nguyên nó."
            />
          </span>
          <Button label="Thay khoá" variant="outline" onClick={() => props.onMode("set")} />
          <Button label="Xoá khoá" variant="ghost" icon="trash" onClick={() => props.onMode("clear")} />
        </div>
      </Show>

      <Show when={props.mode === "clear"}>
        <div class="flex flex-wrap items-center gap-sm">
          <div class="min-w-0 flex-1">
            <Banner tone="danger" icon="warn" title="Bấm Lưu sẽ xoá khoá đã lưu">
              <b>Không gọi được</b> cho tới khi có khoá mới.
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
            ref={
              props.focus === true
                ? (el) => queueMicrotask(() => el.focus())
                : undefined
            }
            mono
            placeholder={optional() ? "để trống nếu máy chủ không yêu cầu" : "sk-…"}
            hint={
              props.hadKey
                ? "Để trống là giữ khoá cũ, không phải xoá khoá."
                : "Khoá đi thẳng vào lõi, không đọc ngược ra được."
            }
            more="Lõi không bao giờ trả khoá về giao diện, nên ô này chỉ nhận khoá mới chứ không hiện lại khoá cũ. Để trống rồi bấm Lưu thì khoá cũ được giữ nguyên — đây không phải cách xoá khoá."
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
 * Bản trước dựng nó thành một ô gõ tay **cộng** một hộp cuộn đầy nút bấm — hai ô cho một
 * lựa chọn, và cái hộp ấy chiếm hơn một phần tư chiều cao hộp thoại để nói đúng cái mà
 * một dòng `<select>` nói được. Giờ cả hai vai dùng chung [`ModelField`], nên hộp thoại
 * có hai hàng giống hệt nhau: chọn mô hình hội thoại, chọn mô hình nhúng.
 *
 * Lối gõ tay không mất đi, nó nằm trong mục "Gõ tên khác…" của chính ô chọn: `llama-server`
 * phục vụ đúng một mô hình và có bản không khai tên nào ra `/v1/models`, còn vài cổng
 * trung chuyển chỉ nhận đúng một chuỗi định danh mà chúng tự đặt. Một ô chỉ cho chọn
 * trong danh sách là một ô nói rằng những trường hợp đó không tồn tại.
 *
 * Một dòng chữ dưới ô, ba trạng thái — đang hỏi, hỏi rồi mà rỗng, và câu nhắc thường
 * trực. Ba dòng xếp chồng thì hai trong ba lúc nào cũng là chữ thừa.
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
      <ModelField
        role="chat"
        label="Mô hình hội thoại"
        showLabel
        models={props.models}
        value={props.value ?? ""}
        onInput={(value) => props.onPick(value.trim() === "" ? null : value)}
        placeholder="gõ tên mô hình, hoặc thử kết nối để chọn"
        more="Để trống cũng lưu được — nhưng provider chưa chọn mô hình thì chưa trò chuyện được."
      />

      {/* Một dòng, ba trạng thái, theo thứ tự thời gian: đang hỏi → hỏi xong mà rỗng →
          câu nhắc thường trực. */}
      <Show
        when={props.busy && props.models.length === 0}
        fallback={
          <Show
            when={props.touched && !props.busy && props.models.length === 0}
            fallback={
              <p class="m-0 text-2xs text-faint">
                Để trống vẫn lưu được, nhưng chưa trò chuyện được.
              </p>
            }
          >
            <p class="m-0 flex items-center gap-2xs text-2xs text-muted">
              Máy chủ chưa khai mô hình nào để chọn.
              <InfoDot
                label="Không có mô hình nào thì làm gì"
                text="Máy chủ chưa khai mô hình nào, nên không có gì để chọn. Gõ thẳng tên mô hình vào ô trên nếu bạn biết máy chủ này nhận tên gì."
              />
            </p>
          </Show>
        }
      >
        <p class="m-0 text-2xs text-muted" role="status" aria-busy="true">
          Đang hỏi máy chủ xem có những mô hình nào…
        </p>
      </Show>
    </div>
  );
}
