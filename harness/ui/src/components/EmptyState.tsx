import { For, Show } from "solid-js";
import { displayMode } from "../lib/prefs";
import type { ProjectKind } from "../lib/protocol";
import Icon from "./Icon";
import { InfoDot } from "./settings/FormKit";

/**
 * Gợi ý cho dự án **mã nguồn**.
 *
 * Chọn theo việc *một coding agent làm được*, không theo việc nghe hay: mỗi câu ở đây
 * chạm vào một tool khác nhau — đọc, tìm, sửa, chạy lệnh — nên bấm thử một câu là thấy
 * ngay agent này khác một hộp chat ở chỗ nào.
 *
 * Và không câu nào gọi tên một thứ **của repo này**. `pai-core` với `derive_messages` chỉ
 * tồn tại ở đây; người dùng mở dự án của họ ra và đọc được một cái tên không có trong mã
 * của mình thì gợi ý đó vừa nói rằng nó được viết cho máy của người khác.
 */
const SUGGESTIONS = [
  "Giải thích kiến trúc của dự án này",
  "Chạy bộ test và tóm tắt chỗ hỏng",
  "Có gì thay đổi so với commit gần nhất?",
  "Viết test cho phần chưa được kiểm",
  "Tìm chỗ xử lý lỗi cẩu thả",
];

/**
 * Gợi ý cho dự án **tài liệu**, và chúng phải khác hẳn bộ trên.
 *
 * Thư viện tài liệu chỉ được cắm `rag` — `docs.search`, `docs.read`, `docs.list`. Không
 * `fs`, không `shell`, không `index`; xem `DOCS_PLUGINS` phía lõi. Nên "chạy bộ test" ở
 * đây là một nút bấm vào sẽ thất bại, và một nút dựng sẵn mà thất bại dạy người dùng rằng
 * cả ứng dụng chưa dùng được.
 *
 * Cả bốn câu đều **không** giả định thư viện chứa gì: người dùng vừa chỉ vào một thư mục
 * mà ứng dụng chưa từng đọc, nên một gợi ý nhắc tên một chủ đề cụ thể là một lời đoán, và
 * đoán trượt thì câu trả lời rỗng.
 */
const SUGGESTIONS_TAI_LIEU = [
  "Thư viện này có những tài liệu gì?",
  "Tóm tắt mỗi tài liệu trong một câu",
  "Những chủ đề chính ở đây là gì?",
  "Trích đoạn nói về chủ đề chính, kèm tên tệp",
];

/**
 * Gợi ý khi **chưa mở dự án nào**, và chúng phải khác hẳn bộ trên.
 *
 * Không có dự án thì lõi không cắm tool nào chạm tới đĩa. Một gợi ý kiểu "sửa lỗi biên
 * dịch trong tệp này" ở đây là một gợi ý bấm vào sẽ thất bại — và một nút dựng sẵn mà
 * thất bại dạy người dùng rằng cả ứng dụng chưa dùng được. Mỗi câu dưới đây trả lời được
 * bằng đúng thứ còn lại: kiến thức của mô hình.
 */
const SUGGESTIONS_KHONG_DU_AN = [
  "Khác nhau giữa async và luồng trong Rust là gì?",
  "Viết regex khớp email rồi giải thích từng phần",
  "SQLite hay Postgres cho một ứng dụng chạy tại chỗ?",
  "Giải thích `git rebase` bằng một ví dụ ngắn",
];

/**
 * Nửa trên của màn hình trống: câu hỏi lớn, và những gì phải đọc **trước** khi gõ.
 *
 * Tách khỏi phần gợi ý vì hai nửa ngồi hai bên ô soạn tin. Trạng thái trống của ChatGPT
 * đặt đúng một câu hỏi ngay sát trên ô nhập rồi thôi — không có gì chen giữa câu hỏi và
 * chỗ trả lời nó — còn gợi ý thì rơi xuống dưới. Nhét gợi ý vào giữa là đẩy ô soạn tin ra
 * xa câu hỏi vừa hỏi người dùng, và cả bố cục mất điểm tựa.
 *
 * Ba thông điệp của trạng thái không-dự-án giữ nguyên từng chữ; ở đây chúng chỉ đổi chỗ
 * và đổi cỡ.
 */
