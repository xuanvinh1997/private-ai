import { Key } from "@solid-primitives/keyed";
import { createResource, createSignal, For, onMount, Show } from "solid-js";
import {
  activeModels,
  inputOf,
  listProviders,
  providerPresets,
  removeProvider,
  saveProvider,
  setActiveProvider,
  setProviderModel,
} from "../../lib/providers";
import type { ModelChoice, Provider, ProviderInput, ProviderPreset } from "../../lib/protocol";
import Icon from "./../Icon";
import { IconButton } from "./../primitives";
import ConfirmDialog from "./ConfirmDialog";
import { Banner, Button, SectionHead, Toggle } from "./FormKit";
import PresetPicker from "./PresetPicker";
import ProviderForm from "./ProviderForm";

type Sheet =
  | { kind: "none" }
  | { kind: "presets" }
  | { kind: "form"; provider: Provider | null; preset: ProviderPreset | null }
  | { kind: "delete"; provider: Provider };

/**
 * Màn hình nhà cung cấp mô hình.
 *
 * Ba câu hỏi, theo đúng thứ tự người dùng hỏi chúng: *cái nào đang chạy*, *dữ liệu của
 * tôi có rời khỏi máy không*, và *mô hình này có làm được việc không*. Bố cục bám theo
 * thứ tự đó — dấu "đang dùng" và huy hiệu "chạy trên máy này" nằm trên cùng một hàng với
 * cái tên, còn bộ chọn mô hình bung ra ngay dưới provider đang hoạt động chứ không nằm ở
 * một khu riêng, vì mô hình chỉ có nghĩa khi đi kèm provider của nó.
 */
