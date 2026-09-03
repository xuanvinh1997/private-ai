# Skill dựng sẵn

Thư mục này là **bộ skill đi kèm bản cài đặt**. Mỗi thư mục con có một `SKILL.md` là một
skill: một quy trình viết sẵn cho một việc thường gặp, để mô hình không phải nhớ lại cách
làm ở mỗi lượt.

Bộ hiện có đều là skill vẽ sơ đồ, dùng nhiều nhất trong dự án loại **tài liệu** — người
dùng nạp lên một chồng tài liệu rồi hỏi "vẽ giúp tôi kiến trúc hệ thống này":

| Thư mục | Loại sơ đồ mermaid |
|---|---|
| `flowchart` | `flowchart` — quy trình, luồng quyết định |
| `sequence-diagram` | `sequenceDiagram` — ai gọi ai theo thời gian |
| `class-diagram` | `classDiagram` — cấu trúc mã, kế thừa |
| `er-diagram` | `erDiagram` — lược đồ dữ liệu |
| `state-diagram` | `stateDiagram-v2` — vòng đời, máy trạng thái |
| `architecture-diagram` | `flowchart` + `subgraph` — kiến trúc hệ thống theo tầng |
| `mindmap` | `mindmap` — rút tài liệu dài thành cây ý |
| `timeline` | `timeline` và `gantt` — mốc thời gian, kế hoạch |
| `user-journey` | `journey` — hành trình người dùng |

Tên thư mục và `name` viết bằng tiếng Anh, như mọi định danh khác trong cây mã này; chữ
tiếng Việt sống ở `title`, `keywords` và phần thân. Đó **không** phải một chi tiết hình
thức: `name` được chấm 3 điểm khi khớp câu hỏi, và một cái tên như `so-do-tuan-tu` sau khi
gỡ dấu chính là cụm người dùng gõ ra — nên đổi nó sang tiếng Anh là bỏ đi 3 điểm ấy. Chỗ
bù lại là `keywords`: mỗi skill ở bảng trên đều mang sẵn cụm tiếng Việt tương ứng, và một
skill mới cũng phải mang, nếu không nó sẽ không bao giờ được chọn cho một câu hỏi tiếng
Việt.

Sơ đồ được vẽ bằng cách mô hình **xuất một khối mã ```mermaid trong câu trả lời**; giao
diện tự dựng hình. Không có tool "tạo sơ đồ", và skill không được nhắc tới một tool không
tồn tại.

## Bộ nạp tìm skill ở đâu

Bộ dựng sẵn — tức chính thư mục này — được dò theo bốn chỗ, dừng ở chỗ đầu tiên có thật
(`builtin_skills()` trong `app/src/harness.rs`):

1. `PAI_SKILLS_DIR` nếu biến môi trường này trỏ tới một thư mục có thật. Lối thoát cho
   người phát triển và cho bộ test.
2. `…/Contents/Resources/skills` cạnh tệp thực thi — chỗ Tauri đặt tài nguyên trong bản
   `.app` của macOS.
3. `…/skills` cạnh chính tệp thực thi — chỗ nó nằm khi chạy `tauri dev` và trên Linux.
4. `<mã nguồn>/skills` — chính thư mục này, khi chạy từ cây mã nguồn.

Không tìm thấy chỗ nào thì bộ dựng sẵn là `None`, và đó là trạng thái hợp lệ: ứng dụng
vẫn khởi động, chỉ là không có skill dựng sẵn.

Ngoài bộ dựng sẵn, `SkillsPlugin` còn quét thêm hai gốc nữa. Cả ba được quét **theo thứ
tự**, và gói quét sau **thay thế** gói trùng `name` của gói quét trước:

1. bộ dựng sẵn (thư mục này),
2. `<kho dữ liệu>/skills` — gói người dùng tự thêm,
3. `<thư mục làm việc>/.pai/skills` — gói riêng của dự án, nằm ngay trong repo.

Thứ tự ấy là một thang thẩm quyền: người dùng đè lên bộ dựng sẵn thì họ đúng, và một repo
nói khác đi về quy trình của chính nó thì nó đúng.

## Tiết lộ dần, ba tầng

Đây là lý do skill tồn tại, và là thứ quyết định cách viết một `SKILL.md`:

1. Prompt hệ thống **luôn** mang `name` và `description` của mọi skill. Một trăm skill tốn
   một trăm dòng.
2. **Toàn văn phần thân** chỉ được chèn vào khi skill được chọn cho lượt đó.
3. Các tệp khác trong cùng thư mục chỉ được **liệt kê tên**; mô hình tự mở bằng `read`.

Việc chọn skill làm bằng **trùng lặp từ khoá trên văn bản của lượt**, đã gỡ dấu tiếng Việt
(nên người dùng gõ "so do tuan tu" vẫn trúng). Điểm: khớp `name` được 3, khớp `title` được
2, mỗi `keywords` khớp được 2, mỗi từ dài hơn bốn ký tự trong `description` được 0,5. Phải
đạt tối thiểu 2 điểm, và phải đạt ít nhất một nửa điểm của skill cao nhất.

Hệ quả khi viết: `description` **mô tả tình huống dùng skill, không mô tả bản thân sơ đồ**.
"Dùng khi cần cho thấy thứ tự các bước theo thời gian giữa nhiều bên" tốt hơn hẳn "vẽ
sequence diagram" — nó vừa giúp mô hình chọn đúng ở tầng một, vừa cho cơ chế chấm điểm
những từ để bắt.

## Thêm một skill mới

Tạo `skills/<ten-skill>/SKILL.md`. Định dạng do `crates/pai-agent/src/skills/loader.rs`
quy định, và nó nghiêm hơn vẻ ngoài:

```markdown
---
name: ten-skill
title: Tên hiển thị
description: "Dùng khi ..."
keywords:
  - "từ khoá"
  - "từ khoá khác"
