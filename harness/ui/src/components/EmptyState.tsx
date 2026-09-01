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

export default function EmptyState(props: {
  onPick: (text: string) => void;
  disabled?: boolean;
  /** Có dự án đang mở hay không. Sai đi một chỗ này là hứa nhầm cả bộ quyền. */
  hasProject: boolean;
  /** Mở màn hình dự án. Lối duy nhất từ đây ra khỏi trạng thái "chưa có dự án". */
  onOpenProject: () => void;
}) {
  return (
    <section class="flex min-h-[58vh] flex-col items-center justify-center gap-lg py-4xl text-center">
      <div
        class="grid size-12 place-items-center rounded-panel"
        classList={{
          "bg-accent-soft text-accent-ink": props.hasProject,
          "bg-[var(--overlay-faint)] text-muted": !props.hasProject,
        }}
      >
        <Icon name={props.hasProject ? "sparkle" : "chat"} size={24} />
      </div>

      <Show
        when={props.hasProject}
        fallback={
          <div class="flex flex-col items-center gap-md">
            <div class="flex flex-col gap-2xs">
              {/* Câu đầu tiên nói rằng **trò chuyện chạy được**, và nó phải đứng trước mọi
                  thứ khác: người mở ứng dụng lần đầu chưa có dự án nào, và nếu điều đầu
                  tiên họ đọc là một thứ còn thiếu thì họ kết luận ứng dụng chưa dùng được
                  rồi đóng nó lại. */}
              <h2 class="m-0 text-xl font-semibold text-ink">Trò chuyện được ngay</h2>
              <p class="m-0 max-w-[48ch] text-sm text-muted">
                Chưa mở dự án nào, nhưng cứ hỏi bên dưới — trợ lý trả lời bình thường.
              </p>
            </div>

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
          </div>
        }
      >
        <div class="flex flex-col gap-2xs">
          <h2 class="m-0 text-xl font-semibold text-ink">Bắt đầu một phiên làm việc</h2>
          <p class="m-0 max-w-[46ch] text-sm text-muted">
            Trợ lý đọc và sửa được tệp trong thư mục làm việc, chạy được lệnh, và hỏi lại
            trước mỗi thao tác ghi.
          </p>
        </div>
      </Show>

      <ul class="m-0 flex max-w-[52ch] list-none flex-wrap justify-center gap-sm p-0">
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
    </section>
  );
}
