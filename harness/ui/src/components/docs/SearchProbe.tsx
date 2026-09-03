import { For, Show, createSignal } from "solid-js";
import { isDemo } from "../../lib/demo";
import { searchDocuments } from "../../lib/docs";
import { demoHits } from "../../lib/fixtures/docs";
import type { DocumentHit } from "../../lib/protocol";
import Icon from "../Icon";
import { InfoDot } from "../settings/FormKit";
import { Button } from "../projects/DialogShell";

/**
 * Ô thử tìm: kiểm chứng thư viện **trước** khi đem nó đi hỏi trợ lý.
 *
 * Khi câu trả lời của trợ lý sai, có hai nguyên nhân rất khác nhau — thư viện không tìm
 * ra đoạn đúng, hay mô hình đọc đoạn đúng rồi trả lời sai. Không có ô này thì hai nguyên
 * nhân đó trông giống hệt nhau, và người dùng sẽ đi sửa cái không hỏng.
 *
 * Trích đoạn hiện **nguyên văn và đóng khung như một trích dẫn**. Đó là nội dung do
 * người ngoài viết, nạp vào từ một tệp bất kỳ; trộn nó vào giọng của ứng dụng là để một
 * dòng chữ trong tài liệu nói thay ứng dụng.
 */
export default function SearchProbe(props: { disabled?: boolean }) {
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<DocumentHit[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const run = async () => {
    const text = query().trim();
    if (text === "" || busy()) return;
    setBusy(true);
    setError(null);
    try {
      setHits(isDemo() ? demoHits(text) : await searchDocuments(text));
    } catch (err) {
      setHits(null);
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="flex flex-col gap-md">
      <div class="flex flex-col gap-3xs">
        <h3 class="m-0 flex items-center gap-2xs text-sm font-semibold text-ink">
          Thử tìm
          <InfoDot text="Đây chỉ là tìm kiếm — không có câu trả lời nào được sinh ra ở đây." />
        </h3>
        <p class="m-0 text-xs text-muted">
          Gõ câu hỏi để xem thư viện tìm ra gì.
        </p>
      </div>

      <div class="flex flex-wrap gap-sm">
        <label class="flex min-w-[220px] flex-1 items-center gap-2xs rounded-btn border border-line bg-surface px-sm focus-within:border-accent">
          <span class="shrink-0 text-faint">
            <Icon name="search" size={14} />
          </span>
          <input
            type="search"
            value={query()}
            spellcheck={false}
            placeholder="Ví dụ: quy trình khôi phục sau sự cố"
            aria-label="Câu hỏi để thử tìm trong thư viện"
            disabled={props.disabled}
            onInput={(event) => setQuery(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void run();
              }
            }}
            class="h-(--control-h) min-w-0 flex-1 bg-transparent text-xs text-text outline-none placeholder:text-faint disabled:opacity-50"
          />
        </label>
        <Button
          variant="outline"
          disabled={props.disabled || busy() || query().trim() === ""}
          onClick={() => void run()}
        >
          {busy() ? "Đang tìm…" : "Tìm"}
        </Button>
      </div>

      <Show when={error()}>
        {(message) => (
          <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <div aria-live="polite" aria-busy={busy() ? "true" : "false"}>
        <Show when={hits()}>
          {(list) => (
            <Show
              when={list().length > 0}
              fallback={
                <p class="m-0 flex items-center justify-center gap-2xs rounded-card border border-dashed border-line px-(--card-pad-x) py-lg text-center text-xs text-muted">
                  Không tìm thấy đoạn nào khớp.
                  <InfoDot text="Thư viện có thể chưa có tài liệu về chuyện này, hoặc câu hỏi dùng từ khác với tài liệu." />
                </p>
              }
            >
              <ul class="m-0 flex list-none flex-col gap-sm p-0">
                <For each={list()}>{(hit) => <Hit hit={hit} />}</For>
              </ul>
            </Show>
          )}
        </Show>
      </div>
    </section>
  );
}

function Hit(props: { hit: DocumentHit }) {
  return (
    <li class="flex flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
      <div class="flex flex-wrap items-center gap-2xs">
        <span class="min-w-0 truncate text-xs font-medium text-ink" title={props.hit.path}>
          {props.hit.title}
        </span>
        <span class="text-2xs text-faint tabular-nums">đoạn {props.hit.ordinal}</span>
        <MatchBadge by={props.hit.matchedBy} />
        <span class="ml-auto text-2xs text-faint tabular-nums">
          {props.hit.score.toFixed(2)}
        </span>
      </div>

      {/* Vạch dọc bên trái cộng chữ nghiêng: dấu hiệu quy ước của một trích dẫn. Không
          cắt bớt và không sửa chính tả — đây là chữ của tài liệu, không phải chữ của ta. */}
      <blockquote class="m-0 border-l-2 border-line-strong bg-surface-soft py-2xs pr-sm pl-md text-xs whitespace-pre-wrap text-text italic">
        {props.hit.text}
      </blockquote>
      <p class="m-0 text-2xs text-faint">Trích nguyên văn từ tài liệu do bạn nạp lên.</p>
    </li>
  );
}

/**
 * Vì sao đoạn này khớp.
 *
 * Ba nhãn này giải thích được điều mà điểm số không giải thích nổi: một thư viện chưa
 * nhúng xong chỉ trả về `keyword`, và khi người dùng nhìn thấy điều đó họ hiểu ngay vì
 * sao kết quả hôm nay khác hôm qua.
 */
function MatchBadge(props: { by: DocumentHit["matchedBy"] }) {
  const label = () =>
    props.by === "both" ? "từ khoá + ngữ nghĩa" : props.by === "semantic" ? "ngữ nghĩa" : "từ khoá";
  return (
    <span
      class="inline-flex shrink-0 items-center rounded-pill px-2xs py-3xs text-2xs whitespace-nowrap"
      classList={{
        "bg-accent-soft text-accent-ink": props.by === "both",
        "bg-[var(--overlay-faint)] text-muted": props.by !== "both",
      }}
    >
      {label()}
    </span>
  );
}
