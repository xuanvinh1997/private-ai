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

## v1.1 — dự án hai loại, provider, MCP

Đợt này trả lời một câu hỏi mà bản v1.0 né: ứng dụng chỉ làm được **một** việc, trên
**một** máy chủ mô hình, với **một** bộ tool dựng sẵn. Ba cái "một" đó là ba trần.

| # | Việc | Nơi | Cỡ |
|---|---|---|---|
| 24 | Dự án có loại: mã nguồn / tài liệu | `pai-project` | **M** ✅ |
| 25 | Clone từ Git, có tiến trình và huỷ được | `pai-project` | **M** ✅ |
| 26 | Thư viện tài liệu: nạp nhiều định dạng, cắt đoạn, nhúng vector, tìm lai ghép | `pai-rag` | **XL** ✅ |
| 27 | Đồ thị bộ nhớ mã nguồn: cạnh gọi/nhập/chứa/kế thừa | `pai-index` | **L** ✅ |
| 28 | Bộ skill vẽ chín loại sơ đồ, và giao diện dựng mermaid | `skills/`, `ui` | **M** ✅ |
| 29 | Nhiều provider mô hình, đổi được lúc đang chạy | `pai-providers` | **L** ✅ |
| 30 | Quản lý server MCP trong ứng dụng, kèm danh mục dựng sẵn | `pai-mcp` | **L** ✅ |

Ba luật mới, và cả ba đều là luật về **sự trung thực**, cùng họ với `Enforcement` của
`pai-sandbox`:

- Đồ thị mã nguồn phân giải lời gọi **theo tên**, không phân tích kiểu. Cạnh là suy đoán,
  và cả tool lẫn giao diện phải nói ra điều đó. Một đồ thị trình bày như sự thật khiến
  người đọc kết luận sai và tự tin.
- Thư viện tài liệu **vẫn dùng được khi chưa nhúng xong**: tìm bằng từ khoá chạy ngay, và
  `LibraryStats` nói rõ phần ngữ nghĩa còn thiếu gì. "Chưa xong" và "hỏng" là hai trạng
  thái, và gộp chúng lại là dạy người dùng bỏ cuộc.
- Khoá API **không bao giờ đi ngược ra giao diện**. Giao diện chỉ biết `hasKey`, và để
  trống ô khoá nghĩa là *giữ nguyên*, không phải *xoá*.

## Cái gì của bản Python đi tiếp, cái gì dừng lại

**Đi tiếp** — ranh giới bảo mật (lọc hai tầng, ghim workspace, đường dẫn được bảo vệ,
khung cảnh báo nội dung không đáng tin), memory cá nhân, skill, danh mục mô hình + GPU
lease, và bộ token thiết kế.

**Dừng lại** — LightRAG và đồ thị thực thể do mô hình sinh (đồ thị AST từ tree-sitter
tốt hơn hẳn cho mã nguồn), MarkItDown và tầng OCR, ASR, `graph_view`, và bốn tool
`artifacts.create_*`. Không cái nào phục vụ một coding agent, và chúng là toàn bộ lý do
phải giữ một sidecar Python.

> **Sửa lại ở v1.1:** thư viện tài liệu quay lại bằng Rust thuần trong tiến trình
> (`pdf-extract`, `zip` + `quick-xml`, SQLite/FTS, Qdrant HTTP và
> `BAAI/bge-reranker-v2-m3` ONNX chạy ngay trong tiến trình). Python sidecar và inference
> API cho reranker đã bị loại bỏ. Các định dạng chưa có bộ đọc Rust tiếp tục báo lỗi khả
> năng rõ ràng thay vì rơi ngầm về Python.

> Nói cách khác: đường chạy RAG hiện tại không còn cần Python; phần còn thiếu được biểu
> diễn thành lỗi khả năng rõ ràng thay vì một tiến trình fallback ẩn.

## Nợ còn lại của v1.1

Ba chỗ đã biết là chưa xong, viết ra để không ai phải phát hiện lại:

| Chỗ | Trạng thái |
|---|---|
| ~~`OllamaEmbedder` chưa gọi mạng thật lần nào~~ | **Xong.** `tests/embed_live.rs` chạy với Ollama thật: đúng số vector, đúng thứ tự qua ranh giới lô, và hai câu cùng nghĩa gần nhau hơn câu khác nghĩa. Đo trên `embeddinggemma`, 768 chiều. Bài tự bỏ qua khi không có máy chủ. `OpenAiEmbedder` vẫn chưa có đường tương đương — cần một khoá API. |
| ~~PDF có dấu tiếng Việt~~ | **Xong.** Bài mới in một PDF có font nhúng kèm `ToUnicode` bằng Chrome headless — cùng đường mà Word đi — rồi so từng cụm có dấu. Rút đúng cả `Đ` hoa lẫn năm dấu thanh. |
| ~~Cosine quét tuyến tính toàn bảng `vectors`~~ | **Đã đo, và ước lượng cũ sai theo hướng bi quan.** 100.000 đoạn × 768 chiều mất **53ms** ở bản release — không ai cảm thấy. `tests/cosine_scale.rs` khoá lại ngưỡng 500ms để bắt hồi quy về bậc độ lớn. Chỉ mục ANN chưa cần tới. |
| ~~`mcp-server-sqlite` trong danh mục~~ | **Xong.** Mục đã gỡ khỏi bảng: bản tham chiếu không còn trong `modelcontextprotocol/servers`, và một hàng danh mục cài phần mềm đã bỏ hoang còn tệ hơn không có hàng nào. |
| ~~Danh mục MCP chỉ dựng được server `stdio`~~ | **Xong.** `CatalogEntry::url` cộng `McpCatalogEntry.url` trên dây; GitHub quay số thẳng tới endpoint remote, không cần Docker. Bí mật của mục từ xa đi vào **header**, không vào biến môi trường. |

Về giam mạng: từ nay có `Policy::deny_network`, **tắt theo mặc định** — mặc định ấy không
đổi, vì cấm mạng luôn luôn thì `cargo` và `npm` hỏng. Chỉ macOS giam thật; Linux và Windows
khai `network_confinable() == false` thay vì nhận cờ rồi không dựng gì. Windows vẫn chưa
giam được gì cả, và đó là món nợ lớn nhất còn lại của crate này.

Và một giới hạn **cố ý**, không phải nợ: đồ thị mã nguồn phân giải lời gọi theo tên, không
phân tích kiểu. Gọi qua biến, qua trait object hay qua con trỏ hàm không sinh cạnh nào. Cả
tool lẫn giao diện đều nói ra điều đó ở chỗ người đọc thấy được.

## Rủi ro đã biết

| Rủi ro | Cách xử lý |
|---|---|
| Vượt biên IPC của Tauri đắt hơn signal của Qt rất nhiều | Gộp token ở **phía Rust**, 16–33 ms mỗi lần gửi. Dùng `Channel`, không dùng `emit` |
| Không có `rustup` trên máy này | Chỉ build được cho máy chủ nhà. Cần `rustup` trước khi cross-build |
| `TypeId` không ổn định qua ranh giới dylib | Không nạp plugin bằng dylib. Bên thứ ba đi qua MCP |
| Drop của Rust chạy xuôi, Cordis dọn LIFO | `EffectScope::dispose` đảo thứ tự tường minh |
| Bộ gõ tiếng Việt gửi Enter để chốt từ | Guard `isComposing` trong composer |
| HTML5 drop không cho đường dẫn tuyệt đối | Dùng `onDragDropEvent` của Tauri |
