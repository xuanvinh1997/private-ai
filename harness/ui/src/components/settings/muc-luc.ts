import type { IconName } from "../Icon";

/**
 * Mục lục của màn hình cài đặt: trang nào có, nằm nhóm nào, và tìm được bằng chữ gì.
 *
 * Một tệp dữ liệu chứ không phải một mảng nằm lẫn trong `SettingsView`, vì ô tìm phải
 * biết nội dung của **mọi** trang kể cả trang chưa mở lần nào. Ô tìm mà chỉ thấy trang
 * đang mở thì nó chỉ là một bộ lọc của trang đó, không phải một lối đi tới chỗ khác — và
 * lối đi tới chỗ khác mới là lý do người ta gõ vào ô tìm của một màn hình cài đặt.
 */

export type SettingsPage =
  | "chung"
  | "phim-tat"
  | "provider"
  | "mcp"
  | "hook"
  | "quyen";

export interface PageMeta {
  id: SettingsPage;
  label: string;
  icon: IconName;
  /** Câu dưới tiêu đề lớn. Bỏ trống ở trang tự mở đầu bằng một `SectionHead` của riêng nó. */
  desc?: string;
}

export interface NavGroup {
  /** Nhóm đầu không có tiêu đề: nó là chỗ mặc định, và đặt tên cho chỗ mặc định là thừa. */
  title?: string;
  pages: PageMeta[];
}

/**
 * Ba nhóm, xếp theo *tần suất mở* chứ không theo mức độ quan trọng.
 *
 * "Mô hình" từng là một nhóm hai trang — hội thoại và nhúng — và đó là một chỗ chia sai.
 * Hai vai đúng là hai máy chủ khác nhau cho hai việc khác nhau, nhưng chúng được **cấu
 * hình từ cùng một danh sách provider**, và tách ra thành hai trang buộc người dùng thêm
 * một provider ở trang này rồi đi sang trang kia mới giao được vai thứ hai cho nó — hai
 * lần đi qua cùng một danh sách để trả lời một câu hỏi. Giờ là một trang: danh sách máy
 * chủ, rồi hai ô chọn mô hình mặc định, cả hai đều hỏi thẳng máy chủ vừa cấu hình xong.
 *
 * Nó đứng **đầu** nhóm không tên vì đó là trang duy nhất mà không đi qua thì ứng dụng
 * không trả lời được câu nào — "Chung" và "Phím tắt" chỉ đổi cách nhìn.
 *
 * "Tích hợp" là những thứ **cắm thêm** vào lõi — MCP mang tool từ ngoài vào, hook mang
 * chính sách từ ngoài vào; cả hai đều là lệnh của người khác chạy trên máy này, nên chúng
 * thuộc về nhau. "Quyền" đứng một mình dưới "Nâng cao" vì nó là trang duy nhất thay đổi
 * *trợ lý được phép làm gì*, và một trang như thế không nên nằm lẫn giữa những trang chỉ
 * đổi màu chữ.
 */
export const NAV: NavGroup[] = [
  {
    pages: [
      {
        id: "provider",
        label: "Mô hình",
        icon: "server",
      },
      {
        id: "chung",
        label: "Chung",
        icon: "monitor",
        desc: "Bảng màu và cách hội thoại được vẽ ra.",
      },
      {
        id: "phim-tat",
        label: "Phím tắt",
        icon: "enter",
        desc: "Bảng tra cứu, chưa gán lại phím được.",
      },
    ],
  },
  {
    title: "Tích hợp",
    pages: [
      { id: "mcp", label: "Server MCP", icon: "plug" },
      {
        id: "hook",
        label: "Hook",
        icon: "tools",
        desc: "Lệnh của bạn chạy trước mỗi lời gọi tool.",
      },
    ],
  },
  {
    title: "Nâng cao",
    pages: [
      {
        id: "quyen",
        label: "Quyền",
        icon: "hand",
        desc: "Trợ lý được phép làm gì trên máy này.",
      },
    ],
  },
];

const ALL: PageMeta[] = NAV.flatMap((group) => group.pages);

export function pageMeta(id: SettingsPage): PageMeta {
  // Danh sách là hằng số trong mã và `SettingsPage` là một union đóng, nên nhánh lùi này
  // không chạy được. Nó tồn tại để không phải viết `!` — một dấu `!` ở đây là chỗ duy
  // nhất trong tệp mà kiểu bị nói dối.
  return ALL.find((page) => page.id === id) ?? ALL[0]!;
}