export function EmptyLead(props: {
  /**
   * Loại dự án đang mở, `null` là chưa mở dự án nào. Sai đi một chỗ này là hứa nhầm cả
   * bộ quyền — và lời hứa ấy nằm ngay trên ô soạn tin, trước khi người dùng gõ chữ đầu.
   */
  kind: ProjectKind | null;
  /** Mở màn hình dự án. Lối duy nhất từ đây ra khỏi trạng thái "chưa có dự án". */
  onOpenProject: () => void;
}) {
  return (
    <div class="mx-auto flex max-w-(--reading-measure) flex-col items-center gap-md px-(--page-pad-x) text-center">
      <Show
        when={props.kind !== null}
        fallback={
          <>
            {/* Câu đầu tiên nói rằng **trò chuyện chạy được**, và nó phải đứng trước mọi
                thứ khác: người mở ứng dụng lần đầu chưa có dự án nào, và nếu điều đầu
                tiên họ đọc là một thứ còn thiếu thì họ kết luận ứng dụng chưa dùng được
                rồi đóng nó lại. */}
            <h2 class="m-0 text-2xl font-semibold text-ink">Trò chuyện được ngay</h2>
            <p class="m-0 max-w-[48ch] text-sm text-muted">
              Chưa có dự án, trợ lý vẫn trả lời được.
            </p>

            {/* Câu thứ hai là câu giới hạn, và nó nói bằng lời của người dùng chứ không
                bằng tên tool: người đọc biết "đọc tệp" và "chạy lệnh", không ai biết
                `glob` với `docs.search` là gì trước khi thấy chúng chạy. */}
            <p class="m-0 flex max-w-[52ch] items-start gap-2xs rounded-panel bg-[var(--overlay-faint)] px-md py-sm text-left text-xs text-muted">
              <span class="mt-3xs shrink-0 text-faint">
                <Icon name="warn" size={13} />
              </span>
              <span class="flex flex-wrap items-center gap-2xs">
                Trợ lý chưa đọc, sửa hay chạy gì trên máy.
                <InfoDot
                  label="Về giới hạn khi chưa có dự án"
                  text="Chưa có dự án thì trợ lý không đọc, không sửa và không chạy được gì trên máy này. Mở một dự án là chỉ cho nó đúng một thư mục để làm việc."
                />
              </span>
            </p>

            <button
              type="button"
              onClick={props.onOpenProject}
              class="flex items-center gap-2xs rounded-pill bg-accent px-md py-2xs text-sm font-medium text-on-accent transition-colors duration-[var(--dur-fast)] hover:bg-accent-hover"
            >
              <Icon name="folder-open" size={14} />
              Mở một dự án
            </button>
          </>
        }
      >
        {/* Một câu hỏi, không một lời chào: câu hỏi để lại chỗ trống mà ô nhập ngay dưới
            lấp vào, còn một lời chào thì tự đóng lại và không dẫn đi đâu. */}
        <h2 class="m-0 text-2xl font-semibold text-ink">Ta làm gì hôm nay?</h2>
        {/* Câu này là lời hứa về quyền, nên nó phải kể đúng bộ tool của **loại dự án đang
            mở**. Hứa "sửa được tệp, chạy được lệnh" trong một thư viện tài liệu — nơi lõi
            chỉ cắm `rag` — là hứa hai thứ không tồn tại, và người dùng chỉ phát hiện ra
            sau khi đã nhờ một việc không ai làm được. */}
        <p class="m-0 flex max-w-[46ch] flex-wrap items-center justify-center gap-2xs text-sm text-muted">
          <Show
            when={props.kind === "docs"}
            fallback={
              <>
                Trợ lý đọc, sửa tệp và chạy lệnh ở đây.
                <InfoDot
                  label="Về quyền trong dự án mã nguồn"
                  text="Trợ lý đọc và sửa được tệp trong thư mục làm việc, chạy được lệnh, và hỏi lại trước mỗi thao tác ghi."
                />
              </>
            }
          >
            Trợ lý đọc tài liệu để trả lời, kèm nguồn.
            <InfoDot
              label="Về quyền trong thư viện tài liệu"
              text="Trợ lý tìm và đọc tài liệu trong thư viện này để trả lời, kèm chỗ nó lấy ra. Nó không sửa tệp và không chạy lệnh trong dự án loại này."
            />
          </Show>
        </p>
      </Show>
    </div>
  );
}

/**
 * Nửa dưới: mấy câu bấm được, ngồi **dưới** ô soạn tin.
 *
 * Rộng đúng bằng ô soạn tin và ăn cùng mép trái với hàng chip trạng thái ngay trên nó —
 * cùng `displayMode`, cùng `px-2xs`. Một cụm hẹp hơn, căn giữa, dưới một hàng căn trái sẽ
 * đọc ra thành hai khối rời nhau thay vì phần đuôi của cùng một ô nhập.
 */
export function PromptChips(props: {
  onPick: (text: string) => void;
  disabled?: boolean;
  kind: ProjectKind | null;
}) {
  const goi_y = () =>
    props.kind === null
      ? SUGGESTIONS_KHONG_DU_AN
      : props.kind === "docs"
        ? SUGGESTIONS_TAI_LIEU
        : SUGGESTIONS;

  return (
    <ul
      class="mx-auto my-0 flex w-full list-none flex-wrap gap-2xs px-2xs py-0"
      classList={{
        "max-w-(--reading-measure)": displayMode() === "bubble",
        "max-w-[min(100%,980px)]": displayMode() === "document",
      }}
    >
      <For each={goi_y()}>
        {(text) => (
          <li>
            <button
              type="button"
              disabled={props.disabled}
              onClick={() => props.onPick(text)}
              class="rounded-pill border border-line bg-surface px-md py-2xs text-xs text-text transition-colors duration-[var(--dur-fast)] hover:border-accent hover:bg-accent-soft hover:text-accent-ink disabled:opacity-40"
            >
              {text}
            </button>
          </li>
        )}
      </For>
    </ul>
  );
}