export default function ProvidersView() {
  const [providers, setProviders] = createSignal<Provider[]>([]);
  const [presets, setPresets] = createSignal<ProviderPreset[]>([]);
  const [ready, setReady] = createSignal(false);
  const [sheet, setSheet] = createSignal<Sheet>({ kind: "none" });
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [formError, setFormError] = createSignal<string | null>(null);

  const active = () => providers().find((entry) => entry.active) ?? null;

  /**
   * Mô hình của provider đang hoạt động.
   *
   * Qua `activeModels()` — tức `list_models` — chứ không qua `probe_provider`: chỉ ở đó
   * cờ `tools` mới có thẩm quyền, và cả bộ chọn mô hình bên dưới treo một cảnh báo lên
   * đúng cái cờ đó.
   *
   * Khoá theo *nội dung* cấu hình chứ không theo tham chiếu mảng: `providers()` dựng lại
   * một mảng mới sau mỗi lần bật/tắt, và khoá theo tham chiếu sẽ gọi lại máy chủ mỗi lần
   * người dùng gạt một công tắc chẳng liên quan.
   */
  const activeKey = () => {
    const entry = active();
    return entry === null ? null : `${entry.id}|${entry.kind}|${entry.baseUrl}|${entry.enabled}`;
  };
  const [models, { refetch: refetchModels }] = createResource(activeKey, () =>
    active() === null ? Promise.resolve<ModelChoice[]>([]) : activeModels(),
  );

  const refresh = async () => {
    setProviders(await listProviders());
  };

  onMount(() => {
    void (async () => {
      const [list, catalog] = await Promise.all([listProviders(), providerPresets()]);
      setProviders(list);
      setPresets(catalog);
      setReady(true);
    })();
  });

  /** Bọc một hành động đi sau cú bấm: lỗi hiện lên chứ không rơi vào console. */
  const act = async (what: string, run: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await run();
      await refresh();
    } catch (err) {
      setError(`${what}: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const submit = async (input: ProviderInput) => {
    setBusy(true);
    setFormError(null);
    try {
      await saveProvider(input);
      await refresh();
      setSheet({ kind: "none" });
      // Cấu hình vừa đổi có thể trỏ sang một máy chủ khác hẳn, nên danh sách mô hình cũ
      // không còn nói về cùng một thứ nữa.
      void refetchModels();
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  // Bóc tách kiểu hợp nhất một lần ở đây thay vì lồng hai `<Show>` ở chỗ dùng: mỗi lớp
  // `<Show>` thêm vào chỉ để thu hẹp kiểu là một lớp nữa che mất cái đang được vẽ.
  const formSheet = () => {
    const current = sheet();
    return current.kind === "form" ? current : null;
  };
  const deleteTarget = () => {
    const current = sheet();
    return current.kind === "delete" ? current.provider : null;
  };

  const chosen = (): ModelChoice | null =>
    (models() ?? []).find((entry) => entry.id === active()?.model) ?? null;

  return (
    <div class="flex flex-col gap-2xl">
      <SectionHead
        title="Nhà cung cấp mô hình"
        desc="Một provider hoạt động tại một thời điểm. Provider chạy tại chỗ giữ mọi thứ trong máy này."
        actions={() => (
          <Button
            label="Thêm provider"
            icon="plus"
            onClick={() => setSheet({ kind: "presets" })}
          />
        )}
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
            <div class="flex flex-col items-start gap-md rounded-card border border-dashed border-line bg-surface-soft px-(--card-pad-x) py-2xl">
              <p class="m-0 max-w-[52ch] text-xs text-muted">
                Chưa có nhà cung cấp nào. Không có provider thì trợ lý không gọi được mô
                hình nào cả — thêm một cái chạy tại chỗ là xong, và mã nguồn của bạn không
                đi đâu hết.
              </p>
              <Button label="Chọn từ danh mục" icon="model" onClick={() => setSheet({ kind: "presets" })} />
            </div>
          }
        >
          <ActiveNotice provider={active()} model={chosen()} loading={models.loading} />

          <ul class="m-0 flex list-none flex-col gap-sm p-0">
            {/* Keyed theo id: danh sách được nạp lại sau mỗi thao tác, và với `<For>` thì
                mọi hàng bị dựng lại — công tắc đang giữ tiêu điểm bàn phím rơi về body
                ngay giữa lúc người dùng vừa gạt nó. */}
            <Key each={providers()} by="id">
              {(entry) => (
                <ProviderRow
                  provider={entry()}
                  busy={busy()}
                  models={models() ?? []}
                  modelsLoading={models.loading}
                  onActivate={() => void act("Không đặt được provider hoạt động", () => setActiveProvider(entry().id))}
                  onToggle={(next) =>
                    void act("Không đổi được trạng thái", () =>
                      saveProvider({ ...inputOf(entry()), enabled: next }).then(() => undefined),
                    )
                  }
                  onPickModel={(model) =>
                    void act("Không chọn được mô hình", () => setProviderModel(entry().id, model))
                  }
                  onRefreshModels={() => void refetchModels()}
                  onEdit={() => {
                    setFormError(null);
                    setSheet({ kind: "form", provider: entry(), preset: null });
                  }}
                  onDelete={() => setSheet({ kind: "delete", provider: entry() })}
                />
              )}
            </Key>
          </ul>
        </Show>
      </Show>

      <Show when={sheet().kind === "presets"}>
        <PresetPicker
          presets={presets()}
          onPick={(preset) => {
            setFormError(null);
            setSheet({ kind: "form", provider: null, preset });
          }}
          onManual={() => {
            setFormError(null);
            setSheet({ kind: "form", provider: null, preset: null });
          }}
          onClose={() => setSheet({ kind: "none" })}
        />
      </Show>

      <Show when={formSheet()} keyed>
        {(open) => (
          <ProviderForm
            provider={open.provider}
            preset={
              open.preset === null
                ? null
                : {
                    name: open.preset.name,
                    kind: open.preset.kind,
                    baseUrl: open.preset.baseUrl,
                    // `defaultModel` chỉ là gợi ý điền sẵn; danh sách có thẩm quyền đến
                    // từ máy chủ sau khi có base URL và khoá.
                    model: open.preset.defaultModel,
                  }
            }
            busy={busy()}
            error={formError()}
            onSubmit={(input) => void submit(input)}
            onClose={() => setSheet({ kind: "none" })}
          />
        )}
      </Show>

      <Show when={deleteTarget()} keyed>
        {(target) => (
          <ConfirmDialog
            title={`Xoá ${target.name}?`}
            body="Cấu hình và khoá API của provider này bị xoá khỏi máy. Thao tác không hoàn tác được."
            detail={target.baseUrl}
            confirmLabel="Xoá provider"
            busy={busy()}
            onConfirm={() =>
              void act("Không xoá được provider", async () => {
                await removeProvider(target.id);
                setSheet({ kind: "none" });
              })
            }
            onClose={() => setSheet({ kind: "none" })}
          />
        )}
      </Show>
    </div>
  );
}

/**
 * Lời cảnh báo đứng trên đầu danh sách.
 *
 * Một mô hình `tools: false` là kiểu hỏng tệ nhất mà màn hình này có thể để lọt: trợ lý
 * vẫn trả lời trôi chảy, chỉ là không bao giờ đọc hay sửa được gì. Không có câu này thì
 * người dùng kết luận "agent này dở", chứ không kết luận "mình chọn nhầm mô hình".
 */
function ActiveNotice(props: { provider: Provider | null; model: ModelChoice | null; loading: boolean }) {
  return (
    <>
      <Show when={props.provider === null}>
        <Banner tone="warn" icon="warn" title="Chưa có provider nào đang hoạt động">
          Trợ lý chưa gọi được mô hình nào. Bấm "Dùng cái này" ở một hàng bên dưới.
        </Banner>
      </Show>

      <Show when={props.provider !== null && props.provider?.enabled === false}>
        <Banner tone="warn" icon="warn" title="Provider đang hoạt động lại đang bị tắt">
          Bật nó lên, hoặc chọn một provider khác làm cái đang dùng.
        </Banner>
      </Show>

      <Show when={props.provider !== null && props.provider?.model === null && !props.loading}>
        <Banner tone="warn" icon="warn" title="Chưa chọn mô hình">
          Chọn một mô hình trong danh sách của provider đang dùng.
        </Banner>
      </Show>

      <Show when={props.model !== null && props.model?.tools === false}>
        <Banner tone="danger" icon="warn" title="Mô hình đang chọn không gọi được tool">
          <code class="font-mono">{props.model?.id}</code> vẫn trả lời được, nhưng nó không
          đọc tệp, không sửa mã và không chạy lệnh — mọi câu trả lời sẽ là phỏng đoán từ
          trí nhớ. Chọn một mô hình có tool nếu bạn cần nó làm việc trong dự án.
        </Banner>
      </Show>
    </>
  );
}

function ProviderRow(props: {
  provider: Provider;
  busy: boolean;
  models: ModelChoice[];
  modelsLoading: boolean;
  onActivate: () => void;
  onToggle: (next: boolean) => void;
  onPickModel: (model: string) => void;
  onRefreshModels: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  return (
    <li>
      <div
        class="flex flex-col gap-sm rounded-card border bg-surface px-(--card-pad-x) py-(--card-pad-y) transition-colors duration-[var(--dur-fast)]"
        classList={{
          "border-accent": props.provider.active,
          "border-line": !props.provider.active,
          // Tắt thì cả hàng nhạt đi, nhưng vẫn đọc được: một hàng mờ tới mức không đọc
          // được là một hàng người dùng không bật lại được vì không biết nó là cái gì.
          "opacity-70": !props.provider.enabled,
        }}
      >
        <div class="flex flex-wrap items-center gap-sm">
          <span
            aria-hidden="true"
            class="grid size-8 shrink-0 place-items-center rounded-panel"
            classList={{
              "bg-accent-soft text-accent-ink": props.provider.onDevice,
              "bg-[var(--overlay-faint)] text-muted": !props.provider.onDevice,
            }}
          >
            <Icon name={props.provider.onDevice ? "plug" : "cloud"} size={16} />
          </span>

          <div class="flex min-w-0 flex-1 flex-col gap-3xs">
            <div class="flex min-w-0 flex-wrap items-center gap-2xs">
              <span class="min-w-0 truncate text-sm font-semibold text-ink">
                {props.provider.name}
              </span>

              <Show when={props.provider.active}>
                <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-accent px-2xs py-3xs text-2xs font-medium text-on-accent">
                  <Icon name="check" size={10} />
                  Đang dùng
                </span>
              </Show>

              {/* Huy hiệu quyền riêng tư. Nó khẳng định dữ liệu không rời khỏi máy, nên nó
                  chỉ được vẽ khi lõi nói `onDevice === true` — không suy đoán lại từ URL
                  ở phía này, vì một lời hứa đoán sai là lời hứa tệ nhất trong ứng dụng. */}
              <Show when={props.provider.onDevice}>
                <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill border border-accent bg-accent-soft px-2xs py-3xs text-2xs font-medium text-accent-ink">
                  <Icon name="plug" size={10} />
                  Chạy trên máy này
                </span>
              </Show>

              <span class="inline-flex shrink-0 items-center rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
                {props.provider.kind === "ollama" ? "Ollama" : "Tương thích OpenAI"}
              </span>

              <Show when={props.provider.hasKey}>
                <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
                  <Icon name="key" size={10} />
                  Có khoá
                </span>
              </Show>
            </div>

            <span class="min-w-0 truncate font-mono text-2xs text-faint" title={props.provider.baseUrl}>
              {props.provider.baseUrl}
            </span>
          </div>

          <div class="flex shrink-0 items-center gap-2xs">
            <Show when={!props.provider.active}>
              <Button
                label="Dùng cái này"
                variant="outline"
                disabled={props.busy || !props.provider.enabled}
                onClick={props.onActivate}
              />
            </Show>
            <Toggle
              label={`${props.provider.enabled ? "Tắt" : "Bật"} ${props.provider.name}`}
              checked={props.provider.enabled}
              busy={props.busy}
              onChange={props.onToggle}
            />
            <IconButton icon="pencil" label={`Sửa ${props.provider.name}`} size="sm" onClick={props.onEdit} />
            <IconButton
              icon="trash"
              label={`Xoá ${props.provider.name}`}
              size="sm"
              danger
              onClick={props.onDelete}
            />
          </div>
        </div>

        <Show when={props.provider.active}>
          <ModelPicker
            models={props.models}
            loading={props.modelsLoading}
            selected={props.provider.model}
            busy={props.busy}
            onPick={props.onPickModel}
            onRefresh={props.onRefreshModels}
          />
        </Show>
      </div>
    </li>
  );
}

/** Bộ chọn mô hình của provider đang hoạt động, dựng từ `ProviderProbe.models`. */
function ModelPicker(props: {
  models: ModelChoice[];
  loading: boolean;
  selected: string | null;
  busy: boolean;
  onPick: (model: string) => void;
  onRefresh: () => void;
}) {
  return (
    <div class="flex flex-col gap-2xs border-t border-line pt-sm">
      <div class="flex items-center gap-sm">
        <span class="flex-1 text-2xs text-faint">Mô hình</span>
        <IconButton icon="refresh" label="Nạp lại danh sách mô hình" size="sm" onClick={props.onRefresh} />
      </div>

      <Show
        when={!props.loading}
        fallback={
          <p class="m-0 text-2xs text-muted" role="status" aria-busy="true">
            Đang hỏi máy chủ xem có những mô hình nào…
          </p>
        }
      >
        <Show
          when={props.models.length > 0}
          fallback={
            <Banner tone="warn" icon="warn">
              Không đọc được danh sách mô hình từ máy chủ này. Mở "Sửa" rồi bấm "Thử kết
              nối" để xem máy chủ trả lời gì.
            </Banner>
          }
        >
          <div
            role="radiogroup"
            aria-label="Mô hình dùng cho provider đang hoạt động"
            class="flex max-h-56 flex-col gap-3xs overflow-y-auto"
          >
            <For each={props.models}>
              {(choice) => (
                <button
                  type="button"
                  role="radio"
                  aria-checked={props.selected === choice.id}
                  disabled={props.busy}
                  onClick={() => props.onPick(choice.id)}
                  class="flex items-center gap-sm rounded-panel border px-sm py-2xs text-left transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed"
                  classList={{
                    "border-accent bg-accent-soft": props.selected === choice.id,
                    "border-transparent hover:bg-[var(--overlay-hover)]": props.selected !== choice.id,
                  }}
                >
                  <span
                    aria-hidden="true"
                    class="size-3 shrink-0 rounded-pill border"
                    classList={{
                      "border-accent bg-accent": props.selected === choice.id,
                      "border-line-strong": props.selected !== choice.id,
                    }}
                  />
                  <span class="min-w-0 flex-1 truncate font-mono text-xs text-text">{choice.id}</span>

                  <Show when={choice.contextWindow}>
                    {(size) => (
                      <span class="shrink-0 text-2xs tabular-nums text-faint">
                        {Intl.NumberFormat("vi-VN").format(size())} token
                      </span>
                    )}
                  </Show>

                  {/* Không chặn lựa chọn — có người *muốn* một mô hình chỉ để trò chuyện.
                      Nhưng nói thẳng ra ở đúng hàng đó, chứ không giấu vào một chú giải. */}
                  <Show when={!choice.tools}>
                    <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-warn-soft px-2xs py-3xs text-2xs font-medium text-warn">
                      <Icon name="warn" size={10} />
                      Không gọi được tool
                    </span>
                  </Show>
                </button>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </div>
  );
}

/** Khung xương lúc nạp: giữ đúng chiều cao hàng để danh sách không giật khi hiện ra. */
function Skeleton() {
  return (
    <ul class="m-0 flex list-none flex-col gap-sm p-0" aria-hidden="true">
      <For each={[0, 1, 2]}>
        {() => (
          <li class="flex items-center gap-sm rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
            <span class="size-8 shrink-0 rounded-panel bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
            <span class="flex min-w-0 flex-1 flex-col gap-2xs">
              <span class="h-3 w-1/3 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
              <span class="h-2.5 w-1/2 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
            </span>
          </li>
        )}
      </For>
    </ul>
  );
}