/** Một dòng tìm được: nó dẫn tới trang nào, và vì sao nó khớp. */
export interface SearchHit {
  page: SettingsPage;
  label: string;
  desc: string;
}

/**
 * Những gì ô tìm nhìn thấy.
 *
 * Viết tay chứ không rút tự động từ JSX của từng trang, và đây là một đánh đổi có thật:
 * bảng này lệch được khỏi trang thật. Đổi lại nó bao được cả những hàng chỉ xuất hiện
 * **bên trong một hộp thoại** — ô khoá API là một ví dụ, và đó đúng là thứ người ta gõ
 * vào ô tìm trước tiên. Rút tự động thì chỉ thấy được thứ đang hiện trên màn hình, tức
 * là không thấy gì cho tới khi người dùng đã tự đi tới đúng chỗ.
 *
 * Không có mặt ở đây: tên provider và tên server MCP của từng máy. Chúng là **dữ liệu**,
 * không phải cài đặt, và một ô tìm cài đặt trả về tên một server cụ thể sẽ hứa một lối đi
 * tới hàng đó mà màn hình chưa dựng được.
 */
export const SEARCH_INDEX: SearchHit[] = [
  {
    page: "chung",
    label: "Bảng màu",
    desc: "Sáng, tối, hoặc theo hệ thống; đổi là thấy ngay.",
  },
  {
    page: "chung",
    label: "Cách hiển thị hội thoại",
    desc: "Bong bóng hai bên, hoặc tài liệu trải hết trang.",
  },
  {
    page: "phim-tat",
    label: "Tìm phiên",
    desc: "⌘K hoặc Ctrl+K mở bảng chọn phiên.",
  },
  {
    page: "phim-tat",
    label: "Gửi tin nhắn",
    desc: "Enter gửi, Shift+Enter xuống dòng.",
  },
  {
    page: "provider",
    label: "Nhà cung cấp mô hình",
    desc: "Máy chủ hội thoại: Ollama, hoặc một API từ xa.",
  },
  {
    page: "provider",
    label: "Khoá API",
    desc: "Khoá gửi kèm mỗi yêu cầu tới provider từ xa.",
  },
  {
    page: "provider",
    label: "Base URL",
    desc: "Base URL của máy chủ mô hình, nơi câu hỏi tới.",
  },
  {
    page: "provider",
    label: "Mô hình nhúng",
    desc: "Mô hình biến tài liệu thành vector để tìm nghĩa.",
  },
  {
    page: "provider",
    label: "Mô hình hội thoại",
    desc: "Mô hình trả lời câu hỏi của bạn trong ô soạn tin.",
  },
  {
    page: "mcp",
    label: "Server MCP",
    desc: "Cắm tool từ ngoài: kho mã, cơ sở dữ liệu.",
  },
  {
    page: "hook",
    label: "Hook trước lời gọi tool",
    desc: "Lệnh ngoài chạy trước mỗi lời gọi tool và chặn được.",
  },
  {
    page: "quyen",
    label: "Phạm vi tool cho lượt mới",
    desc: "Lượt mới bắt đầu ở mức đọc, ghi, hay chạy lệnh.",
  },
  {
    page: "quyen",
    label: "Vòng giam tiến trình",
    desc: "Sandbox chặn lệnh ghi ra ngoài thư mục dự án.",
  },
];

/**
 * Bỏ dấu tiếng Việt để "khoa" tìm ra "khoá".
 *
 * Người ta gõ ô tìm bằng đúng cái tốc độ mà bàn phím cho phép, và bỏ dấu là thứ nhanh
 * nhất. Một ô tìm bắt gõ đủ dấu mới ra kết quả thì phần lớn lượt gõ sẽ ra rỗng, và người
 * dùng kết luận là "không có mục đó" chứ không kết luận là "mình gõ thiếu dấu".
 */
export function khongDau(raw: string): string {
  return raw
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/đ/g, "d");
}

/** Lọc mục lục theo chuỗi người dùng gõ. Chuỗi rỗng trả về rỗng, không trả về tất cả. */
export function timTrongCaiDat(query: string): SearchHit[] {
  const needle = khongDau(query.trim());
  if (needle === "") return [];
  return SEARCH_INDEX.filter((hit) => {
    // Khớp cả nhãn **và** mô tả: nửa số thứ người ta đi tìm trong một trang cài đặt không
    // có tên riêng, chúng chỉ được nhắc tới trong câu mô tả của hàng chứa chúng.
    const hay = khongDau(`${hit.label} ${hit.desc} ${pageMeta(hit.page).label}`);
    return hay.includes(needle);
  });
}
