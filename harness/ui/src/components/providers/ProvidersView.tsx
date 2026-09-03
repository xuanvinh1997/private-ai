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
import EmbeddingView from "./EmbeddingView";
import RerankView from "./RerankView";
import { Banner, Button, InfoDot, Row, RowGroup, SectionHead, Select, Toggle } from "../settings/FormKit";
import ProviderForm from "./ProviderForm";

type Sheet =
  | { kind: "none" }
  | { kind: "form"; provider: Provider | null; preset: ProviderPreset | null }
  | { kind: "delete"; provider: Provider };

/**
 * Màn hình nhà cung cấp mô hình.
 *
 * Ba câu hỏi, theo đúng thứ tự người dùng hỏi chúng: *cái nào đang chạy*, *dữ liệu của
 * tôi có rời khỏi máy không*, và *mô hình này có làm được việc không*. Bố cục bám theo
 * thứ tự đó — vai đang giữ và huy hiệu "chạy trên máy này" nằm trên cùng một hàng với cái
 * tên, còn bộ chọn mô hình bung ra ngay dưới provider giữ vai hội thoại chứ không nằm ở
 * một khu riêng, vì mô hình chỉ có nghĩa khi đi kèm provider của nó.
 *
 * Một provider giữ **hai vai độc lập**: hội thoại và nhúng. Nó có thể giữ cả hai, một,
 * hoặc không vai nào, và ba trạng thái đó phải phân biệt được bằng mắt — nên "không vai
 * nào" cũng có nhãn riêng chứ không phải chỗ trống. Vai nhúng chỉ *hiện* ở đây; nó được
 * *chọn* ở màn hình mô hình nhúng, vì chọn nó là chọn nơi nhận toàn văn tài liệu.
 */
