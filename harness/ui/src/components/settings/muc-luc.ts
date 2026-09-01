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
  | "nhung"
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
 * Bốn nhóm, xếp theo *tần suất mở* chứ không theo mức độ quan trọng.
 *
 * Nhóm đầu không tên gồm hai trang ai cũng mở ít nhất một lần. "Mô hình" đứng riêng vì
 * hai trang ấy là hai máy chủ khác nhau cho hai việc khác nhau, và gộp chúng vào "Tích
 * hợp" sẽ làm mất mất điều đó. "Tích hợp" là những thứ **cắm thêm** vào lõi — MCP mang
 * tool từ ngoài vào, hook mang chính sách từ ngoài vào; cả hai đều là lệnh của người khác
 * chạy trên máy này, nên chúng thuộc về nhau. "Quyền" đứng một mình dưới "Nâng cao" vì
 * nó là trang duy nhất thay đổi *trợ lý được phép làm gì*, và một trang như thế không nên
 * nằm lẫn giữa những trang chỉ đổi màu chữ.
 */
export const NAV: NavGroup[] = [
  {
    pages: [
      {
        id: "chung",
        label: "Chung",
        icon: "monitor",
        desc: "Bảng màu, và cách bản ghi hội thoại được vẽ ra.",
      },
      {
        id: "phim-tat",
        label: "Phím tắt",
        icon: "enter",
        desc: "Một bảng tra cứu. Chưa gán lại được phím từ đây.",
      },
    ],
  },
  {
    title: "Mô hình",
    pages: [
      { id: "provider", label: "Mô hình hội thoại", icon: "server" },
      // Mô hình nhúng đứng riêng, không nằm trong trang provider. Gộp lại thì nó trông như
      // một tuỳ chọn nâng cao của việc chọn mô hình hội thoại, trong khi nó là một lựa chọn
      // độc lập và thường là một máy chủ khác hẳn — thường là máy chủ chạy tại chỗ, để tài
      // liệu không rời khỏi máy.
      { id: "nhung", label: "Mô hình nhúng", icon: "library" },
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
        desc: "Lệnh của bạn chạy trước mỗi lời gọi tool, và được quyền chặn nó.",
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
        desc: "Trợ lý được phép làm gì trên máy này, và cái gì đang giữ nó lại.",
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
    desc: "Sáng, tối, hoặc theo hệ thống. Đổi ngay không cần mở lại ứng dụng.",
  },
  {
    page: "chung",
    label: "Cách hiển thị hội thoại",
    desc: "Bong bóng hai bên như chat, hoặc trải hết bề rộng như một tài liệu.",
  },
  {
    page: "phim-tat",
    label: "Tìm phiên",
    desc: "⌘K hoặc Ctrl+K mở bảng chọn phiên từ bất cứ đâu.",
  },
  {
    page: "phim-tat",
    label: "Gửi tin nhắn",
    desc: "Enter gửi, Shift+Enter xuống dòng.",
  },
  {
    page: "provider",
    label: "Nhà cung cấp mô hình",
    desc: "Máy chủ giữ vai hội thoại: Ollama trên máy này, hoặc một API từ xa.",
  },
  {
    page: "provider",
    label: "Khoá API",
    desc: "Khoá gửi kèm mỗi yêu cầu tới provider từ xa. Lõi không bao giờ trả khoá về giao diện, nên ô này chỉ nhận khoá mới chứ không hiện lại khoá cũ.",
  },
  {
    page: "provider",
    label: "Base URL",
    desc: "Địa chỉ máy chủ mô hình. Đổi nó là đổi nơi từng câu hỏi của bạn được gửi tới.",
  },
  {
    page: "nhung",
    label: "Mô hình nhúng",
    desc: "Mô hình biến tài liệu thành vector để tìm theo ý nghĩa. Đổi nó thì cả thư viện được nhúng lại từ đầu.",
  },
  {
    page: "mcp",
    label: "Server MCP",
    desc: "Cắm thêm công cụ từ bên ngoài: kho mã, cơ sở dữ liệu, hệ thống theo dõi lỗi.",
  },
  {
    page: "hook",
    label: "Hook trước lời gọi tool",
    desc: "Lệnh ngoài chạy trước mỗi lời gọi tool và được quyền chặn nó. Hook chạy ngoài vòng giam, và hook hỏng thì cho qua.",
  },
  {
    page: "quyen",
    label: "Phạm vi tool cho lượt mới",
    desc: "Lượt mới bắt đầu ở mức chỉ đọc, đọc và ghi, hay được chạy lệnh trên máy này.",
  },
  {
    page: "quyen",
    label: "Vòng giam tiến trình",
    desc: "Sandbox chặn lệnh ghi ra ngoài thư mục dự án. Nó không chặn mạng và không chặn việc đọc.",
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
