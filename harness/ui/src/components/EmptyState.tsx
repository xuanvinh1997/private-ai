import { For, Show } from "solid-js";
import Icon from "./Icon";

/**
 * Gợi ý cho màn hình trống.
 *
 * Chọn theo việc *một coding agent làm được*, không theo việc nghe hay: mỗi câu ở đây
 * chạm vào một tool khác nhau — đọc, tìm, sửa, chạy lệnh — nên bấm thử một câu là thấy
 * ngay agent này khác một hộp chat ở chỗ nào.
 */
const SUGGESTIONS = [
  "Giải thích kiến trúc của repo này",
  "Tìm mọi chỗ dùng hàm `derive_messages`",
  "Bỏ hết `unwrap` trong crate pai-core",
  "Chạy bộ test và tóm tắt chỗ hỏng",
  "Viết test cho đường ống thi hành tool",
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
  "Viết regex khớp địa chỉ email rồi giải thích từng phần",
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
  /** Có dự án đang mở hay không. Sai đi một chỗ này là hứa nhầm cả bộ quyền. */
  hasProject: boolean;
  /** Mở màn hình dự án. Lối duy nhất từ đây ra khỏi trạng thái "chưa có dự án". */
  onOpenProject: () => void;
}) {
  return (
    <div class="mx-auto flex max-w-(--reading-measure) flex-col items-center gap-md px-(--page-pad-x) text-center">
      <Show
        when={props.hasProject}
        fallback={
          <>
            {/* Câu đầu tiên nói rằng **trò chuyện chạy được**, và nó phải đứng trước mọi
                thứ khác: người mở ứng dụng lần đầu chưa có dự án nào, và nếu điều đầu
                tiên họ đọc là một thứ còn thiếu thì họ kết luận ứng dụng chưa dùng được
                rồi đóng nó lại. */}
            <h2 class="m-0 text-2xl font-semibold text-ink">Trò chuyện được ngay</h2>
            <p class="m-0 max-w-[48ch] text-sm text-muted">
              Chưa mở dự án nào, nhưng cứ hỏi bên dưới — trợ lý trả lời bình thường.
            </p>

            {/* Câu thứ hai là câu giới hạn, và nó nói bằng lời của người dùng chứ không
                bằng tên tool: người đọc biết "đọc tệp" và "chạy lệnh", không ai biết
                `glob` với `docs.search` là gì trước khi thấy chúng chạy. */}
            <p class="m-0 flex max-w-[52ch] items-start gap-2xs rounded-panel bg-[var(--overlay-faint)] px-md py-sm text-left text-xs text-muted">
              <span class="mt-3xs shrink-0 text-faint">
                <Icon name="warn" size={13} />
              </span>
              <span>
                Chưa có dự án thì trợ lý không đọc, không sửa và không chạy được gì trên
                máy này. Mở một dự án là chỉ cho nó đúng một thư mục để làm việc.
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
        <p class="m-0 max-w-[46ch] text-sm text-muted">
          Trợ lý đọc và sửa được tệp trong thư mục làm việc, chạy được lệnh, và hỏi lại
          trước mỗi thao tác ghi.
        </p>
      </Show>
    </div>
  );
}

/** Nửa dưới: mấy câu bấm được, ngồi **dưới** ô soạn tin. */
export function PromptChips(props: {
  onPick: (text: string) => void;
  disabled?: boolean;
  hasProject: boolean;
}) {
  return (
    <ul class="mx-auto m-0 flex max-w-[52ch] list-none flex-wrap justify-center gap-2xs p-0">
      <For each={props.hasProject ? SUGGESTIONS : SUGGESTIONS_KHONG_DU_AN}>
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
