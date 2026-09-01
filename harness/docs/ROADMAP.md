# Lộ trình

Thang công sức: **S** ≈ 1–3 ngày · **M** ≈ 1–2 tuần · **L** ≈ 3–5 tuần · **XL** ≈ 6+ tuần.

## Nguyên tắc thứ tự

Năm thứ tự không được đảo:

1. **Sổ tay phiên trước vòng lặp agent.** Vòng lặp dựng lịch sử mô hình *từ sổ*; viết
   ngược lại là dựng một nguồn sự thật thứ hai rồi phải xoá đi.
2. **Sổ đăng ký tool và đường ống canh gác trước tool đầu tiên.** Bộ test bảo mật là
   bản đặc tả; port `tests/test_mcp.py` sang Rust **trước khi** viết tool nào.
3. **`read` trước `edit`.** Chính sách đọc-trước-khi-sửa là một gate trên sự kiện `fs/*`,
   không phải một trường trong schema tool.
4. **Approval trước `bash`.** Một tool thi hành lệnh mà chưa có đường hỏi người dùng thì
   không có chế độ an toàn để mặc định vào.
5. **Sandbox sau `bash`, không đồng thời.** Giam tiến trình là bài toán riêng cho từng
   hệ điều hành; gộp vào cùng lúc là hai việc khó chồng lên nhau.

## v0.1 — MVP chạy được đầu-cuối

Mục tiêu: gõ một câu, mô hình đọc được repo, sửa được tệp, và thay đổi hiện ra dạng diff.

| # | Việc | Crate | Cỡ |
|---|---|---|---|
| 1 | Lõi plugin: seam, event bus, effect scope | `pai-core` | **M** ✅ |
| 2 | Vỏ Tauri + hợp đồng sự kiện + vỏ giao diện | `pai-app`, `ui` | **S** ✅ |
| 3 | Sổ tay phiên chỉ-ghi-thêm + `derive_messages` + SQLite | `pai-session` | **M** ✅ |
| 4 | Từ vựng stream + adapter Ollama và OpenAI-compatible | `pai-llm` | **M** ✅ |
| 5 | Sổ đăng ký tool có phạm vi + guard + hook + approval | `pai-tools` | **M** ✅ |
| 6 | `read`, `write`, `edit`, `glob`, `grep` | `pai-fs` | **M** ✅ |
| 7 | `bash` (chạy nền + `job_*`) | `pai-shell` | **M** ✅ |
| 8 | `todo_write` | `pai-tools` | **S** ✅ |
| 9 | Vòng lặp turn/step | `pai-agent` | **M** ✅ |
| 10 | Giao diện: transcript streaming, thẻ tool, khối diff, danh sách phiên | `ui` | **M** ✅ |

**Bộ tool v0.1: mười cái.** `read` `write` `edit` `glob` `grep` `bash` `job_output`
`job_kill` `job_list` `todo_write`.

v0.1 **đã xong**: 113 test xanh, clippy sạch, cửa sổ mở được và cây plugin dựng đúng.
Cái còn thiếu để dùng thật là sandbox (mục 11) — cho tới lúc đó, `bash` chạy với đầy đủ
quyền của người dùng và chỉ được chặn bởi hộp thoại duyệt.

## v0.5 — dùng được hằng ngày

| # | Việc | Crate | Cỡ |
|---|---|---|---|
| 11 | Sandbox: seatbelt macOS | `pai-sandbox` | **L** ✅ |
| 11b | Sandbox: landlock Linux | `pai-sandbox` | **M** ✅ |
| 12 | Client MCP cho server bên thứ ba (`rmcp` 3.2) | `pai-mcp` | **M** ✅ |
| 13 | Phơi sổ đăng ký ra ngoài dưới dạng một server MCP | `pai-mcp` | **M** ✅ |
| 14 | Nén ngữ cảnh (`replace`, không xoá) | `pai-agent` | **M** ✅ |
| 15 | Skill: SKILL.md + tiết lộ dần ba tầng | `pai-agent` | **S** ✅ |
| 16 | Hook trước tool, cấu hình được | `pai-hooks` | **S** ✅ |
| 17 | Chỉ mục mã nguồn: tree-sitter + FTS5, tăng dần | `pai-index` | **L** ✅ |
| 18 | Cấu hình theo lớp (nền + bản vá của người dùng) | `pai-core` | **M** ✅ |

## v1.0

| # | Việc | Cỡ |
|---|---|---|
| 19 | Terminal PTY bền (6 tool) | **L** ✅ |
| 20 | Subagent / task | **L** ✅ |
| 21 | LSP | **M** ✅ |
| 22 | Sandbox Windows (restricted token) | **L** — cần máy Windows để kiểm chứng |
| 23 | Đóng gói và ký: macOS xong, hai nền tảng kia là cấu hình chưa chạy | **M** ◐ |

## Cái gì của bản Python đi tiếp, cái gì dừng lại

**Đi tiếp** — ranh giới bảo mật (lọc hai tầng, ghim workspace, đường dẫn được bảo vệ,
khung cảnh báo nội dung không đáng tin), memory cá nhân, skill, danh mục mô hình + GPU
lease, và bộ token thiết kế.

**Dừng lại** — LightRAG và đồ thị thực thể do mô hình sinh (đồ thị AST từ tree-sitter
tốt hơn hẳn cho mã nguồn), MarkItDown và tầng OCR, ASR, `graph_view`, thư viện tài liệu,
và bốn tool `artifacts.create_*`. Không cái nào phục vụ một coding agent, và chúng là
toàn bộ lý do phải giữ một sidecar Python.

> Nói cách khác: **coding agent bằng Rust thuần là khả thi; port nguyên Private AI sang
> Rust thuần thì không.** Việc thu hẹp phạm vi chính là thứ mở đường cho bản Rust thuần.

## Rủi ro đã biết

| Rủi ro | Cách xử lý |
|---|---|
| Vượt biên IPC của Tauri đắt hơn signal của Qt rất nhiều | Gộp token ở **phía Rust**, 16–33 ms mỗi lần gửi. Dùng `Channel`, không dùng `emit` |
| Không có `rustup` trên máy này | Chỉ build được cho máy chủ nhà. Cần `rustup` trước khi cross-build |
| `TypeId` không ổn định qua ranh giới dylib | Không nạp plugin bằng dylib. Bên thứ ba đi qua MCP |
| Drop của Rust chạy xuôi, Cordis dọn LIFO | `EffectScope::dispose` đảo thứ tự tường minh |
| Bộ gõ tiếng Việt gửi Enter để chốt từ | Guard `isComposing` trong composer |
| HTML5 drop không cho đường dẫn tuyệt đối | Dùng `onDragDropEvent` của Tauri |
