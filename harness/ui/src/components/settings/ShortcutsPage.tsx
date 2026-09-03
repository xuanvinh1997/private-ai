import { For } from "solid-js";
import type { IconName } from "../Icon";
import { Row, RowGroup, SectionHead } from "./FormKit";

/**
 * Trang Phím tắt.
 *
 * Tách khỏi trang Chung vì nó **không đổi được gì**: nó là một bảng tra cứu, và một bảng
 * tra cứu nằm dưới hai hàng có ô chọn sẽ bị đọc lướt qua như phần chú thích của chúng.
 *
 * Phím hiện ở cột phải, đúng chỗ mọi trang khác đặt điều khiển. Nghe như đặt sai — cột
 * phải vốn là chỗ *bấm được* — nhưng cột phải thật ra là chỗ đặt **giá trị hiện tại của
 * hàng**, và giá trị của một hàng phím tắt chính là tổ hợp phím. Ngày nào gán lại được
 * phím thì cái `<kbd>` ấy thành một nút, và không hàng nào phải dời chỗ.
 */

interface Shortcut {
  keys: string;
  what: string;
  desc?: string;
  /** Câu dài đằng sau hàng, cất trong `InfoDot` cạnh nhãn. */
  more?: string;
}

const NHOM: { title: string; desc: string; icon: IconName; items: Shortcut[] }[] = [
  {
    title: "Điều hướng",
    desc: "Chạy được kể cả khi đang gõ tin nhắn.",
    icon: "search",
    items: [
      {
        keys: "⌘K / Ctrl+K",
        what: "Tìm phiên",
        desc: "Mở bảng chọn phiên và lọc theo tên.",
        more: "Mở bảng chọn phiên và lọc theo tên, từ bất cứ màn hình nào.",
      },
      {
        keys: "Esc",
        what: "Đóng thứ đang mở",
        desc: "Đóng hộp thoại, hoặc thoát khỏi màn hình cài đặt.",
      },
    ],
  },
  {
    title: "Soạn tin",
    desc: "Chỉ có tác dụng trong ô soạn tin.",
    icon: "pencil",
    items: [
      { keys: "Enter", what: "Gửi tin nhắn" },
      {
        keys: "Shift+Enter",
        what: "Xuống dòng",
        desc: "Ô soạn tin tự cao thêm theo số dòng.",
        more: "Ô soạn tin tự cao thêm theo số dòng, không có thanh cuộn riêng.",
      },
      {
        keys: "Enter",
        what: "Xếp hàng khi đang bận",
        desc: "Câu được giữ lại, gửi khi lượt hiện tại xong.",
        more: "Trợ lý đang trả lời thì Enter giữ câu lại và gửi khi lượt hiện tại xong. Đúng một câu chờ; gõ tiếp thì thay câu cũ.",
      },
    ],
  },
  {
    title: "Hoàn thành trong ô soạn tin",
    desc: "Danh sách gợi ý giành phím khi nó đang mở.",
    icon: "sparkle",
    items: [
      {
        keys: "@",
        what: "Chèn đường dẫn tệp",
        desc: "Gõ @ ở đầu một từ rồi gõ để lọc.",
        more: "Gõ @ ở đầu một từ rồi gõ tiếp để lọc. Chỉ thấy tệp mà chỉ mục đã quét.",
      },
      {
        keys: "/",
        what: "Bảng lệnh",
        desc: "Chỉ mở khi / là ký tự đầu tiên.",
        more: "Chỉ mở khi / là ký tự đầu tiên của ô nhập, nên gõ một đường dẫn không làm nó bật ra.",
      },
      { keys: "↑ / ↓", what: "Đi trong danh sách gợi ý" },
      { keys: "Enter / Tab", what: "Chọn gợi ý đang sáng" },
      {
        keys: "Esc",
        what: "Đóng danh sách gợi ý",
        desc: "Chỉ đóng danh sách, giữ nguyên chữ đã gõ.",
        more: "Chỉ đóng danh sách, giữ nguyên chữ đã gõ. Gõ tiếp thì nó mở lại.",
      },
    ],
  },
];

export default function ShortcutsPage() {
  return (
    <div class="flex flex-col gap-2xl">
      <For each={NHOM}>
        {(group) => (
          <section class="flex flex-col gap-md">
            <SectionHead icon={group.icon} title={group.title} desc={group.desc} />
            <RowGroup>
              <For each={group.items}>
                {(item) => (
                  <Row
                    label={item.what}
                    desc={item.desc}
                    more={item.more}
                    control={() => (
                      <kbd class="rounded-btn border border-line bg-surface-soft px-2xs py-3xs font-mono text-2xs text-text">
                        {item.keys}
                      </kbd>
                    )}
                  />
                )}
              </For>
            </RowGroup>
          </section>
        )}
      </For>
    </div>
  );
}
