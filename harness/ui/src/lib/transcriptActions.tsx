import { createContext, useContext, type JSX } from "solid-js";

export interface TranscriptActions {
  /** Gửi lại một tin nhắn người dùng. `null` nghĩa là lượt đang chạy, không cho gửi lại. */
  resend: ((text: string) => void) | null;
  /** Xoá một node khỏi bản ghi *đang xem*. Sổ tay phiên bên Rust không đổi. */
  remove: (id: string) => void;
  /**
   * Mở một tệp trong một khung xem, ở đúng dòng nếu chỗ gọi biết.
   *
   * `null` nghĩa là không có khung nào để mở vào — đường dẫn lúc đó vẫn hiện, chỉ là không
   * bấm được. Một đường dẫn trông như nút bấm mà bấm không ra gì tệ hơn hẳn một đường dẫn
   * trông như chữ. Vỏ ứng dụng hiện truyền `null`: nó không còn màn hình đọc mã nguồn nào,
   * vì người dùng đã có editor riêng của họ.
   */
  openFile: ((path: string, line?: number) => void) | null;
}

const NOOP: TranscriptActions = { resend: null, remove: () => {}, openFile: null };

const Ctx = createContext<TranscriptActions>(NOOP);

/**
 * Hành động của một tin nhắn, truyền qua context chứ không qua props.
 *
 * Sổ đăng ký renderer chỉ nhận đúng một prop là `node` — đó là hợp đồng làm cho việc
 * thêm loại node mới không phải sửa `Transcript`. Nhét thêm callback vào hợp đồng đó sẽ
 * bắt *mọi* renderer khai báo chúng, kể cả những cái không có hành động nào. Context
 * giữ hợp đồng nguyên vẹn và chỉ ai cần mới đọc.
 */
export function TranscriptActionsProvider(props: {
  value: TranscriptActions;
  children: JSX.Element;
}) {
  return <Ctx.Provider value={props.value}>{props.children}</Ctx.Provider>;
}

export const useTranscriptActions = (): TranscriptActions => useContext(Ctx);
