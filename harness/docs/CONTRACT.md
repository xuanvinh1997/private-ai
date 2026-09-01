# Hợp đồng đợt "dự án hai loại, provider, MCP"

Tài liệu này tồn tại vì đợt việc này được làm song song bởi nhiều người. Nó nói ba thứ:
ai sở hữu tệp nào, hình dạng dữ liệu đi qua dây, và những luật không được phá.

## Ai sở hữu cái gì

| Vùng | Chủ | Không ai khác được sửa |
|---|---|---|
| `crates/pai-project/` | dự án + clone | ✓ |
| `crates/pai-rag/` | thư viện tài liệu | ✓ |
| `crates/pai-index/` | đồ thị mã nguồn | ✓ |
| `crates/pai-providers/`, `crates/pai-agent/src/driver.rs` | provider | ✓ |
| `crates/pai-mcp/` | MCP | ✓ |
| `crates/pai-agent/src/skills/` + `skills/` (dữ liệu) | skill biểu đồ | ✓ |
| `ui/src/components/**`, `ui/src/lib/**` | giao diện | chia theo tệp, xem prompt |
| `Cargo.toml` gốc, `app/**`, `docs/**` | tích hợp | ✓ |

`app/src/protocol.rs` và `ui/src/lib/protocol.ts` **đã viết xong** cho đợt này. Cần thêm
trường thì nói ra chứ đừng tự sửa: hai tệp đó là chỗ duy nhất hai đầu gặp nhau, và một
thay đổi một phía là một lỗi chỉ lộ ra lúc chạy.

## Luật giữ nguyên từ trước

1. **Lọc tool hai tầng** — kiểm quyền lúc liệt kê *và* lúc gọi, sau khi gỡ tên wire.
2. **Ghim tham số workspace** — bỏ khỏi schema và ghi đè lúc gọi, không điền mặc định.
3. **Đường dẫn được bảo vệ bị giấu khỏi cả danh sách**, không chỉ bị chặn đọc.
4. **Duyệt là fail-closed**; **hook là fail-open**. Hook đại diện một tệp cấu hình, người
   duyệt đại diện một con người đang ngồi đó.
5. **Từ chối là văn bản, không phải lỗi.** Một exception kết thúc lượt trong im lặng.
6. **Guard đơn điệu**: chỉ `Deny` hoặc bỏ phiếu trắng, không có nhánh `Allow`.
7. **Nội dung từ ngoài vào được đóng khung là không đáng tin** — tài liệu người dùng nạp
   lên và kết quả tool MCP đều thuộc nhóm này.
8. Bình luận và chuỗi hiển thị **bằng tiếng Việt**, và giải thích **vì sao**, không phải
   *cái gì*.
9. `cargo fmt --all` và `cargo clippy --workspace --all-targets -- -D warnings` phải sạch.

## Kiểm chứng

Mỗi vùng tự chạy test của mình bằng `CARGO_TARGET_DIR` riêng để không giành khoá:

```sh
CARGO_TARGET_DIR=target-agents/<ten> cargo test -p <crate>
```

Không chạy `cargo test --workspace` — đó là việc của bước tích hợp.
