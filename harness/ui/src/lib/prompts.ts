import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import type { ProjectKind, PromptSeeds } from "./protocol";

/** Không có gì để dựng gợi ý từ đó — và đó là một trạng thái, không phải một lỗi. */
export const NO_SEEDS: PromptSeeds = { symbols: [], directories: [], documents: [] };

/**
 * Nguyên liệu dựng gợi ý, lấy từ dự án đang mở.
 *
 * **Nuốt lỗi**, cùng lý do với `listDocuments`: lệnh này chạy lúc dựng màn hình chứ không
 * sau một cú bấm, nên không có ai đang đứng chờ một câu trả lời. Hỏng thì trả về rỗng và
 * màn hình trống lùi về bộ gợi ý tĩnh — người dùng vẫn có năm câu bấm được, và họ sẽ gặp
 * đúng lỗi ấy ở chỗ nó thật sự cản trở họ.
 */
export async function promptSeeds(): Promise<PromptSeeds> {
  if (!inTauri()) return NO_SEEDS;
  try {
    return await invoke<PromptSeeds>("prompt_seeds");
  } catch (err) {
    console.error("không lấy được nguyên liệu gợi ý", err);
    return NO_SEEDS;
  }
}

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
 * **Trần** số chip, không phải số cố định.
 *
 * Bộ tĩnh của dự án mã nguồn có đúng năm câu, hai bộ kia chỉ có bốn — nên không thêm
 * nguyên liệu nào thì hàng chip ngắn hơn trần, và đó là đúng. Đệm cho đủ năm bằng một
 * câu chung chung là đánh đổi thứ duy nhất mấy con chip này có: câu nào cũng bấm được và
 * câu nào cũng dạy được một việc.
 */
const SO_CHIP = 5;

/**
 * Câu hỏi dựng từ **kho mã của người dùng**, không phải từ kho nào khác.
 *
 * Ba câu, ba tool khác nhau — đọc một ký hiệu, lần ngược người gọi, mở một thư mục — nên
 * bấm thử là thấy ngay agent này khác một hộp chat ở chỗ nào. Và cả ba đều gọi tên một
 * thứ chỉ mục vừa xác nhận là **có thật**, nên không câu nào bấm vào rồi trả về rỗng.
 */
function tuMaNguon(seeds: PromptSeeds): string[] {
  const ra: string[] = [];
  const [sym1, sym2] = seeds.symbols;
  if (sym1) ra.push(`\`${sym1}\` làm gì?`);
  if (sym2) ra.push(`Ai gọi \`${sym2}\`?`);
  const thu_muc = seeds.directories[0];
  if (thu_muc) ra.push(`Có gì trong \`${thu_muc}\`?`);
  return ra;
}

/**
 * Câu hỏi dựng từ **thư viện của người dùng**.
 *
 * Câu so sánh chỉ xuất hiện khi thư viện có từ hai tài liệu. Một thư viện một tệp mà
 * được mời "so sánh" là một gợi ý tự mâu thuẫn, và người dùng bấm vào để nhận lại lời
 * giải thích rằng không có gì để so.
 */
function tuTaiLieu(titles: string[]): string[] {
  const ra: string[] = [];
  const [t1, t2] = titles;
  if (t1) ra.push(`Tóm tắt “${t1}” trong một câu`);
  if (t1 && t2) ra.push(`“${t1}” và “${t2}” khác nhau chỗ nào?`);
  return ra;
}

/**
 * Ghép bộ động lên trước bộ tĩnh, cắt còn [`SO_CHIP`] câu.
 *
 * Động trước vì đó là những câu chứng minh trợ lý đã đọc *kho của bạn* chứ không đọc một
 * kho mẫu nào đó; tĩnh theo sau để phần đuôi vẫn chạm tới những tool mà bộ động bỏ sót.
 * Nguyên liệu rỗng — chưa mở dự án, chỉ mục chưa quét, thư viện chưa có gì — thì hàm này
 * trả về đúng bộ tĩnh cũ, nên màn hình trống không bao giờ trống.
 */
export function goiY(kind: ProjectKind | null, seeds: PromptSeeds): string[] {
  const tinh =
    kind === null
      ? SUGGESTIONS_KHONG_DU_AN
      : kind === "docs"
        ? SUGGESTIONS_TAI_LIEU
        : SUGGESTIONS;
  const dong = kind === "docs" ? tuTaiLieu(seeds.documents) : kind === "code" ? tuMaNguon(seeds) : [];

  const ra: string[] = [];
  for (const cau of [...dong, ...tinh]) {
    if (ra.length >= SO_CHIP) break;
    if (!ra.includes(cau)) ra.push(cau);
  }
  return ra;
}
