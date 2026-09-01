/**
 * Tách văn bản trợ lý thành đoạn chữ và khối mã có rào ba dấu huyền.
 *
 * Đây **không phải** một bộ dựng markdown. Repo không có thư viện markdown và không được
 * thêm, và một bộ dựng viết tay đủ dùng thì luôn dừng ở chỗ nó sai — đậm lồng trong
 * nghiêng, bảng, tham chiếu liên kết. Phạm vi là khối rào, đúng bằng thứ cần để vẽ được
 * sơ đồ và tô được mã. Ngoài rào ra, chữ vẫn là chữ.
 */

export type Segment =
  | { kind: "text"; text: string }
  /** `closed` là false khi rào mở mà chưa gặp rào đóng — tức là trợ lý còn đang gõ. */
  | { kind: "fence"; lang: string; code: string; closed: boolean };

/** Rào mở: tối đa ba dấu cách thụt vào, từ ba dấu huyền trở lên, rồi chuỗi thông tin. */
const OPEN = /^ {0,3}(`{3,})[ \t]*([^`\n]*)$/;
/** Rào đóng: cùng luật thụt, không có gì sau dãy dấu huyền. */
const CLOSE = /^ {0,3}(`{3,})[ \t]*$/;

export function splitFences(input: string): Segment[] {
  const out: Segment[] = [];
  const lines = input.split("\n");
  let text: string[] = [];

  const flushText = (): void => {
    if (text.length === 0) return;
    const joined = text.join("\n");
    // Đoạn chữ rỗng hoàn toàn giữa hai khối mã chỉ tạo thêm một khoảng trắng thừa.
    if (joined.trim() !== "") out.push({ kind: "text", text: joined });
    text = [];
  };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i] ?? "";
    const open = OPEN.exec(line);
    if (open === null) {
      text.push(line);
      continue;
    }

    flushText();
    const ticks = (open[1] ?? "```").length;
    const lang = (open[2] ?? "").trim().split(/\s+/)[0] ?? "";
    const body: string[] = [];
    let closed = false;
    i += 1;
    for (; i < lines.length; i += 1) {
      const inner = lines[i] ?? "";
      const close = CLOSE.exec(inner);
      if (close !== null && (close[1] ?? "").length >= ticks) {
        closed = true;
        break;
      }
      body.push(inner);
    }
    out.push({ kind: "fence", lang: lang.toLowerCase(), code: body.join("\n"), closed });
  }

  flushText();
  return out;
}

/** Nhãn ngôn ngữ hiện trên đầu khối mã. Tên lạ thì hiện nguyên tên. */
const LANG_LABEL: Record<string, string> = {
  "": "mã",
  text: "văn bản",
  txt: "văn bản",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  console: "shell",
  rs: "rust",
  py: "python",
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  yml: "yaml",
  md: "markdown",
};

export const langLabel = (lang: string): string => LANG_LABEL[lang] ?? lang;
