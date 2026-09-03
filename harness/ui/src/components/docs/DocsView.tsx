import { createEffect, createSignal, For, on, Show } from "solid-js";
import { isDemo } from "../../lib/demo";
import {
  addDocuments,
  libraryStats,
  listDocuments,
  reprocessLibrary,
  syncLibrary,
  pickDocuments,
  removeDocument,
} from "../../lib/docs";
import { demoDocuments, demoIngestFrames, demoLibraryStats } from "../../lib/fixtures/docs";
import type { DocumentView, IngestProgress, LibraryStats } from "../../lib/protocol";
import Icon from "../Icon";
import { InfoDot } from "../settings/FormKit";
import ConfirmDialog from "../projects/ConfirmDialog";
import { Button } from "../projects/DialogShell";
import DocumentTable from "./DocumentTable";
import DropZone from "./DropZone";
import SearchProbe from "./SearchProbe";

interface Failure {
  path: string;
  error: string;
}

/**
 * Màn hình thư viện tài liệu. Chỉ có nghĩa khi dự án đang mở là loại `docs`.
 *
 * Ba quyết định định hình cả màn hình này, cả ba đều về việc **nói đúng chuyện đang xảy
 * ra** thay vì nói cho gọn:
 *
 * **Một tệp hỏng không làm hỏng cả lô.** Nạp hai mươi tệp mà một tệp là bản quét không
 * rút được chữ thì mười chín tệp kia vẫn vào thư viện. Một dòng "nạp thất bại" cho cả lô
 * sẽ khiến người dùng nạp lại mười chín tệp đã nằm sẵn trong đó — nên tệp hỏng được gom
 * riêng, kèm lý do, cạnh con số nói rõ bao nhiêu tệp đã vào.
 *
 * **`semanticReady === false` không phải là hỏng.** Tìm bằng từ khoá vẫn chạy, và thư
 * viện vẫn trả lời được ngay. Dải trạng thái nói cả hai vế — lý do *và* việc thư viện
 * vẫn dùng được — vì nói mỗi vế đầu là đuổi người dùng đi chờ một thứ họ không cần chờ.
 *
 * **Trích đoạn là chữ của người ngoài.** Xem `SearchProbe`.
 *
 * **Và có một nút cho việc mà máy đáng ra tự làm.** Lượt quét lúc mở màn hình là lượt
 * tăng dần — tệp không đổi thì không đọc lại, tệp từng đọc hỏng cũng không. Một tệp hỏng
 * vì lý do đã qua vì thế nằm mãi ở đó: nó không đổi một byte nên không lượt quét nào chạm
 * lại vào nó, và người dùng không có câu nào để nói "thử lại đi". `Xử lý lại` là câu đó.
 */
