import type { IconName } from "../components/Icon";

/**
 * Nhận dạng ngôn ngữ, biểu tượng theo loại tệp, và một bộ tô màu cú pháp cố tình nhỏ.
 *
 * Bộ tô ở đây quét **một lượt** qua ký tự chứ không chạy một chuỗi `replace` bằng regex.
 * Lý do là chuỗi regex sai theo kiểu khó thấy nhất: một từ khoá nằm trong chuỗi ký tự
 * hay trong chú thích vẫn bị tô, và người đọc mã tin vào màu trước khi kịp đọc chữ. Máy
 * quét một lượt thì trạng thái "đang trong chuỗi" là thật, không phải đoán.
 *
 * Ngôn ngữ nào không nằm trong bảng dưới thì trả về **một khối chữ đơn sắc** — không cố
 * đoán. Chữ đơn sắc đọc được vẫn hơn một bộ tô màu sai.
 */

const BY_EXTENSION: Record<string, string> = {
  ts: "typescript", tsx: "typescript", mts: "typescript", cts: "typescript",
  js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
  rs: "rust",
  py: "python", pyi: "python",
  go: "go",
  c: "c", h: "c", cpp: "c", cc: "c", hpp: "c", java: "c", cs: "c", swift: "c", kt: "c",
  json: "json",
  css: "css", scss: "css",
  sh: "shell", bash: "shell", zsh: "shell", fish: "shell",
  yml: "yaml", yaml: "yaml",
  toml: "toml",
  sql: "sql",
  md: "markdown", markdown: "markdown",
  html: "markup", htm: "markup", xml: "markup", svg: "markup",
  txt: "text", lock: "text", log: "text",
};

