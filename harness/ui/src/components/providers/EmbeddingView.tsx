import { createSignal, onMount, Show } from "solid-js";
import {
  embeddingSetting,
  listProviders,
  probeEmbedding,
  setEmbedding,
  suggestedEmbeddingModel,
} from "../../lib/providers";
import type { EmbeddingProbe, EmbeddingSetting, Provider } from "../../lib/protocol";
import ConfirmDialog from "./ConfirmDialog";
import { Banner, Button, Row, RowGroup, SectionHead, Select, TextField } from "../settings/FormKit";

const NONE: EmbeddingSetting = {
  providerId: null,
  providerName: null,
  model: null,
  onDevice: false,
  reason: null,
};

/**
 * Màn hình mô hình nhúng — đứng riêng, không phải một mục trong trang provider.
 *
 * Đứng riêng vì hai vai là hai quyết định khác nhau về *quyền riêng tư*, không chỉ khác
 * nhau về kỹ thuật. Chọn mô hình trò chuyện là chọn nơi nhận câu hỏi của bạn; chọn mô
 * hình nhúng là chọn nơi nhận **toàn văn mọi tài liệu bạn nạp vào**. Người ta nạp hợp
 * đồng, hồ sơ bệnh án, ghi chú riêng vào đây, nên cách ghép hợp lý nhất lại là ghép chéo:
 * nhúng bằng một mô hình nhỏ chạy tại chỗ, trò chuyện bằng một mô hình lớn từ xa. Gộp hai
 * lựa chọn vào một chỗ là ngầm loại bỏ đúng cấu hình đó.
 *
 * Ba thứ màn hình này phải nói ra, và chúng quyết định cả bố cục:
 *
 *   1. **Tài liệu đi tới đâu.** Huy hiệu "chạy trên máy này" ở đây nặng hơn ở trang hội
 *      thoại, nên nó không phải một cái nhãn nhỏ mà là một câu đầy đủ ngay dưới ô chọn.
 *   2. **Nút thử làm gì.** Nó nhúng thật một câu và đo số chiều — khác hẳn nút thử ở
 *      trang provider, thứ chỉ liệt kê tên mô hình.
 *   3. **Đổi mô hình là nhúng lại tất cả.** Hỏi xác nhận, và câu xác nhận nói đúng cái
 *      đó, chứ không dọa chung chung.
 */
