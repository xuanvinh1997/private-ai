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

```
cd harness/ui && npm install
cd .. && cargo test          # lõi
npm run tauri dev --prefix ui
```

## Trạng thái

**v0.1 và v0.5 xong; v1.0 còn hai mục chặn bởi phần cứng.** 233 test xanh, clippy 0 cảnh báo.

Hai mươi tool: tệp (`read` `write` `edit` `glob` `grep`), lệnh (`bash` `job_output`
`job_kill` `job_list`), terminal PTY bền (sáu tool), mã nguồn (`symbol_search` `outline`
`lsp`), kế hoạch (`todo_write`), và giao việc (`task`).

Cộng **dự án** (mở nhiều repo, mỗi repo có phiên riêng), **trình duyệt mã nguồn**,
vòng lặp turn/step, sổ tay phiên trên SQLite, hai adapter mô hình, giao diện có thẻ
diff, giam tiến trình trên macOS và Linux, MCP hai chiều, nén ngữ cảnh, skill, hook, chỉ mục mã
nguồn bằng tree-sitter, agent con, và cấu hình theo lớp.

Đóng gói: bản macOS build, đóng gói `.app`/`.dmg` và chạy được — xem
[hướng dẫn đóng gói](docs/PACKAGING.md). Cấu hình Windows và Linux đã viết nhưng **chưa
từng chạy**.

Còn lại: giam tiến trình trên Windows. Nó chặn bởi phần cứng chứ không bởi thời gian —
một provider sandbox chưa từng chạy thật mà báo là đang giam đúng là cái thất bại mà crate
đó sinh ra để tránh, nên nó báo `Enforcement::None` kèm lý do cho tới khi có máy để thử. Trên Linux và Windows, `bash` hiện chạy với
đầy đủ quyền của người dùng và thứ duy nhất đứng giữa là hộp thoại duyệt — hộp thoại đó
nói đúng điều này thay vì nói chung chung. Xem [lộ trình](docs/ROADMAP.md).

Có một bài kiểm chứng chạy với mô hình thật, mang `#[ignore]` vì nó cần một máy chủ đang
chạy:

```
PAI_MODEL=<mô hình> cargo test -p pai-app --test live_ollama -- --ignored --nocapture
```

Nó kiểm thứ mà 196 bài kia không kiểm được: schema tool ta phát ra có đủ để một mô hình
thật quyết định gọi tool không, tham số nó sinh ra có parse được không, và kết quả có
quay lại được vào lượt sau không.