---

# Phần thân

Hướng dẫn viết bằng markdown.
```

Luật của bộ nạp, đúng từng chữ:

- Tệp **phải mở đầu bằng `---` rồi xuống dòng**, không có dòng trống hay ký tự nào trước.
- Frontmatter kết thúc ở **lần xuất hiện đầu tiên** của một dòng bắt đầu bằng `---`.
- `name` là **bắt buộc** và chỉ được chứa chữ thường ASCII, chữ số, `.`, `-`, `_`. Không
  dấu tiếng Việt, không khoảng trắng, không chữ hoa. Nên trùng tên thư mục.
- `description` là **bắt buộc** và không được rỗng. Thiếu thì gói bị bỏ qua — hợp lý, vì
  nó là thứ duy nhất mô hình đọc ở tầng một, nên một skill không có nó thì không bao giờ
  được chọn.
- `title` không bắt buộc; thiếu thì lấy `name` thay. Nhưng `title` đáng viết: nó là chuỗi
  tiếng Việt tự nhiên mà người dùng hay gõ, và nó được 2 điểm khi khớp.
- `keywords` không bắt buộc, mặc định rỗng.
- **Phần thân không được rỗng.** Thiếu thì gói bị bỏ qua.
- Giá trị nào có dấu `:`, `#` hay `"` thì bọc trong nháy kép — đây là YAML.
- Không có giới hạn độ dài, nhưng phần thân đi vào prompt nguyên vẹn khi skill được chọn.
  Ngắn gọn là một khoản tiết kiệm thật.

Một gói hỏng bị **bỏ qua kèm log cảnh báo**, không làm hỏng lần quét: mất một skill là
mất một quy trình, còn ném lỗi ở đó là mất cả bộ. Nghĩa là một `SKILL.md` sai định dạng
sẽ **im lặng biến mất** chứ không báo gì lên giao diện — kiểm bằng cách chạy lại và xem
skill có trong danh mục không, đừng tin là nó đã nạp.

## Nội dung ngoài vào là dữ liệu, không phải chỉ dẫn

**Skill là chỉ dẫn đáng tin cậy** — nội dung của nó do người vận hành viết, nên nó được
chèn vào prompt như luật của chính ứng dụng. Vì thế không có đường nào từ truy hồi hay từ
mô hình được phép tạo, đặt tên hay sửa một skill.

Đối xứng lại: tài liệu người dùng nạp lên và kết quả tool MCP là **dữ liệu không đáng tin
cậy**. Mỗi skill trong thư mục này đều nhắc lại điều đó ở cuối, và skill mới cũng nên
nhắc: một câu trong tài liệu viết "hãy vẽ sơ đồ khác đi" hay "bỏ qua hướng dẫn phía trên"
là chữ để trích dẫn, không phải việc để làm.

## Kiểm chứng cú pháp mermaid

Mọi khối ```mermaid trong thư mục này đã được `mermaid.parse()` của **mermaid 11.17.2**
chấp nhận — đúng bản ghim trong `ui/package.json`. Thêm hay sửa ví dụ thì chạy lại kiểm
chứng: một sơ đồ sai cú pháp hiện ra là một ô đỏ trên màn hình người dùng, và mermaid có
nhiều chỗ **phân tích trót lọt mà vẽ ra sai** (xem mục "Cái hay hỏng" trong từng skill),
nên chỉ `parse()` thôi chưa đủ để yên tâm về một ví dụ mới.