export default function EmbeddingView() {
  const [providers, setProviders] = createSignal<Provider[]>([]);
  const [setting, setSetting] = createSignal<EmbeddingSetting>(NONE);
  const [ready, setReady] = createSignal(false);

  // Bản nháp của người dùng, tách khỏi `setting()` đang có hiệu lực: cả màn hình xoay
  // quanh việc so hai cái đó với nhau để biết có phải nhúng lại hay không.
  const [providerId, setProviderId] = createSignal("");
  const [model, setModel] = createSignal("");

  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [confirming, setConfirming] = createSignal(false);

  const [probing, setProbing] = createSignal(false);
  const [probe, setProbe] = createSignal<EmbeddingProbe | null>(null);
  const [probeError, setProbeError] = createSignal<string | null>(null);

  const chosen = () => providers().find((entry) => entry.id === providerId()) ?? null;
  const suggestion = () => {
    const entry = chosen();
    return entry === null ? "" : suggestedEmbeddingModel(entry.kind);
  };

  /** Mô hình đã lưu của provider đó, hoặc gợi ý theo loại nó — luôn sửa được. */
  const modelFor = (entry: Provider | null) =>
    entry === null ? "" : (entry.embeddingModel ?? suggestedEmbeddingModel(entry.kind));

  onMount(() => {
    void (async () => {
      const [list, current] = await Promise.all([listProviders(), embeddingSetting()]);
      setProviders(list);
      setSetting(current);

      // Chưa cấu hình gì thì trỏ sẵn vào provider chạy tại chỗ đầu tiên. Đó là lựa chọn
      // giữ tài liệu trong máy, và mặc định của một màn hình về quyền riêng tư phải là
      // lựa chọn an toàn — không phải cái đứng đầu danh sách một cách tình cờ.
      const fallback =
        list.find((entry) => entry.enabled && entry.onDevice) ??
        list.find((entry) => entry.enabled) ??
        null;
      const start = list.find((entry) => entry.id === current.providerId) ?? fallback;
      setProviderId(start?.id ?? "");
      setModel(current.model ?? modelFor(start));
      setReady(true);
    })();
  });

  const pickProvider = (id: string) => {
    const entry = providers().find((item) => item.id === id) ?? null;
    setProviderId(id);
    setModel(modelFor(entry));
    // Kết quả thử cũ nói về một máy chủ khác. Giữ nó lại là để một dấu tích xanh chứng
    // nhận cho một cấu hình chưa ai thử.
    setProbe(null);
    setProbeError(null);
  };

  const complete = () => providerId() !== "" && model().trim() !== "";
  const dirty = () =>
    providerId() !== (setting().providerId ?? "") || model().trim() !== (setting().model ?? "");

  /** Đã có vector trong thư viện thì đổi cấu hình là nhúng lại — phải hỏi trước. */
  const needsConfirm = () =>
    setting().providerId !== null && setting().model !== null && dirty();

  const apply = async () => {
    if (!complete()) return;
    setBusy(true);
    setError(null);
    try {
      await setEmbedding(providerId(), model().trim());
      const [list, current] = await Promise.all([listProviders(), embeddingSetting()]);
      setProviders(list);
      setSetting(current);
      setConfirming(false);
    } catch (err) {
      setError(`Không đặt được mô hình nhúng: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const runProbe = async () => {
    if (probing() || !complete()) return;
    setProbing(true);
    setProbe(null);
    setProbeError(null);
    try {
      setProbe(await probeEmbedding(providerId(), model().trim()));
    } catch (err) {
      setProbeError(err instanceof Error ? err.message : String(err));
    } finally {
      setProbing(false);
    }
  };

  const options = () => {
    const list = providers()
      // Provider đang tắt vẫn hiện, kèm chữ "đang tắt": giấu nó đi thì một cấu hình nhúng
      // đang trỏ vào provider bị tắt sẽ biến mất khỏi ô chọn mà không ai hiểu vì sao.
      .map((entry) => ({
        id: entry.id,
        label: `${entry.name}${entry.enabled ? "" : " (đang tắt)"}${entry.onDevice ? " · trên máy này" : ""}`,
      }));
    return providerId() === "" ? [{ id: "", label: "— chưa chọn —" }, ...list] : list;
  };

  return (
    <div class="flex flex-col gap-2xl">
      <SectionHead
        title="Máy chủ giữ vai nhúng"
        desc="Mô hình biến tài liệu thành vector để tìm theo ý nghĩa. Nó tách hẳn khỏi mô hình trò chuyện, và chọn riêng ở đây."
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title="Không làm được">
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={ready()} fallback={<Skeleton />}>
        <Show
          when={providers().length > 0}
          fallback={
            <div class="rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-2xl">
              <p class="m-0 max-w-[56ch] text-xs text-muted">
                Chưa có nhà cung cấp nào để giao vai nhúng. Thêm một provider ở mục "Nhà
                cung cấp mô hình" trước — một cái chạy tại chỗ là đủ, và nó giữ tài liệu
                trong máy này.
              </p>
            </div>
          }
        >
          <CurrentState setting={setting()} />

          <RowGroup>
            <Row
              label="Provider nhúng"
              desc="Nơi toàn văn tài liệu được gửi tới để biến thành vector."
              control={() => (
                <Select
                  label="Provider dùng để nhúng tài liệu"
                  value={providerId()}
                  options={options()}
                  disabled={busy()}
                  onPick={pickProvider}
                />
              )}
              below={() => (
                <Show when={chosen()}>{(entry) => <Privacy provider={entry()} />}</Show>
              )}
            />

            <Row
              label="Mô hình nhúng"
              desc="Điền sẵn theo loại provider, nhưng sửa được — máy bạn có thể đang chạy một mô hình khác."
              control={() => (
                <div class="w-[280px] max-w-full">
                  <TextField
                    label="Tên mô hình nhúng"
                    hideLabel
                    mono
                    value={model()}
                    disabled={busy() || providerId() === ""}
                    placeholder={suggestion()}
                    onInput={(value) => {
                      setModel(value);
                      setProbe(null);
                      setProbeError(null);
                    }}
                  />
                </div>
              )}
            />

            <Row
              label="Thử nhúng một câu"
              desc="Phép thử này gửi thật một câu đi và báo lại số chiều của vector nhận về. Nó khác nút thử ở trang provider: danh sách mô hình của máy chủ liệt kê mọi mô hình và không có gì trong đó nói cái nào nhúng được."
              control={() => (
                <Button
                  label={probing() ? "Đang nhúng thử…" : "Thử ngay"}
                  variant="outline"
                  icon="plug"
                  busy={probing()}
                  disabled={!complete() || busy()}
                  onClick={() => void runProbe()}
                />
              )}
              below={() => (
                <ProbeResult busy={probing()} probe={probe()} error={probeError()} />
              )}
            />
          </RowGroup>

          <div class="flex flex-wrap items-center justify-end gap-sm">
            <Show when={dirty() && complete()}>
              <span class="mr-auto text-2xs text-muted">
                {needsConfirm()
                  ? "Lưu thay đổi này sẽ nhúng lại toàn bộ thư viện tài liệu."
                  : "Chưa có vector nào trong thư viện, nên lần lưu này không phải nhúng lại gì."}
              </span>
            </Show>
            <Button
              label="Lưu mô hình nhúng"
              icon="check"
              busy={busy()}
              disabled={!complete() || !dirty()}
              onClick={() => {
                if (needsConfirm()) setConfirming(true);
                else void apply();
              }}
            />
          </div>
        </Show>
      </Show>

      <Show when={confirming()}>
        <ConfirmDialog
          title="Nhúng lại toàn bộ thư viện?"
          body="Đổi mô hình nhúng làm lõi bỏ hết vector cũ và nhúng lại từng tài liệu bằng mô hình mới. Bắt buộc phải thế: vector của hai mô hình nằm ở hai không gian khác nhau, và đem so với nhau thì ra một con số vô nghĩa trông y hệt một con số có nghĩa — tức là kết quả tìm kiếm sai mà không có gì báo sai. Trong lúc nhúng lại, thư viện vẫn tìm được bằng từ khoá; chỉ phần tìm theo ý nghĩa là tạm thiếu."
          detail={`Đang dùng:  ${setting().providerName ?? "?"} · ${setting().model ?? "?"}\nSẽ dùng:    ${chosen()?.name ?? "?"} · ${model().trim()}`}
          confirmLabel="Đổi và nhúng lại"
          busy={busy()}
          onConfirm={() => void apply()}
          onClose={() => setConfirming(false)}
        />
      </Show>
    </div>
  );
}

/**
 * Cấu hình đang có hiệu lực, đứng trên đầu trang.
 *
 * Trạng thái "chưa cấu hình" cố ý **không** mang sắc lỗi: thư viện tài liệu vẫn chạy,
 * chỉ là tìm bằng từ khoá. Vẽ nó bằng màu đỏ là đẩy người dùng đi cấu hình một thứ họ có
 * thể không cần, và ở đây thứ đó lại đúng là thứ gửi tài liệu của họ đi đâu đó.
 */
function CurrentState(props: { setting: EmbeddingSetting }) {
  return (
    <>
      <Show when={props.setting.providerId === null}>
        <div class="flex flex-col gap-2xs rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
          <span class="text-xs font-medium text-ink">Chưa cấu hình nhúng</span>
          <p class="m-0 max-w-[62ch] text-2xs text-muted">
            Thư viện tài liệu vẫn dùng được: nó tìm bằng <b>từ khoá</b>, nghĩa là bạn phải
            gõ đúng chữ có trong tài liệu chứ không hỏi được bằng ý. Đây là một trạng thái
            dùng được, không phải một lỗi — chọn mô hình nhúng bên dưới nếu bạn muốn hỏi
            bằng ý, và chọn một provider chạy tại chỗ nếu tài liệu không được rời khỏi máy.
          </p>
        </div>
      </Show>

      <Show when={props.setting.providerId !== null && props.setting.reason}>
        {(reason) => (
          <Banner tone="warn" icon="warn" title="Cấu hình nhúng chưa dùng được">
            {reason()} Cho tới khi sửa xong, thư viện tài liệu chỉ tìm được bằng từ khoá.
          </Banner>
        )}
      </Show>

      <Show when={props.setting.providerId !== null && props.setting.reason === null}>
        <Banner tone="accent" icon="check" title="Đang nhúng bằng mô hình này">
          <code class="font-mono">{props.setting.model}</code> trên{" "}
          {props.setting.providerName}.{" "}
          {props.setting.onDevice
            ? "Tài liệu không rời khỏi máy này."
            : `Toàn văn mỗi tài liệu được gửi tới ${props.setting.providerName} để nhúng.`}
        </Banner>
      </Show>
    </>
  );
}

/**
 * Câu về quyền riêng tư của provider đang chọn.
 *
 * Chỉ đọc cờ `onDevice` của lõi, không đoán lại từ base URL: huy hiệu này là một *lời
 * hứa*, và một lời hứa đoán sai là thứ tệ nhất màn hình có thể vẽ ra. Câu chữ ở đây dài
 * hơn ở trang provider vì thứ đi qua đây cũng khác: không phải câu hỏi của người dùng,
 * mà là toàn văn từng tài liệu họ nạp vào.
 */
function Privacy(props: { provider: Provider }) {
  return (
    <Show
      when={props.provider.onDevice}
      fallback={
        <Banner tone="warn" icon="cloud" title="Tài liệu được gửi ra khỏi máy">
          Nhúng bằng {props.provider.name} nghĩa là <b>toàn văn</b> mỗi tài liệu bạn nạp
          vào được gửi tới <code class="font-mono">{props.provider.baseUrl}</code>. Nhúng
          lại cả thư viện thì gửi lại tất cả một lần nữa.
        </Banner>
      }
    >
      <Banner tone="accent" icon="plug" title="Chạy trên máy này">
        Tài liệu bạn nạp vào — hợp đồng, hồ sơ, ghi chú riêng — được nhúng ngay tại đây và{" "}
        <b>không rời khỏi máy này</b>. Không có yêu cầu mạng nào mang nội dung của chúng đi.
      </Banner>
    </Show>
  );
}

/**
 * Kết quả thử.
 *
 * Số chiều được nói thẳng ra vì nó là **bằng chứng** duy nhất rằng một câu đã đi qua và
 * một vector đã quay về. "Thành công" không kèm con số thì không phân biệt được với "máy
 * chủ trả 200 rỗng".
 */
function ProbeResult(props: { busy: boolean; probe: EmbeddingProbe | null; error: string | null }) {
  return (
    <div role="status" aria-live="polite" aria-busy={props.busy} class="flex flex-col gap-2xs">
      <Show when={props.busy}>
        <Banner tone="info" icon="refresh">
          Đang gửi một câu đi để nhúng thử…
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
        {(result) => (
          <Banner
            tone={result().ok ? "accent" : "danger"}
            icon={result().ok ? "check" : "warn"}
            title={result().ok ? "Nhúng được" : "Không nhúng được"}
          >
            <p class="m-0">{result().message}</p>
            <Show when={result().dimensions}>
              {(dims) => (
                <p class="m-0 mt-2xs">
                  Vector nhận về có{" "}
                  <b class="tabular-nums">{Intl.NumberFormat("vi-VN").format(dims())}</b>{" "}
                  chiều. Đây là con số đo từ một vector thật, không phải từ một danh sách
                  mô hình.
                </p>
              )}
            </Show>
          </Banner>
        )}
      </Show>
    </div>
  );
}

/** Khung xương lúc nạp: giữ đúng chiều cao ba hàng để trang không giật khi hiện ra. */
function Skeleton() {
  return (
    <div class="flex flex-col divide-y divide-line rounded-card border border-line bg-surface" aria-hidden="true">
      <div class="flex items-center gap-md px-(--card-pad-x) py-sm">
        <span class="h-3 w-1/4 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
      </div>
      <div class="flex items-center gap-md px-(--card-pad-x) py-sm">
        <span class="h-3 w-1/3 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
      </div>
      <div class="flex items-center gap-md px-(--card-pad-x) py-sm">
        <span class="h-3 w-1/5 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
      </div>
    </div>
  );
}
