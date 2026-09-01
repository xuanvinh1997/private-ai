# Luật của repo

Những thứ dưới đây đúng ở mọi crate và mọi màn hình. Chúng không phải quy ước cho đẹp —
mỗi cái tương ứng với một kiểu hỏng đã xảy ra thật, hoặc một kiểu hỏng đắt tới mức không
đáng để thử.

## Ranh giới tin cậy

1. **Lọc tool hai tầng** — kiểm quyền lúc liệt kê *và* lúc gọi, sau khi gỡ tên wire. Chỉ
   kiểm một tầng nghĩa là một tên gọi thẳng đi vòng qua bộ lọc.
2. **Ghim tham số workspace** — bỏ khỏi schema và ghi đè lúc gọi. Không điền mặc định:
   một mặc định là thứ mô hình ghi đè được.
3. **Đường dẫn được bảo vệ bị giấu khỏi cả danh sách**, không chỉ bị chặn đọc. Một tệp
   hiện ra trong `glob` là một tệp mô hình biết là có.
4. **Duyệt là fail-closed; hook là fail-open.** Hook đại diện một tệp cấu hình, người duyệt
   đại diện một con người đang ngồi đó.
5. **Guard đơn điệu**: chỉ `Deny` hoặc bỏ phiếu trắng, không có nhánh `Allow`. Có `Allow`
   thì thứ tự đăng ký biến một lần từ chối thành một lần cho phép.
6. **Nội dung từ ngoài vào được đóng khung là không đáng tin** — tài liệu người dùng nạp
   lên, kết quả tool MCP, và mọi thứ đọc từ đĩa của người khác.
7. **Từ chối là văn bản, không phải lỗi.** Một exception kết thúc lượt trong im lặng.

## Trung thực về năng lực

8. **Báo cáo sự thật, không hứa.** `Enforcement` nói vòng giam đang ở mức nào chứ không nói
   nó an toàn; đồ thị mã nguồn nói cạnh là suy đoán theo tên; `LibraryStats` nói vì sao
   phần ngữ nghĩa chưa sẵn sàng. Một thứ trình bày như sự thật trong khi nó là phỏng đoán
   khiến người đọc kết luận sai **và tự tin**.
9. **"Chưa xong" và "hỏng" là hai trạng thái.** Gộp lại là dạy người dùng bỏ cuộc.
10. **Một quyền trông như đang mở mà sau nó không có gì** là kiểu nói dối tệ nhất một giao
    diện quyền hạn có thể làm.

## Dữ liệu người dùng

11. **Khoá API không bao giờ đi ngược ra giao diện.** Giao diện chỉ biết `hasKey`; ô trống
    lúc lưu nghĩa là *giữ nguyên*, không phải *xoá*.
12. **Kho người dùng gõ vào thì migrate, không dựng lại.** Chỉ mục thì ngược lại — nó dựng
    lại được từ nguồn. Mỗi kho phải chọn một hướng và viết lý do vào mã.
13. **Ghi cấu hình là ghi nguyên tử** (tệp tạm cùng thư mục rồi `rename`), và tệp chứa bí
    mật đặt `0600`.

## Hình dạng mã

14. Bình luận và chuỗi hiển thị **bằng tiếng Việt**, và giải thích **vì sao** chứ không
    phải *cái gì*. Không viết bình luận thừa.
15. Không `unwrap()` trên đường chạy thật. Khoá `Mutex` nhiễm độc thì lấy lại mà dùng.
16. **Mã chết tệ hơn mã bị xoá.** Gỡ một màn hình thì gỡ cả chuỗi phía sau nó.
17. `app/src/protocol.rs` và `ui/src/lib/protocol.ts` là **một** hợp đồng ở hai tệp. Sửa
    một phía mà không sửa phía kia là một lỗi chỉ lộ ra lúc chạy.

## Kiểm chứng

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
npm run build --prefix ui        # gồm tsc --noEmit
```

Test phải kiểm chứng **hành vi**, không kiểm chứng rằng hàm tồn tại. Một bài test đáng giá
là bài mà tắt phần cài đặt đi thì nó đỏ — nếu không chắc, hãy thử tắt và xem.