export default function DocsView(props: {
  /** Đổi giá trị là đổi dự án: vứt sạch trạng thái cũ rồi nạp lại từ đầu. */
  resetKey: string;
  /** Tên thư viện để đặt tiêu đề; vắng thì dùng chữ chung. */
  name?: string;
}) {
  const [docs, setDocs] = createSignal<DocumentView[]>([]);
  const [stats, setStats] = createSignal<LibraryStats | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [ingest, setIngest] = createSignal<IngestProgress | null>(null);
  const [failures, setFailures] = createSignal<Failure[]>([]);
  const [added, setAdded] = createSignal(0);
  const [error, setError] = createSignal<string | null>(null);
  const [removing, setRemoving] = createSignal<DocumentView | null>(null);
  const [busy, setBusy] = createSignal(false);

  const load = async () => {
    setLoading(true);
    if (isDemo()) {
      setDocs(demoDocuments());
      setStats(demoLibraryStats());
      setLoading(false);
      return;
    }
    // Hiện ngay cái đã biết, rồi mới quét. Chờ quét xong mới vẽ gì cả là để người dùng
    // nhìn một màn hình trống trong lúc thư mục vài trăm tệp chạy qua.
    const [list, health] = await Promise.all([listDocuments(), libraryStats()]);
    setDocs(list);
    setStats(health);
    setLoading(false);

    // Rồi đồng bộ với thư mục. **Đây là chỗ trả lời câu "chọn folder mà không thấy tệp
    // nào"**: thư mục dự án là thư viện, nên mở màn hình ra là quét, không đợi ai bấm.
    // Lõi bỏ qua tệp không đổi, nên lần mở thứ hai gần như không tốn gì.
    try {
      const sau = await syncLibrary(note);
      setDocs(sau);
      setStats(await libraryStats());
    } catch (err) {
      // Quét hỏng không được xoá mất danh sách vừa hiện: những gì đã nạp lần trước vẫn
      // tìm được, và đó vẫn là một thư viện dùng được.
      setError(`Không quét được thư mục: ${err}`);
    } finally {
      setIngest(null);
    }
  };

  // Nạp lại khi đổi dự án. Không dùng `onMount` vì component sống qua nhiều dự án khác
  // nhau: giữ lại danh sách cũ là hiện tài liệu của thư viện vừa rời khỏi.
  createEffect(
    on(
      () => props.resetKey,
      () => {
        setDocs([]);
        setStats(null);
        setIngest(null);
        setFailures([]);
        setAdded(0);
        setError(null);
        void load();
      },
    ),
  );

  const note = (frame: IngestProgress) => {
    setIngest(frame);
    const reason = frame.error;
    if (reason === null) return;
    // Đợt nhúng bù hỏng **không** phải một tệp hỏng: mọi tệp đã vào thư viện, chỉ có máy
    // chủ nhúng là chưa trả lời. Đếm nó vào danh sách tệp hỏng là bảo người dùng đi sửa
    // tệp của họ trong khi thứ cần bật lại nằm ở chỗ khác.
    if (frame.stage === "embedding") {
      setError(reason);
      return;
    }
    setFailures((all) => [...all, { path: frame.path, error: reason }]);
  };

  async function runDemoIngest(paths: string[]): Promise<DocumentView[]> {
    for (const frame of demoIngestFrames(paths)) {
      await new Promise<void>((resolve) => setTimeout(resolve, 320));
      note(frame);
    }
    setStats(demoLibraryStats());
    return demoDocuments();
  }

  const addFiles = async (paths: string[]) => {
    if (paths.length === 0 || busy()) return;
    setBusy(true);
    setError(null);
    setFailures([]);
    setAdded(0);
    setIngest({ path: paths[0] ?? "", stage: "Đang chuẩn bị", done: 0, total: paths.length, finished: false, error: null });
    try {
      const next = isDemo() ? await runDemoIngest(paths) : await addDocuments(paths, note);
      setDocs(next);
      setAdded(paths.length - failures().length);
      if (!isDemo()) setStats(await libraryStats());
    } catch (err) {
      // Chỉ tới đây khi **cả lô** không chạy được. Tệp hỏng lẻ đi qua `note`, không qua
      // đường này — gộp hai thứ lại là báo động nhầm cho mười chín tệp đã vào.
      setError(String(err));
    } finally {
      setIngest(null);
      setBusy(false);
    }
  };

  /**
   * Xử lý lại cả thư viện.
   *
   * Dùng chung đường tiến trình với lúc nạp: cùng thanh chạy, cùng danh sách tệp hỏng.
   * Người dùng bấm nút này vì có gì đó kẹt, nên thứ họ cần thấy là **từng tệp một đang
   * chạy** — không phải một con quay không nói gì rồi biến mất.
   */
  const reprocess = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    setFailures([]);
    setAdded(0);
    setIngest({ path: stats()?.root ?? "", stage: "Đang chuẩn bị", done: 0, total: 0, finished: false, error: null });
    try {
      if (isDemo()) {
        await runDemoIngest(demoDocuments().map((doc) => doc.path));
        setDocs(demoDocuments());
      } else {
        setDocs(await reprocessLibrary(note));
        setStats(await libraryStats());
      }
    } catch (err) {
      setError(`Không xử lý lại được thư viện: ${err}`);
    } finally {
      setIngest(null);
      setBusy(false);
    }
  };

  const pick = async () => {
    setError(null);
    try {
      await addFiles(await pickDocuments());
    } catch (err) {
      setError(`Không mở được hộp thoại chọn tệp: ${err}`);
    }
  };

  const confirmRemove = async (doc: DocumentView) => {
    setRemoving(null);
    setBusy(true);
    setError(null);
    try {
      if (!isDemo()) await removeDocument(doc.id);
      setDocs((all) => all.filter((entry) => entry.id !== doc.id));
      if (!isDemo()) setStats(await libraryStats());
    } catch (err) {
      setError(`Không xoá được "${doc.title}": ${err}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-[880px] flex-col gap-2xl">
        <section class="flex flex-col gap-md">
          <div class="flex items-start gap-sm">
            <span class="mt-3xs grid size-7 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
              <Icon name="library" size={15} />
            </span>
            <div class="flex min-w-0 flex-col gap-3xs">
              <h2 class="m-0 flex items-center gap-2xs text-md font-semibold text-ink">
                {props.name ?? "Thư viện tài liệu"}
                <InfoDot text="Trợ lý không sửa tệp và không chạy lệnh trong dự án loại này." />
              </h2>
              <p class="m-0 text-xs text-muted">
                Trợ lý đọc tài liệu ở đây để trả lời.
              </p>
            </div>
          </div>

          <StatsStrip
            stats={stats()}
            loading={loading()}
            busy={busy()}
            onReprocess={() => void reprocess()}
          />
        </section>

        <section class="flex flex-col gap-md">
          <DropZone
            compact={docs().length > 0}
            busy={busy()}
            onPaths={(paths) => void addFiles(paths)}
            onPick={() => void pick()}
          />

          <Show when={ingest()}>
            {(frame) => (
              <div
                class="flex flex-col gap-2xs rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)"
                role="status"
                aria-live="polite"
              >
                <div class="flex items-baseline justify-between gap-sm">
                  <span class="min-w-0 truncate text-xs text-text">
                    {frame().stage}: <span class="font-mono text-2xs">{fileName(frame().path)}</span>
                  </span>
                  <span class="shrink-0 text-2xs text-muted tabular-nums">
                    {frame().done}/{frame().total}
                  </span>
                </div>
                <div
                  role="progressbar"
                  aria-valuenow={frame().done}
                  aria-valuemin={0}
                  aria-valuemax={frame().total}
                  aria-label="Tiến trình nạp tài liệu"
                  class="h-1.5 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
                >
                  <div
                    class="h-full rounded-pill bg-accent transition-[width] duration-[var(--dur-base)]"
                    style={{
                      width: `${frame().total === 0 ? 0 : Math.round((frame().done / frame().total) * 100)}%`,
                    }}
                  />
                </div>
              </div>
            )}
          </Show>

          {/* Tệp hỏng đứng riêng khỏi lỗi của cả lô, và câu đầu tiên nói về những tệp đã
              **vào được**. Người đọc một danh sách lỗi luôn cho rằng mọi thứ đã hỏng trừ
              khi có ai đó nói ngược lại ngay dòng đầu. */}
          <Show when={failures().length > 0}>
            <div class="flex flex-col gap-2xs rounded-card border border-line bg-warn-soft px-(--card-pad-x) py-(--card-pad-y)">
              <div class="flex items-start gap-sm">
                <span class="mt-3xs shrink-0 text-warn">
                  <Icon name="warn" size={15} />
                </span>
                <p class="m-0 flex flex-1 flex-wrap items-center gap-2xs text-xs text-text">
                  <span>
                    <Show when={added() > 0} fallback={<>Không tệp nào nạp được.</>}>
                      {added()} tệp đã vào, {failures().length} tệp không nạp được.
                    </Show>
                  </span>
                  <InfoDot text="Thư viện vẫn dùng bình thường với phần còn lại." />
                </p>
                <button
                  type="button"
                  onClick={() => setFailures([])}
                  aria-label="Ẩn danh sách tệp không nạp được"
                  class="shrink-0 rounded-icon p-3xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
                >
                  <Icon name="x" size={13} />
                </button>
              </div>
              <ul class="m-0 flex list-none flex-col gap-2xs p-0 pl-lg">
                <For each={failures()}>
                  {(failure) => (
                    <li class="flex flex-col gap-3xs">
                      <span class="min-w-0 truncate font-mono text-2xs text-text" title={failure.path}>
                        {fileName(failure.path)}
                      </span>
                      <span class="text-2xs text-muted">{failure.error}</span>
                    </li>
                  )}
                </For>
              </ul>
            </div>
          </Show>

          <Show when={error()}>
            {(message) => (
              <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <Show when={!loading() && docs().length > 0}>
            <DocumentTable
              docs={docs()}
              busy={busy()}
              onRemove={(doc) => setRemoving(doc)}
            />
          </Show>

          <Show when={loading()}>
            <p class="m-0 text-xs text-muted" role="status" aria-live="polite">
              Đang đọc thư viện…
            </p>
          </Show>
        </section>

        <Show when={docs().length > 0}>
          <SearchProbe disabled={busy()} />
        </Show>
      </div>

      <Show when={removing()}>
        {(doc) => (
          <ConfirmDialog
            icon="trash"
            title={`Xoá "${doc().title}" khỏi thư viện?`}
            body="Tệp gốc trên đĩa vẫn nguyên."
            more="Tài liệu và toàn bộ đoạn đã cắt từ nó bị bỏ khỏi thư viện, nên trợ lý sẽ không còn tìm thấy nội dung này nữa. Tệp gốc trên đĩa vẫn nguyên — nạp lại được bất cứ lúc nào."
            detail={doc().path}
            confirmLabel="Xoá khỏi thư viện"
            onClose={() => setRemoving(null)}
            onConfirm={() => void confirmRemove(doc())}
          />
        )}
      </Show>
    </div>
  );
}

/**
 * Dải sức khoẻ thư viện.
 *
 * Khi `semanticReady === false`, câu quan trọng nhất không phải lý do mà là vế thứ hai:
 * **tìm bằng từ khoá vẫn đang chạy**. Một người đọc "chưa sẵn sàng" mà không đọc được vế
 * đó sẽ ngồi đợi, hoặc tệ hơn là bỏ cuộc — trong khi thư viện đã trả lời được rồi.
 */
function StatsStrip(props: {
  stats: LibraryStats | null;
  loading: boolean;
  busy: boolean;
  onReprocess: () => void;
}) {
  return (
    <Show
      when={props.stats}
      fallback={
        <p class="m-0 rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y) text-xs text-muted">
          {props.loading ? "Đang đọc tình trạng thư viện…" : "Chưa có thông tin thư viện."}
        </p>
      }
    >
      {(stats) => (
        <div class="flex flex-col gap-sm rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
          <dl class="m-0 flex flex-wrap gap-x-2xl gap-y-sm">
            <Stat label="Tài liệu" value={String(stats().documents)} />
            <Stat label="Đoạn" value={String(stats().chunks)} />
            <Stat
              label="Đã nhúng"
              value={`${stats().embeddedChunks}/${stats().chunks}`}
            />
            <Stat label="Bộ nhúng" value={stats().embedder ?? "chưa có"} />
          </dl>

          <Show
            when={!stats().semanticReady}
            fallback={
              <p class="m-0 flex items-center gap-2xs text-2xs text-success">
                <Icon name="check" size={12} />
                Tìm theo ngữ nghĩa và từ khoá đều đang chạy.
              </p>
            }
          >
            <div class="flex items-start gap-sm rounded-panel bg-warn-soft px-sm py-2xs">
              <span class="mt-3xs shrink-0 text-warn">
                <Icon name="clock" size={13} />
              </span>
              <p class="m-0 flex flex-wrap items-center gap-2xs text-2xs text-text">
                <span>
                  <Show when={stats().reason}>
                    {(reason) => <>{reason()} </>}
                  </Show>
                  Tìm bằng <strong class="font-medium">từ khoá vẫn chạy</strong>, dùng
                  được ngay.
                </span>
                <InfoDot text="Câu trả lời sẽ bắt được nhiều cách diễn đạt hơn khi phần nhúng chạy xong." />
              </p>
            </div>
          </Show>

          {/* Nút này luôn có mặt, kể cả khi mọi thứ đang xanh. Một nút chỉ hiện ra lúc
              hỏng là một nút người dùng phải học thuộc chỗ nó *sẽ* xuất hiện, và họ tới
              đây vì đang không hiểu chuyện gì xảy ra — đó là lúc tệ nhất để đi tìm. */}
          <div class="flex flex-wrap items-center justify-between gap-sm border-t border-line pt-sm">
            <p class="m-0 flex max-w-[52ch] flex-wrap items-center gap-2xs text-2xs text-muted">
              <span>
                <Show
                  when={stats().chunks > stats().embeddedChunks}
                  fallback={<>Đọc lại mọi tệp, kể cả tệp lần trước hỏng.</>}
                >
                  Còn {stats().chunks - stats().embeddedChunks} đoạn chờ nhúng — bấm để
                  nhúng nốt.
                </Show>
              </span>
              <InfoDot text="Đọc lại mọi tệp trong thư mục, kể cả tệp lần trước đọc hỏng và tệp không đổi từ lần quét trước." />
            </p>
            <Button
              variant="outline"
              icon="retry"
              disabled={props.busy}
              onClick={props.onReprocess}
            >
              {props.busy ? "Đang xử lý…" : "Xử lý lại"}
            </Button>
          </div>
        </div>
      )}
    </Show>
  );
}

function Stat(props: { label: string; value: string }) {
  return (
    <div class="flex flex-col gap-3xs">
      <dt class="m-0 text-2xs text-faint">{props.label}</dt>
      <dd class="m-0 text-sm text-ink tabular-nums">{props.value}</dd>
    </div>
  );
}

/** Tên tệp cho dòng tiến trình: đường dẫn đầy đủ đẩy con số done/total ra khỏi màn hình. */
function fileName(path: string): string {
  return path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || path;
}