export function extensionOf(path: string): string {
  const name = path.split(/[/\\]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot <= 0 ? "" : name.slice(dot + 1).toLowerCase();
}

export function langFromPath(path: string): string | null {
  return BY_EXTENSION[extensionOf(path)] ?? null;
}

/**
 * Ngôn ngữ từ một phần mở rộng trần.
 *
 * `FileView.lang` của lõi là **đuôi tệp**, không phải tên ngôn ngữ — lõi nói thẳng ra
 * rằng việc đoán ngôn ngữ là của giao diện. Đưa thẳng `"rs"` vào bảng ngữ pháp thì không
 * khớp gì cả và cả tệp rơi về chữ đơn sắc, im lặng.
 */
export function langFromExtension(ext: string | null): string | null {
  return ext === null ? null : (BY_EXTENSION[ext.toLowerCase()] ?? null);
}

const CODE_ICON = new Set([
  "typescript", "javascript", "rust", "python", "go", "c", "css", "shell", "sql", "markup",
]);

/**
 * Biểu tượng cho một hàng trong cây.
 *
 * Bộ biểu tượng có hai mươi hình, không phải hai trăm: gộp theo *vai trò* của tệp (mã,
 * chữ, cấu hình, ảnh) thay vì theo từng phần mở rộng. Người ta quét cây bằng hình dạng
 * chung, và bốn hình dạng phân biệt được rõ hơn hai mươi hình gần giống nhau.
 */
export function fileIcon(path: string): IconName {
  const ext = extensionOf(path);
  if (["png", "jpg", "jpeg", "gif", "webp", "ico", "svg", "avif"].includes(ext)) return "image";
  if (["json", "yml", "yaml", "toml", "ini", "env", "lock"].includes(ext)) return "settings";
  if (["md", "markdown", "txt", "rst", "adoc"].includes(ext)) return "document";
  const lang = BY_EXTENSION[ext];
  return lang !== undefined && CODE_ICON.has(lang) ? "file-code" : "file";
}

export type TokenKind = "plain" | "comment" | "string" | "number" | "keyword";

export interface Token {
  kind: TokenKind;
  text: string;
}

interface Grammar {
  lineComment: string[];
  blockComment?: [string, string];
  quotes: string[];
  keywords: Set<string>;
}

const words = (list: string): Set<string> => new Set(list.split(" "));

const C_LIKE = "if else for while return break continue switch case default do try catch finally throw new class struct enum interface extends implements public private protected static final void int long float double bool char true false null";

const GRAMMARS: Record<string, Grammar> = {
  typescript: {
    lineComment: ["//"],
    blockComment: ["/*", "*/"],
    quotes: ['"', "'", "`"],
    keywords: words(
      "import export from as default const let var function return if else for of in while do break continue switch case new class extends implements interface type enum namespace declare async await yield try catch finally throw typeof instanceof void delete this super null undefined true false readonly public private protected static get set satisfies keyof infer never unknown any string number boolean object symbol",
    ),
  },
  rust: {
    lineComment: ["//"],
    blockComment: ["/*", "*/"],
    quotes: ['"'],
    keywords: words(
      "fn let mut const static struct enum trait impl for in while loop if else match return break continue use mod pub crate self super where as dyn ref move async await unsafe extern type box true false Self Some None Ok Err",
    ),
  },
  python: {
    lineComment: ["#"],
    quotes: ['"', "'"],
    keywords: words(
      "def class return if elif else for while in is not and or import from as pass break continue try except finally raise with yield lambda global nonlocal assert del async await True False None self",
    ),
  },
  go: {
    lineComment: ["//"],
    blockComment: ["/*", "*/"],
    quotes: ['"', "`"],
    keywords: words(
      "package import func var const type struct interface map chan go defer return if else for range switch case default break continue fallthrough select nil true false string int int64 float64 bool byte rune error",
    ),
  },
  c: {
    lineComment: ["//"],
    blockComment: ["/*", "*/"],
    quotes: ['"', "'"],
    keywords: words(`${C_LIKE} include define typedef sizeof unsigned signed const auto extern`),
  },
  json: { lineComment: [], quotes: ['"'], keywords: words("true false null") },
  css: { lineComment: [], blockComment: ["/*", "*/"], quotes: ['"', "'"], keywords: new Set() },
  shell: {
    lineComment: ["#"],
    quotes: ['"', "'"],
    keywords: words("if then elif else fi for while do done case esac function return export local set echo cd exit"),
  },
  yaml: { lineComment: ["#"], quotes: ['"', "'"], keywords: words("true false null yes no") },
  toml: { lineComment: ["#"], quotes: ['"', "'"], keywords: words("true false") },
  sql: {
    lineComment: ["--"],
    blockComment: ["/*", "*/"],
    quotes: ["'", '"'],
    keywords: words("select from where insert into values update set delete create table drop alter index join left right inner outer on group by order limit offset and or not null as distinct"),
  },
};

const isWordChar = (ch: string): boolean => /[A-Za-z0-9_$]/.test(ch);
const isDigit = (ch: string): boolean => ch >= "0" && ch <= "9";

/**
 * Tô một tệp, trả về **từng dòng một mảng token**.
 *
 * Chú thích khối băng qua nhiều dòng, nên không thể tô từng dòng độc lập — máy quét chạy
 * trên cả tệp rồi mới cắt theo `\n`. Cắt sau cũng là chỗ duy nhất biết một token dài
 * thuộc về mấy dòng, nên khung xem không phải ghép lại.
 */
export function highlight(text: string, lang: string | null): Token[][] {
  const grammar = lang === null ? undefined : GRAMMARS[lang];
  if (!grammar) return text.split("\n").map((line) => [{ kind: "plain", text: line } as Token]);

  const runs: Token[] = [];
  const push = (kind: TokenKind, value: string) => {
    if (value === "") return;
    const last = runs[runs.length - 1];
    if (last && last.kind === kind) last.text += value;
    else runs.push({ kind, text: value });
  };

  let at = 0;
  while (at < text.length) {
    const rest = text.slice(at);
    const ch = text[at]!;

    const line = grammar.lineComment.find((mark) => rest.startsWith(mark));
    if (line !== undefined) {
      const end = text.indexOf("\n", at);
      const stop = end === -1 ? text.length : end;
      push("comment", text.slice(at, stop));
      at = stop;
      continue;
    }

    const block = grammar.blockComment;
    if (block && rest.startsWith(block[0])) {
      const end = text.indexOf(block[1], at + block[0].length);
      const stop = end === -1 ? text.length : end + block[1].length;
      push("comment", text.slice(at, stop));
      at = stop;
      continue;
    }

    if (grammar.quotes.includes(ch)) {
      let cursor = at + 1;
      while (cursor < text.length) {
        const here = text[cursor]!;
        // Dấu `\` nuốt ký tự kế tiếp, kể cả dấu nháy đóng. Không xử lý chỗ này thì
        // `"\""` kết thúc chuỗi sớm một ký tự và cả phần còn lại của tệp đổi màu.
        if (here === "\\") cursor += 2;
        else if (here === ch) { cursor += 1; break; }
        // Chuỗi một dòng không được phép băng qua `\n`: một dấu nháy lẻ trong chú thích
        // tiếng Việt (chữ "đừng") sẽ nhuộm nốt phần còn lại của tệp nếu ta cho phép.
        else if (here === "\n" && ch !== "`") break;
        else cursor += 1;
      }
      push("string", text.slice(at, cursor));
      at = cursor;
      continue;
    }

    if (isDigit(ch) && !isWordChar(text[at - 1] ?? "")) {
      let cursor = at;
      while (cursor < text.length && /[0-9a-fA-FxX_.]/.test(text[cursor]!)) cursor += 1;
      push("number", text.slice(at, cursor));
      at = cursor;
      continue;
    }

    if (isWordChar(ch)) {
      let cursor = at;
      while (cursor < text.length && isWordChar(text[cursor]!)) cursor += 1;
      const word = text.slice(at, cursor);
      push(grammar.keywords.has(word) ? "keyword" : "plain", word);
      at = cursor;
      continue;
    }

    push("plain", ch);
    at += 1;
  }

  const lines: Token[][] = [[]];
  for (const run of runs) {
    const pieces = run.text.split("\n");
    pieces.forEach((piece, index) => {
      if (index > 0) lines.push([]);
      if (piece !== "") lines[lines.length - 1]!.push({ kind: run.kind, text: piece });
    });
  }
  return lines;
}
