# Harness

Bản viết lại Private AI thành một **coding & working agent** chạy trên máy người dùng:
lõi Rust, vỏ Tauri, giao diện SolidJS.

Kiến trúc theo triết lý *everything is a plugin* của
[deepseek-harness](https://github.com/deepseek-ai/deepseek-harness). Không có lõi đặc
quyền: vòng lặp agent, bộ chuyển đổi mô hình, sổ tay phiên và sổ đăng ký tool đều là
plugin cắm vào cùng một cây, đều thay được từ cấu hình.

- [Kiến trúc](docs/ARCHITECTURE.md) — seam, sự kiện, đường ống tool, ranh giới tin cậy.
- [Lộ trình](docs/ROADMAP.md) — v0.1 đến v1.0, và cái gì của bản Python dừng lại ở đây.

## Vì sao viết lại thay vì port

Bản Python là một *knowledge agent*: đọc tài liệu, truy hồi, trả lời. Nó cố ý **không có
tool nào ghi được** — "chat may look, never touch". Một coding agent thì đảo ngược đúng
bất biến đó: nó phải sửa tệp và chạy lệnh. Đấy là thay đổi về hình dạng sản phẩm, không
phải về ngôn ngữ.

Việc thu hẹp phạm vi mới là thứ mở đường cho bản Rust thuần. LightRAG, MarkItDown và ASR
là toàn bộ lý do phải nuôi một sidecar Python, và không cái nào phục vụ một coding agent —
mã nguồn không cần OCR, còn đồ thị AST từ tree-sitter tốt hơn hẳn đồ thị thực thể do mô
hình sinh ra.

## Cây thư mục

```
crates/       Chín crate, chia theo họ khả năng như dsh
app/          Vỏ Tauri: lệnh invoke và kênh sự kiện. Cố tình mỏng
ui/           SolidJS + TypeScript + Tailwind v4
docs/         Kiến trúc và lộ trình
```

Mỗi họ khả năng tách ba vai: **định nghĩa seam**, **provider**, **consumer**. Đó là lý do
đổi một provider là đổi cả sản phẩm — trỏ hệ tệp và tiến trình con vào một sandbox từ xa
thì Bash, PTY và LSP đi theo, không cần fork provider nào.

## Chạy

Cần Rust 1.88+ và Node 20+.

```sh
cd harness/ui && npm install
cd ..                                   # phải đứng ở `harness/`
./ui/node_modules/.bin/tauri dev
```

Tauri CLI đi qua npm, không qua `cargo install tauri-cli` — nên `cargo tauri dev` **không
chạy** trừ khi bạn tự cài binary đó. Và lệnh phải chạy từ `harness/`: CLI tìm
`app/tauri.conf.json` từ thư mục hiện hành đi xuống.

Lần đầu mở, ứng dụng **không mở dự án nào** và trò chuyện chạy ngay. Trợ lý chưa đọc hay
sửa được tệp cho tới khi bạn mở một dự án — đó là có chủ ý; xem `docs/ARCHITECTURE.md`.

| Biến | Mặc định |
|---|---|
| `PAI_WORKSPACE` | *(không có)* — dự án mở sẵn. Bỏ trống thì mở lại cái gần nhất, hoặc không mở gì |
| `PAI_MODEL` | `qwen3:8b` — chỉ dùng để **gieo** hàng provider đầu tiên |
| `PAI_OLLAMA_URL` | `http://127.0.0.1:11434` — cũng chỉ để gieo |
| `PAI_EMBED_MODEL` | `nomic-embed-text` — cũng chỉ để gieo |
| `PAI_DATA_DIR` | `~/.private-ai` |
| `PAI_CONTEXT_WINDOW` | `32768` |
| `PAI_SKILLS_DIR` | *(tự dò)* — bộ skill dựng sẵn |
| `PAI_LOG` | `info` |

Sau lần gieo đầu, nhà cung cấp và mô hình được sửa **từ trong ứng dụng**; biến môi trường
không còn quyền gì. Hai nguồn cho cùng một giá trị thì một nguồn sẽ luôn là nguồn người ta
quên.

Chỉ xem giao diện, không cần lõi:

```sh
npm run dev --prefix ui     # rồi mở http://localhost:5173/?demo=1
```

## Trạng thái

**316 test xanh, clippy 0 cảnh báo, `tsc` sạch.**

Tool mà mô hình thấy, và **chúng phụ thuộc vào loại dự án đang mở**:

| Dự án | Tool |
|---|---|
| Mã nguồn | tệp (`read` `write` `edit` `glob` `grep`), lệnh (`bash` `job_*`), terminal PTY bền (6 tool), mã nguồn (`symbol_search` `outline` `code.graph` `code.trace` `code.overview` `lsp`), `todo_write`, `task` |
| Tài liệu | `docs.search` `docs.read` `docs.list`, `todo_write`, `task` |
| *(chưa mở dự án)* | `todo_write` |

Cộng tool từ server MCP ở mọi trạng thái. Danh sách rút ngắn ở hai dòng dưới không phải
thiếu sót: một thư viện tài liệu là một chồng tệp do người khác gửi tới, và cấp cho nó
`bash` là mở đúng cánh cửa không nên mở.

Ngoài ra: **dự án hai loại** (mã nguồn / tài liệu, clone được từ Git), **nhiều nhà cung
cấp mô hình** đổi được lúc đang chạy với **vai nhúng tách khỏi vai hội thoại**, **quản lý
server MCP** kèm danh mục dựng sẵn, thư viện tài liệu tìm lai ghép BM25 + vector, đồ thị bộ
nhớ mã nguồn, chín skill vẽ sơ đồ với mermaid dựng trong bản ghi, vòng lặp turn/step, sổ
tay phiên trên SQLite, giam tiến trình trên macOS và Linux, MCP hai chiều, nén ngữ cảnh,
hook, và cấu hình theo lớp.

Nợ còn lại và các giới hạn cố ý: `docs/ROADMAP.md`.