export default function ProvidersView() {
  const [providers, setProviders] = createSignal<Provider[]>([]);
  const [presets, setPresets] = createSignal<ProviderPreset[]>([]);
  const [ready, setReady] = createSignal(false);
  const [sheet, setSheet] = createSignal<Sheet>({ kind: "none" });
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [formError, setFormError] = createSignal<string | null>(null);

  /** Provider giữ vai **hội thoại**. Vai nhúng không đi qua màn hình này. */
  const active = () => providers().find((entry) => entry.activeChat) ?? null;

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

  /**
   * Mỗi lần danh sách máy chủ đổi thì con số này tăng, và mục nhúng ở cuối trang hỏi lại
   * lõi. Một con số chứ không phải một `Provider[]` truyền xuống: mục nhúng cần cả cấu
   * hình nhúng đang có hiệu lực nữa, và thứ đó chỉ lõi mới trả lời được.
   */
  const [stamp, setStamp] = createSignal(0);

  const refresh = async () => {
    setProviders(await listProviders());
    setStamp((n) => n + 1);
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
        icon="server"
        desc="Mỗi lúc chỉ một nhà cung cấp được dùng để trò chuyện."
        more="Mỗi lúc chỉ một nhà cung cấp được dùng để trò chuyện. Nhà cung cấp nhúng tài liệu chọn riêng ở mục bên dưới."
      />

      <Show when={error()}>
        {(message) => (
          <Banner tone="danger" icon="warn" role="alert" title="Không làm được">
            {message()}
          </Banner>
        )}
      </Show>

      <Show when={ready()} fallback={<Skeleton />}>
        <Show when={providers().length > 0}>
          <ActiveNotice provider={active()} model={chosen()} loading={models.loading} />

          <RowGroup>
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
                  onActivate={() => void act("Không đổi được nhà cung cấp đang dùng", () => setActiveProvider(entry().id))}
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
          </RowGroup>
        </Show>

        <Catalog
          presets={presets()}
          added={providers()}
          empty={providers().length === 0}
          onPick={(preset) => {
            setFormError(null);
            setSheet({ kind: "form", provider: null, preset });
          }}
          onManual={() => {
            setFormError(null);
            setSheet({ kind: "form", provider: null, preset: null });
          }}
        />

        {/* Vai nhúng, cùng trang, dưới cùng danh sách máy chủ.
            Nó **không** phải một tuỳ chọn nâng cao của việc chọn mô hình hội thoại — hai
            vai độc lập, và cấu hình đáng dùng nhất lại là cấu hình ghép chéo: nhúng bằng
            một mô hình nhỏ chạy tại chỗ, trò chuyện bằng một mô hình lớn từ xa. Nhưng cả
            hai đều được giao **từ đúng danh sách provider ở trên**, nên bắt người dùng
            sang một trang khác để giao vai thứ hai là bắt họ đi qua cùng một danh sách hai
            lần. Đứng dưới, sau danh mục, để thứ tự đọc trùng thứ tự làm: thêm máy chủ
            trước, giao vai sau.

            Chỉ hiện khi đã có provider: một ô chọn mô hình nhúng trên một máy không có máy
            chủ nào là một câu hỏi chưa ai trả lời được. */}
        <Show when={providers().length > 0}>
          <div class="border-t border-line pt-2xl">
            <EmbeddingView reloadKey={stamp()} />
            <RerankView />
          </div>
        </Show>
      </Show>

      <Show when={formSheet()} keyed>
        {(open) => (
          <ProviderForm
            provider={open.provider}
            preset={open.preset}
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
            body="Xoá vĩnh viễn cấu hình và khoá API khỏi máy."
            more="Cấu hình và khoá API của nhà cung cấp này bị xoá khỏi máy. Thao tác không hoàn tác được."
            detail={target.baseUrl}
            confirmLabel="Xoá nhà cung cấp"
            busy={busy()}
            onConfirm={() =>
              void act("Không xoá được nhà cung cấp", async () => {
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
 * Danh mục nhà cung cấp — **đứng thẳng trên trang**, không nằm sau một hộp thoại.
 *
 * Bản trước giấu nó sau nút "Thêm provider", nên trang này mở ra chỉ trả lời được câu hỏi
 * của người *đã có* provider. Người mở nó ra vì chưa có cái nào thì gặp một ô rỗng và một
 * cái nút, rồi một hộp thoại danh mục, rồi một hộp thoại biểu mẫu **chồng lên** hộp thoại
 * đó — ba lớp cho một việc mà đối thủ làm bằng một cú bấm trên một hàng.
 *
 * Chỗ gập là điểm chính, và nó **không** phải "năm mục đầu" tuỳ tiện: mở sẵn là nhóm
 * **chạy trên máy này**, còn nhóm gửi dữ liệu ra ngoài nằm sau một cú bấm. Trong một ứng
 * dụng mà cả kiến trúc dựng quanh việc dữ liệu không rời khỏi máy, thứ tự mặc định của
 * danh mục là một câu khẳng định chứ không phải một chi tiết sắp xếp.
 *
 * Nút "Nối" là `outline`, không phải nút chính: mười hàng, mười nút xanh thì không nút nào
 * còn là hành động chính nữa.
 */
function Catalog(props: {
  presets: ProviderPreset[];
  added: Provider[];
  /** Chưa có provider nào — khi ấy danh mục là nội dung chính của trang, không phải phần đuôi. */
  empty: boolean;
  onPick: (preset: ProviderPreset) => void;
  onManual: () => void;
}) {
  const [more, setMore] = createSignal(false);
  const local = () => props.presets.filter((entry) => entry.onDevice);
  const remote = () => props.presets.filter((entry) => !entry.onDevice);

  /** Đã có một provider trỏ vào đúng địa chỉ ấy chưa. Dấu `/` cuối không tính là khác. */
  const already = (preset: ProviderPreset) => {
    const bare = (url: string) => url.trim().replace(/\/+$/, "").toLowerCase();
    return props.added.some((entry) => bare(entry.baseUrl) === bare(preset.baseUrl));
  };

  /**
   * Một hàng danh mục: biểu tượng, tên, nút. **Không có dòng mô tả.**
   *
   * `hint` của mỗi mục dài mười tám chữ — nó nói cả điều kiện chạy được ("phải bật máy chủ
   * cục bộ trong tab Developer") chứ không chỉ nói mục này là gì. Trải bốn câu như thế
   * xuống bốn hàng thì danh mục thành một trang chữ, mà người đang lướt danh mục thì chưa
   * cần điều kiện của mục họ chưa chọn. Nên nó vào `InfoDot`, và nó hiện đầy đủ ở hộp
   * thoại — đúng lúc người dùng vừa bấm "Nối" và sắp cần tới nó.
   */
  const entry = (preset: ProviderPreset) => (
    <Row
      label={preset.name}
      icon={preset.onDevice ? "plug" : "cloud"}
      more={`${preset.hint} Địa chỉ mặc định ${preset.baseUrl}.${preset.needsKey ? " Cần khoá API." : ""}`}
      control={() => (
        <>
          {/* "Đã thêm" không khoá cái nút: hai máy chủ Ollama ở hai cổng khác nhau là một
              cấu hình hợp lệ, và một hàng bị khoá thì không có gì nói ra vì sao. */}
          <Show when={already(preset)}>
            <span class="rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-faint">
              đã thêm
            </span>
          </Show>
          <Button label="Kết nối" variant="outline" icon="plus" onClick={() => props.onPick(preset)} />
        </>
      )}
    />
  );

  return (
    <section class="flex flex-col gap-sm">
      <h3 class="m-0 flex items-center gap-2xs text-xs font-semibold text-ink">
        {props.empty ? "Chọn một nhà cung cấp để bắt đầu" : "Thêm nhà cung cấp"}
        <InfoDot
          label="Về danh mục nhà cung cấp"
          text="Những mục đầu chạy ngay trên máy này: mã nguồn và câu hỏi của bạn không đi đâu cả. Các dịch vụ từ xa nằm sau nút xem thêm — chúng nhanh và mạnh hơn, nhưng mọi thứ bạn gửi đều rời khỏi máy. Máy chủ không có trong danh sách thì dùng mục Máy chủ khác."
        />
      </h3>

      <RowGroup>
        <For each={local()}>{entry}</For>

        {/* Tự khai báo là một hàng như mọi hàng khác, không phải một nút lạc ở chân hộp
            thoại: `llama-server`, một cổng nội bộ, một máy chủ tự dựng — đó là những thứ
            người dùng của ứng dụng này thật sự chạy, chứ không phải trường hợp ngoại lệ. */}
        <Row
          label="Máy chủ khác"
          icon="sparkle"
          more="Dùng cho máy chủ không có trong danh sách: llama.cpp tự dựng, một cổng trung chuyển nội bộ, hay một dịch vụ tương thích OpenAI khác. Bạn tự điền tên, loại API và địa chỉ."
          control={() => (
            <>
              <span class="rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-faint">
                tuỳ chỉnh
              </span>
              <Button label="Khai báo" variant="outline" icon="plus" onClick={props.onManual} />
            </>
          )}
        />

        <Show when={more()}>
          <For each={remote()}>{entry}</For>
        </Show>
      </RowGroup>

      <Show when={!more() && remote().length > 0}>
        <div>
          <Button
            label={`Xem thêm ${remote().length} dịch vụ từ xa`}
            variant="ghost"
            icon="cloud"
            onClick={() => setMore(true)}
          />
        </div>
      </Show>
    </section>
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
        <Banner
          tone="warn"
          icon="warn"
          title="Chưa chọn nhà cung cấp để trò chuyện"
          more={'Trợ lý chưa gọi được mô hình nào. Bấm "Dùng để trò chuyện" ở một hàng bên dưới.'}
        >
          Bấm "Dùng để trò chuyện" ở một hàng bên dưới.
        </Banner>
      </Show>

      <Show when={props.provider !== null && props.provider?.enabled === false}>
        <Banner tone="warn" icon="warn" title="Nhà cung cấp đang dùng để trò chuyện lại bị tắt">
          Bật nó lên, hoặc giao vai cho provider khác.
        </Banner>
      </Show>

      <Show when={props.provider !== null && props.provider?.model === null && !props.loading}>
        <Banner tone="warn" icon="warn" title="Chưa chọn mô hình hội thoại">
          Chọn mô hình ở hàng provider đang giữ vai.
        </Banner>
      </Show>

      <Show when={props.model !== null && props.model?.tools === false}>
        <Banner
          tone="danger"
          icon="warn"
          title="Mô hình đang chọn không gọi được tool"
          more={`${props.model?.id ?? "Mô hình này"} vẫn trả lời được, nhưng nó không đọc tệp, không sửa mã và không chạy lệnh — mọi câu trả lời sẽ là phỏng đoán từ trí nhớ. Chọn một mô hình có tool nếu bạn cần nó làm việc trong dự án.`}
        >
          <code class="font-mono">{props.model?.id}</code> không đọc tệp, không sửa mã,
          không chạy lệnh.
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
  /** Tên loại API, hoặc `null` khi nó chỉ nhắc lại tên provider. */
  const kindLabel = () => {
    const label =
      props.provider.kind === "ollama"
        ? "Ollama"
        : props.provider.kind === "lmstudio"
          ? "LM Studio"
          : "Tương thích OpenAI";
    const bare = (value: string) => value.trim().toLowerCase();
    return bare(props.provider.name).includes(bare(label)) ? null : label;
  };

  return (
    <Row
      label={props.provider.name}
      /* Icon dẫn hàng **mang màu**, và nó nói nốt phần mà huy hiệu "Chạy trên máy này"
         từng nói bằng chữ. Chữ ấy lặp xuống mọi hàng tại chỗ — mà đa số hàng của ứng dụng
         này là hàng tại chỗ — nên nó là một đoạn văn bản không ai đọc tới lần thứ hai. Ổ
         cắm trong ô nhấn xanh đọc được trong một nhịp mắt, và `title` giữ nguyên câu đầy
         đủ cho người cần nó. Cờ lấy thẳng từ lõi, không đoán lại từ URL: một lời hứa đoán
         sai là lời hứa tệ nhất trong ứng dụng. */
      lead={() => (
        <span
          class="grid size-7 shrink-0 place-items-center rounded-panel"
          classList={{
            "bg-accent-soft text-accent-ink": props.provider.onDevice,
            "bg-[var(--overlay-faint)] text-muted": !props.provider.onDevice,
          }}
          title={
            props.provider.onDevice
              ? "Chạy trên máy này — dữ liệu không rời khỏi đây."
              : "Máy chủ từ xa — mọi thứ bạn gửi đều rời khỏi máy này."
          }
        >
          <Icon name={props.provider.onDevice ? "plug" : "cloud"} size={14} />
        </span>
      )}
      dim={!props.provider.enabled}
      control={() => (
        <>
          <Show when={!props.provider.activeChat}>
            <Button
              label="Dùng để trò chuyện"
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
        </>
      )}
      below={() => (
        <>
          <div class="flex min-w-0 flex-wrap items-center gap-2xs">
            <Roles provider={props.provider} />

            {/* Chỉ **chiều đi ra ngoài** mới có huy hiệu chữ. Nhãn dán lên mọi hàng thì
                không còn là cảnh báo; nhãn dán lên đúng ngoại lệ thì mới là. Chiều an
                toàn đã nằm trong ô ổ cắm xanh ở đầu hàng. */}
            <Show when={!props.provider.onDevice}>
              <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-warn-soft px-2xs py-3xs text-2xs font-medium text-warn">
                <Icon name="cloud" size={10} />
                Gửi ra ngoài
              </span>
            </Show>

            {/* Loại API chỉ hiện khi nó **nói thêm** được gì. "Ollama" dán dưới một hàng
                tên là Ollama là một chữ không mang tin; còn "Tương thích OpenAI" dưới một
                hàng tên LM Studio thì đúng là thứ giải thích vì sao hàng ấy không đọc được
                danh sách mô hình. */}
            <Show when={kindLabel() !== null}>
              <span class="inline-flex shrink-0 items-center rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs text-muted">
                {kindLabel()}
              </span>
            </Show>

            {/* Chỉ ở hàng **không** giữ vai hội thoại: hàng đang hoạt động đã có cả một bộ
                chọn mô hình bên dưới, và nhắc lại cùng một tên hai lần cách nhau một dòng
                là mời người đọc đi tìm xem hai chỗ ấy có khác nhau không. */}
            <Show when={!props.provider.activeChat}>
              <span
                class="inline-flex min-w-0 shrink items-center gap-3xs rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-2xs"
                classList={{
                  "text-muted": props.provider.model !== null,
                  "text-faint": props.provider.model === null,
                }}
                title={props.provider.model ?? undefined}
              >
                <Icon name="model" size={10} />
                <span class="min-w-0 truncate font-mono">
                  {props.provider.model ?? "chưa chọn mô hình"}
                </span>
              </span>
            </Show>

            {/* Chìa khoá đứng một mình. Hai chữ "Có khoá" lặp xuống mọi hàng từ xa, và
                cái ổ khoá thì không lặp lại gì cả. */}
            <Show when={props.provider.hasKey}>
              <span
                class="inline-flex shrink-0 items-center rounded-pill bg-[var(--overlay-faint)] px-2xs py-3xs text-muted"
                title="Đã lưu khoá API cho nhà cung cấp này"
                aria-label="Đã lưu khoá API"
              >
                <Icon name="key" size={11} />
              </span>
            </Show>

            <span class="min-w-0 truncate font-mono text-2xs text-faint" title={props.provider.baseUrl}>
              {props.provider.baseUrl}
            </span>
          </div>

          <Show when={props.provider.activeChat}>
            <ModelPicker
              models={props.models}
              loading={props.modelsLoading}
              selected={props.provider.model}
              busy={props.busy}
              onPick={props.onPickModel}
              onRefresh={props.onRefreshModels}
            />
          </Show>
        </>
      )}
    />
  );
}

/**
 * Vai mà provider này đang giữ: cả hai, một, hoặc không vai nào.
 *
 * "Không vai nào" cũng có nhãn riêng chứ không phải một khoảng trống. Chỗ trống đọc ra là
 * "chưa nạp xong" hoặc "hàng này bị lỗi vẽ", còn ba trạng thái ở đây phải phân biệt được
 * ngay bằng mắt — đó là toàn bộ thông tin mà một hàng provider mang.
 *
 * Vai nhúng hiện ở đây nhưng **không đổi được** ở đây: nó là một quyết định về nơi nhận
 * toàn văn tài liệu, nên nó đứng cùng chỗ với câu giải thích của nó ở màn hình mô hình
 * nhúng, chứ không nấp sau một cái nhãn nhỏ giữa danh sách.
 */
function Roles(props: { provider: Provider }) {
  const none = () => !props.provider.activeChat && !props.provider.activeEmbedding;
  return (
    <>
      <Show when={props.provider.activeChat}>
        <span class="inline-flex shrink-0 items-center gap-3xs rounded-pill bg-accent px-2xs py-3xs text-2xs font-medium text-on-accent">
          <Icon name="chat" size={10} />
          Hội thoại
        </span>
      </Show>

      <Show when={props.provider.activeEmbedding}>
        <span
          class="inline-flex shrink-0 items-center gap-3xs rounded-pill border border-accent px-2xs py-3xs text-2xs font-medium text-accent-ink"
          title={`Đang nhúng tài liệu bằng ${props.provider.embeddingModel ?? "mô hình chưa chọn"}`}
        >
          <Icon name="library" size={10} />
          Nhúng
        </span>
      </Show>

      <Show when={none()}>
        <span class="inline-flex shrink-0 items-center rounded-pill border border-dashed border-line-strong px-2xs py-3xs text-2xs text-faint">
          Chưa giao vai
        </span>
      </Show>
    </>
  );
}

/**
 * Bộ chọn mô hình hội thoại của provider đang giữ vai đó.
 *
 * `<select>` thật thay cho danh sách nút cũ: một máy chủ Ollama đầy đủ trả về hàng chục
 * mô hình, và một danh sách dài như thế phải tự cuộn — tức là một vùng cuộn nữa nằm trong
 * vùng cuộn của trang cài đặt, đúng thứ mà hình dạng hàng gọn tồn tại để tránh.
 *
 * Cảnh báo `tools: false` đi theo *tên mô hình* trong từng dòng của danh sách, và lặp lại
 * thành một băng riêng ở đầu trang cho mô hình **đang chọn** — vì đó là mô hình duy nhất
 * mà cờ đó thật sự gây hậu quả.
 */
function ModelPicker(props: {
  models: ModelChoice[];
  loading: boolean;
  selected: string | null;
  busy: boolean;
  onPick: (model: string) => void;
  onRefresh: () => void;
}) {
  const options = () => {
    const list = props.models.map((choice) => ({
      id: choice.id,
      label: `${choice.id}${choice.tools ? "" : " — không gọi được tool"}${
        choice.contextWindow === null
          ? ""
          : ` · ${Intl.NumberFormat("vi-VN").format(choice.contextWindow)} token`
      }`,
    }));
    // Chưa chọn gì thì phải có một mục rỗng, nếu không trình duyệt hiện mục đầu tiên và
    // màn hình nói dối rằng mô hình đó đang được dùng.
    return props.selected === null ? [{ id: "", label: "— chưa chọn —" }, ...list] : list;
  };

  return (
    <div class="flex flex-wrap items-center gap-sm border-t border-line pt-sm">
      {/* Biểu tượng thay cho dòng chữ "Mô hình hội thoại".
          Cái nhãn ấy không nói gì mà hàng chưa nói: hàng này đeo huy hiệu "Hội thoại", và
          thứ nằm trong ô chọn là một tên mô hình. Nó chỉ có mặt ở đúng một hàng trong danh
          sách, nên nó cũng không phải một tiêu đề cột — nó là mười lăm ký tự chiếm chỗ của
          chính cái tên mô hình. Tên đầy đủ vẫn tới được trình đọc màn hình qua `aria-label`
          của `<select>`. */}
      <span class="shrink-0 text-faint" title="Mô hình dùng để trò chuyện">
        <Icon name="model" size={13} />
      </span>

      <Show
        when={!props.loading}
        fallback={
          <span class="text-2xs text-muted" role="status" aria-busy="true">
            Đang lấy danh sách mô hình…
          </span>
        }
      >
        <Show
          when={props.models.length > 0}
          fallback={
            <span class="flex min-w-0 flex-1 items-center gap-2xs text-xs text-warn">
              Không đọc được danh sách mô hình từ máy chủ này.
              <InfoDot
                label="Xem thêm về danh sách mô hình"
                text={'Mở "Sửa" — hộp thoại tự hỏi lại máy chủ và nói ra nó trả lời gì.'}
              />
            </span>
          }
        >
          <Select
            label="Mô hình dùng để trò chuyện"
            mono
            value={props.selected ?? ""}
            options={options()}
            disabled={props.busy}
            onPick={props.onPick}
          />
        </Show>
      </Show>

      <IconButton icon="refresh" label="Nạp lại danh sách mô hình" size="sm" onClick={props.onRefresh} />
    </div>
  );
}

/** Khung xương lúc nạp: giữ đúng chiều cao hàng để danh sách không giật khi hiện ra. */
function Skeleton() {
  return (
    <div
      class="flex flex-col divide-y divide-line rounded-card border border-line bg-surface"
      aria-hidden="true"
    >
      <For each={[0, 1, 2]}>
        {() => (
          <div class="flex flex-col gap-2xs px-(--card-pad-x) py-sm">
            <span class="h-3 w-1/4 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse" />
            <span class="h-2.5 w-1/2 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
          </div>
        )}
      </For>
    </div>
  );
}
