import { createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import Icon from "./Icon";
import McpView from "./mcp/McpView";
import ProvidersView from "./providers/ProvidersView";
import GeneralPage from "./settings/GeneralPage";
import HooksPage from "./settings/HooksPage";
import PermissionsPage from "./settings/PermissionsPage";
import ShortcutsPage from "./settings/ShortcutsPage";
import { NAV, pageMeta, timTrongCaiDat, type SettingsPage } from "./settings/muc-luc";

export type { SettingsPage } from "./settings/muc-luc";

/**
 * Màn hình cài đặt: một **chế độ chiếm trọn cửa sổ**, không phải một tab trong khu làm việc.
 *
 * Bản trước là bốn tab ngang nằm trong khung hội thoại, và bốn tab ngang không mọc thêm
 * được: tab thứ năm làm hàng tab tự xuống dòng, tab thứ bảy làm nó thành hai hàng chữ
 * nhỏ mà mắt phải quét ngang trước mỗi lần đi tới đâu đó. Một cột dọc chia nhóm thì mọc
 * mãi vẫn đọc được, và đó là lý do mọi ứng dụng desktop có nhiều hơn ba trang cài đặt đều
 * chọn nó.
 *
 * Chiếm trọn cửa sổ chứ không nằm cạnh thanh bên vì cài đặt là một **chỗ khác**, không
 * phải một màn hình khác: người ta vào đó để sửa một thứ rồi đi ra, và trong lúc đó danh
 * sách phiên với ô soạn tin chỉ là chỗ để bấm nhầm. Đổi lại thì lối ra phải rõ ràng tuyệt
 * đối — nên có cả nút `← Về ứng dụng` ở đúng góc trên trái lẫn phím `Esc`.
 *
 * Trang đang mở do `App` giữ chứ không do màn hình này giữ, và hợp đồng đó giữ nguyên từ
 * bản trước: thanh bên có một lối đi thẳng tới trang MCP, và một trạng thái nội bộ ở đây
 * sẽ nuốt mất cú bấm ấy mỗi khi màn hình đã mở sẵn.
 */
export default function SettingsView(props: {
  page: SettingsPage;
  onPage: (page: SettingsPage) => void;
  /** Lối ra. `Esc` và nút `← Về ứng dụng` gọi cùng một hàm này. */
  onClose: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const results = createMemo(() => timTrongCaiDat(query()));
  const searching = () => query().trim() !== "";
  const meta = () => pageMeta(props.page);

  const go = (page: SettingsPage) => {
    props.onPage(page);
    // Đi tới một trang là đã tìm xong. Giữ lại chuỗi tìm thì trang vừa mở bị chính kết
    // quả tìm che mất, và người dùng bấm một lần nữa vào đúng cái vừa bấm.
    setQuery("");
  };

  /**
   * `Esc`: xoá ô tìm nếu đang tìm, còn không thì ra khỏi cài đặt.
   *
   * Bắt ở `window` chứ không ở một phần tử nào: tiêu điểm có thể đang nằm trong bất kỳ ô
   * nhập nào của bảy trang. Bỏ qua khi sự kiện đã được xử lý — `useFocusTrap` của hộp
   * thoại bắt `Esc` ở pha capture và `preventDefault`, nên một `Esc` trong hộp thoại sửa
   * provider đóng hộp thoại chứ không quăng người dùng ra khỏi cả màn hình cài đặt.
   */
  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (searching()) {
        setQuery("");
        return;
      }
      props.onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  return (
    // `fixed inset-0` chứ không phải một ô trong lưới của `App`: màn hình này *là* cửa sổ
    // trong lúc nó mở. Cửa sổ mở ở chế độ "Overlay" nên ba nút giao thông của macOS nằm
    // đè lên góc trên trái — cả hai cột vì thế đều chừa sẵn một dải `--titlebar-h`.
    <div class="fixed inset-0 z-30 flex bg-bg">
      <aside
        aria-label="Cài đặt"
        class="flex w-(--sidebar-w) shrink-0 flex-col border-r border-line bg-sidebar"
      >
        <div class="h-(--titlebar-h) shrink-0" data-tauri-drag-region />

        <div class="shrink-0 px-sm pb-2xs">
          <button
            type="button"
            onClick={() => props.onClose()}
            class="flex w-full items-center gap-2xs rounded-panel px-2xs py-2xs text-sm font-medium text-ink transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
          >
            <Icon name="chevron-right" size={14} class="rotate-180" />
            Về ứng dụng
          </button>
        </div>

        <div class="shrink-0 px-sm pb-sm">
          <div class="relative">
            {/* Kính lúp nằm *trong* ô, không phải một nút cạnh ô: ô tìm này luôn hiện,
                không có trạng thái đóng/mở nào để bấm. */}
            <span
              aria-hidden="true"
              class="pointer-events-none absolute top-1/2 left-sm -translate-y-1/2 text-faint"
            >
              <Icon name="search" size={13} />
            </span>
            <input
              type="search"
              value={query()}
              onInput={(event) => setQuery(event.currentTarget.value)}
              placeholder="Tìm trong cài đặt…"
              aria-label="Tìm trong cài đặt"
              spellcheck={false}
              autocapitalize="off"
              autocomplete="off"
              class="h-(--control-h) w-full rounded-btn border border-line bg-surface pr-sm pl-2xl text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
            />
          </div>
        </div>

        <nav aria-label="Mục cài đặt" class="min-h-0 flex-1 overflow-y-auto px-sm pb-lg">
          <For each={NAV}>
            {(group) => (
              <div class="mb-md flex flex-col gap-3xs last:mb-0">
                <Show when={group.title}>
                  {(title) => (
                    // Tiêu đề nhóm là chữ, không phải nút: nó không đi tới đâu cả. Vẽ nó
                    // giống một hàng bấm được là mời người dùng bấm vào một chỗ chết.
                    <h2 class="m-0 px-2xs pt-2xs pb-3xs text-2xs font-medium text-faint">
                      {title()}
                    </h2>
                  )}
                </Show>
                <For each={group.pages}>
                  {(item) => (
                    <button
                      type="button"
                      onClick={() => go(item.id)}
                      aria-current={props.page === item.id ? "page" : undefined}
                      class="flex items-center gap-sm rounded-panel px-2xs py-2xs text-left text-xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink aria-[current=page]:bg-surface-hover aria-[current=page]:font-medium aria-[current=page]:text-ink"
                    >
                      <Icon name={item.icon} size={15} />
                      <span class="min-w-0 truncate">{item.label}</span>
                    </button>
                  )}
                </For>
              </div>
            )}
          </For>
        </nav>
      </aside>

      <div class="flex min-w-0 flex-1 flex-col">
        <div class="h-(--titlebar-h) shrink-0" data-tauri-drag-region />

        <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) pb-4xl">
          <div class="mx-auto flex w-full max-w-(--reading-measure) flex-col">
            <Show
              when={!searching()}
              fallback={
                <SearchPane query={query()} hits={results()} onGo={go} />
              }
            >
              {/* Tiêu đề trang cỡ `display` và một dải trống rộng quanh nó. Đây là thứ
                  duy nhất trên trang nói "bạn đang ở đâu", và ở một màn hình mà mọi trang
                  còn lại trông hệt nhau — cùng một dãy hàng bo góc — thì nó phải to đến
                  mức đọc được bằng mắt ngoại vi. */}
              <header class="flex flex-col gap-2xs pt-3xl pb-2xl">
                <h1 class="m-0 text-display font-semibold tracking-tight text-ink">
                  {meta().label}
                </h1>
                <Show when={meta().desc}>
                  {(desc) => <p class="m-0 max-w-[60ch] text-sm text-muted">{desc()}</p>}
                </Show>
              </header>

              <Switch>
                <Match when={props.page === "chung"}>
                  <GeneralPage />
                </Match>
                <Match when={props.page === "phim-tat"}>
                  <ShortcutsPage />
                </Match>
                {/* Một trang cho cả hai vai. `ProvidersView` vẽ danh sách máy chủ rồi
                    gọi `EmbeddingView` làm mục cuối của chính nó: hai vai được giao từ
                    cùng một danh sách provider, nên tách ra hai trang là bắt người dùng đi
                    qua danh sách ấy hai lần. */}
                <Match when={props.page === "provider"}>
                  <ProvidersView />
                </Match>
                <Match when={props.page === "mcp"}>
                  <McpView />
                </Match>
                <Match when={props.page === "hook"}>
                  <HooksPage />
                </Match>
                <Match when={props.page === "quyen"}>
                  <PermissionsPage />
                </Match>
              </Switch>
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * Kết quả tìm.
 *
 * Mỗi dòng mang **tên trang chứa nó**, vì kết quả tìm trong một màn hình cài đặt không
 * phải là câu trả lời — nó là một lối đi. Không nói ra trang thì người dùng bấm vào rồi
 * mới biết mình vừa bị đưa đi đâu, và lần sau họ vẫn phải gõ lại đúng chuỗi ấy thay vì
 * nhớ được chỗ.
 *
 * Không có kết quả thì nói ra bằng chữ, kèm chính chuỗi đã gõ. Một danh sách rỗng không
 * kèm lời nào đọc ra là màn hình chưa vẽ xong.
 */
function SearchPane(props: {
  query: string;
  hits: ReturnType<typeof timTrongCaiDat>;
  onGo: (page: SettingsPage) => void;
}) {
  return (
    <>
      <header class="flex flex-col gap-2xs pt-3xl pb-2xl">
        <h1 class="m-0 text-display font-semibold tracking-tight text-ink">Kết quả tìm</h1>
        <p class="m-0 text-sm text-muted" role="status" aria-live="polite">
          {props.hits.length === 0
            ? `Không có mục cài đặt nào khớp “${props.query.trim()}”.`
            : `${props.hits.length} mục khớp “${props.query.trim()}”.`}
        </p>
      </header>

      <Show
        when={props.hits.length > 0}
        fallback={
          <p class="m-0 max-w-[60ch] text-xs text-muted">
            Ô tìm chỉ thấy mục cài đặt, không thấy dữ liệu trên máy bạn.
          </p>
        }
      >
        <ul class="m-0 flex list-none flex-col gap-2xs p-0">
          <For each={props.hits}>
            {(hit) => (
              <li>
                <button
                  type="button"
                  onClick={() => props.onGo(hit.page)}
                  class="flex w-full flex-col gap-3xs rounded-card border border-line bg-surface px-(--card-pad-x) py-sm text-left transition-colors duration-[var(--dur-fast)] hover:bg-surface-hover"
                >
                  <span class="flex flex-wrap items-baseline gap-2xs">
                    <span class="text-xs font-medium text-ink">{hit.label}</span>
                    <span class="flex items-center gap-3xs text-2xs text-faint">
                      <Icon name="chevron-right" size={10} />
                      {pageMeta(hit.page).label}
                    </span>
                  </span>
                  <span class="text-2xs text-muted">{hit.desc}</span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </>
  );
}
