import { For } from "solid-js";
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

export default function EmptyState(props: { onPick: (text: string) => void; disabled?: boolean }) {
  return (
    <section class="flex min-h-[58vh] flex-col items-center justify-center gap-lg py-4xl text-center">
      <div class="grid size-12 place-items-center rounded-panel bg-accent-soft text-accent-ink">
        <Icon name="sparkle" size={24} />
      </div>

      <div class="flex flex-col gap-2xs">
        <h2 class="m-0 text-xl font-semibold text-ink">Bắt đầu một phiên làm việc</h2>
        <p class="m-0 max-w-[46ch] text-sm text-muted">
          Trợ lý đọc và sửa được tệp trong thư mục làm việc, chạy được lệnh, và hỏi lại
          trước mỗi thao tác ghi.
        </p>
      </div>

      <ul class="m-0 flex max-w-[52ch] list-none flex-wrap justify-center gap-sm p-0">
        <For each={SUGGESTIONS}>
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
