# i18n

Hai ngôn ngữ: **`en` là mặc định**, `vi` là bản dịch. Chuỗi hiện có trong mã nguồn đang là
tiếng Việt — chúng chuyển thành trường `vi`, và trường `en` là chữ **viết mới**, không
phải dịch máy ngược lại.

## Dùng

```tsx
import { S, t } from "../lib/i18n";

<button>{t(S.chat.send)}</button>
<IconButton icon="trash" label={t(S.common.delete)} />
<p>{t(S.docs.indexed, { n: files().length })}</p>
```

`t()` đọc signal `locale()`, nên gọi thẳng trong JSX là đủ để chữ đổi ngay khi người dùng
đổi ngôn ngữ. Ngoài JSX (trong hàm sự kiện, khi dựng `toast`, khi ném lỗi) cũng gọi bình
thường — chuỗi được tính tại thời điểm gọi.

Số ít / số nhiều: `tn(n, S.x.oneFile, S.x.manyFiles)`. `{n}` có sẵn cho cả hai dạng.

## Viết chuỗi

Mỗi khu vực một tệp trong `strings/`. Một thông điệp là `{ en, vi }`:

```ts
export const chat = {
  send: { en: "Send", vi: "Gửi" },
  emptyHint: { en: "Ask anything", vi: "Hỏi bất cứ điều gì" },
  indexed: { en: "{n} files indexed", vi: "Đã lập chỉ mục {n} tệp" },
} satisfies Record<string, Msg | Record<string, Msg>>;
```

- Khoá là camelCase, đặt tên theo **vai trò**, không theo nội dung: `emptyHint`, không
  phải `askAnything`. Đổi chữ thì không phải đổi khoá.
- Gom được thì lồng một cấp: `form: { name: …, url: … }`. Sâu hơn hai cấp thì tách tệp.
- Không nối chuỗi: `"Xoá " + n + " tệp"` sai thứ tự ở ngôn ngữ khác. Dùng `{n}`.
- Chuỗi dùng ở từ hai khu vực trở lên thì chuyển sang `strings/common.ts`, không chép.

## Luật chữ tiếng Anh

Phần chữ `en` viết ngắn hơn bản tiếng Việt hiện tại, theo ba luật:

1. **Nhan đề ≤ 2 từ.** Tiêu đề trang, nhãn tab, nhãn nút, tiêu đề nhóm, tiêu đề hộp
   thoại, tiêu đề cột. `"Add provider"`, không phải `"Add a new provider"`.
2. **Câu mô tả ≤ 5 từ.** Câu dưới tiêu đề, chú thích ô nhập, trạng thái rỗng, gợi ý.
   `"Colors and chat layout"`, không phải `"Bảng màu và cách hội thoại được vẽ ra"` dịch
   nguyên. Bỏ mạo từ và động từ nối khi câu vẫn đọc được.
3. **Ưu tiên biểu tượng.** Nút phụ, nút lặp lại trong danh sách, nút trong thanh công cụ
   → `IconButton` với `aria-label`, bỏ chữ hiện. Xem `components/Icon.tsx` cho bộ có sẵn;
   thiếu hình thì thêm một `path` vào cùng lưới 24×24, đừng kéo thư viện icon về.

Ba ngoại lệ, **không** cắt chữ:

- `aria-label`, `title`, và chữ chỉ trình đọc màn hình thấy: đó là tên truy cập được của
  điều khiển, phải nói đủ việc nút làm (`"Close changes panel"`). Vẫn giữ gọn, nhưng
  đúng nghĩa quan trọng hơn đếm từ.
- Câu cảnh báo trước một hành động **không lấy lại được** (xoá, ghi đè, chạy lệnh): nói
  đủ hậu quả.
- Thông báo lỗi: nói được cái gì hỏng và làm gì tiếp.

Bản `vi` giữ giọng hiện có của ứng dụng: viết thường sau chữ đầu, không chấm câu ở nhãn
ngắn, không dùng chữ Anh khi tiếng Việt có từ (`Máy chủ`, không `Provider`).

## Từ vựng chung

| en | vi | ghi chú |
|---|---|---|
| Session | Phiên | một lượt hội thoại |
| Chat | Hội thoại | không dùng "trò chuyện" |
| Project | Dự án | thư mục mã nguồn đang mở |
| Provider | Máy chủ | nơi cung cấp mô hình |
| Model | Mô hình | |
| Docs | Tài liệu | thư viện tài liệu, không phải tệp mã |
| Changes | Thay đổi | bảng diff |
| Tool | Tool | giữ nguyên, đã là thuật ngữ |
| Hook | Hook | giữ nguyên |
| Embedding | Nhúng | |
| Rerank | Xếp hạng lại | |

## Không đụng vào

Bình luận trong mã (`//`, `/** */`) vẫn viết tiếng Việt — đó là chữ cho người sửa mã, không
phải cho người dùng. Tên biến, khoá `localStorage`, giá trị trong protocol, khoá test, và
chuỗi trong `lib/fixtures/` + `lib/demo.ts` chỉ đổi khi nó thật sự hiện ra trên màn hình.
