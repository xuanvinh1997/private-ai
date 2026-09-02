# adobe-cmap-parser — bản vá tại chỗ

Bản sao `adobe-cmap-parser 0.4.1` (MIT, Jeff Muizelaar,
<https://github.com/jrmuizel/adobe-cmap-parser>) với **đúng một** thay đổi trong
`src/lib.rs`, được nối vào cây qua `[patch.crates-io]` ở `harness/Cargo.toml`.

## Vì sao phải vá

`pdf-extract` đọc bảng `ToUnicode` của mỗi font qua crate này. Bộ lexer gốc coi một
operator là **một chuỗi ký tự alpha**, nên nó dừng ngay ở dấu `-` đầu tiên:

```postscript
/CMapName LiberationSerif-cmap def
```

`LiberationSerif` khớp; `-cmap` thì không khớp token nào, `file()` ngừng lặp, và
`get_unicode_map` trả về **`Ok(HashMap::new())`** — một bảng rỗng, không lỗi, không
hoảng loạn. `pdf-extract` sau đó decode mọi glyph thành chuỗi rỗng và trả về một tài
liệu **toàn khoảng trắng**, vẫn là `Ok`.

Triệu chứng ở phía người dùng: một quyển sách 3623 trang nạp "thành công" và cho 0
đoạn. Mọi PDF sinh bởi Calibre, LibreOffice hay bất kỳ bộ sinh nào đặt tên CMap có
dấu gạch ngang — tức là phần lớn PDF ngoài đời — đều rơi vào đây.

`0.4.1` là bản mới nhất trên crates.io tính đến tháng 09/2026, và `pdf-extract 0.12`
vẫn hỏng y hệt: không có đường vòng nào ở phía trên.

## Bản vá

`operator()` nhận đúng bộ **regular character** của PostScript — mọi byte trừ khoảng
trắng và tám ký tự delimiter `()<>[]{}/%`. Đây là luật trong PLRM 3rd ed. §3.1, chứ
không phải một chỗ nới rộng cho vừa một tệp: `-`, `.`, `+`, `_` và chữ số đều là ký
tự hợp lệ trong một tên PostScript.

Thứ tự trong `value()` không đổi, nên phép vá này **không** nuốt mất token nào đang
chạy đúng: `integer()` và `number()` được thử trước `operator()`, còn delimiter thì
vẫn mở token của riêng chúng.

Kèm theo là một bài kiểm trong `src/lib.rs` khoá đúng CMap đã gây ra chuyện này.
