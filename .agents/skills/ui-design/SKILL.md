---
name: ui-design
description: Ngôn ngữ thiết kế của Private AI (PySide6 + QSS) — token, lớp `class`, khuôn mẫu trang/card/hàng/dialog, luật rút gọn chữ và vòng lặp chụp ảnh kiểm chứng. Dùng khi tạo mới hoặc redesign bất kỳ view, dialog, widget nào trong src/private_ai/ui.
---

# Ngôn ngữ thiết kế Private AI

Một cửa sổ desktop, tiếng Việt, chạy hoàn toàn tại máy. Cảm giác cần đạt: **công cụ bàn
làm việc** — dày đặc thông tin, im lặng, một màu nhấn duy nhất. Không phải landing page.

## 1. Chín luật

1. **Không hardcode.** Màu qua `theme.token("accent-ink")`, khoảng cách qua `SPACE[...]`,
   không có số px hay mã hex nào viết thẳng trong view.
2. **Một điểm nhấn mỗi màn hình.** `accent` dành cho hành động chính và lựa chọn đang bật.
   Ba nút xanh trên một trang nghĩa là không nút nào quan trọng.
3. **Chữ lặp xuống mọi hàng → icon + tooltip.** `chat · vision · tools · thinking` lặp 20
   lần là một đoạn văn bản vô nghĩa; bốn glyph thì đọc được trong một nhịp mắt.
4. **Không hàng nào nhắc lại tên card.** Một card "Tìm kiếm web" không cần hàng "Bật" —
   công tắc nằm ngay dòng tiêu đề (`_group(title, hint, control)`).
5. **Hint chỉ nói điều tiêu đề chưa nói.** Không viết lại nhãn bằng câu dài hơn.
6. **Metadata hạng ba đi vào tooltip.** SHA, id, đường dẫn tuyệt đối, ngày giờ đầy đủ —
   đặt `setToolTip`, đừng chiếm một dòng.
7. **Số liệu có cột cố định.** Dung lượng/số đếm canh phải trong cột `setFixedWidth`, đơn
   vị hoặc nhãn chuyển thành icon, để mắt dò dọc được cả danh sách.
8. **Trạng thái = `StatusPip` + 1–2 từ**, không phải một câu.
9. **Mọi control chỉ có icon phải có `setToolTip` + `setAccessibleName`.** Nếu không đặt
   được tên, đó là dấu hiệu icon sai.

## 2. Token màu — `theme.token(key)`

`bg` `sidebar` `surface` `surface-soft` `surface-hover` · `ink` `text` `muted` `faint` ·
`line` `line-strong` · `accent` `accent-hover` `accent-soft` `accent-ink` `on-accent` ·
`success` `success-soft` `warn` `warn-soft` `danger` `danger-soft` · `shadow` `scrim`.

Hai theme `light`/`dark` khai báo **cùng bộ khóa** (test bắt buộc). Màu dữ liệu (node đồ
thị) lấy từ `graph_palette()`, không lấy từ bảng giao diện.

## 3. Chữ

Ladder rem trong `TYPE_SCALE`, nhân với root 14/15/18px theo `font_scale`. Chọn lớp, đừng
chọn size:

| `class` | dùng cho |
|---|---|
| `display` `title` `heading` | tiêu đề trang / mục |
| `section-label` | eyebrow phía trên tiêu đề trang (viết hoa nhỏ, muted) |
| `card-title` | tên của một card hoặc một hàng danh sách |
| `body` `body-strong` | nội dung, và nửa nhấn mạnh của cặp |
| `subtitle` `muted` | mô tả phụ |
| `faint` | metadata hạng ba (runtime, quantization) |
| `danger` `empty` | lỗi, và ô rỗng giữa trang |
| `code` | chuỗi kỹ thuật, font IBM Plex Mono |

Font UI: Manrope. Không thêm họ font mới.

## 4. Nhịp

`SPACE`: `3xs`2 `2xs`4 `xs`6 `sm`8 `md`12 `lg`16 `xl`20 `2xl`24 `3xl`32 `4xl`40.
Composite dùng sẵn: `PAGE_MARGINS` `PAGE_SPACING` `CARD_MARGINS` `CARD_SPACING`
`DIALOG_MARGINS` `DIALOG_SPACING` `TOOLBAR_SPACING`. Mọi control cao `CONTROL_HEIGHT`=32.

## 5. Bộ phận có sẵn — dùng lại, đừng dựng mới

- **Nút**: `primary` `cta` (hành động chính), mặc định (thứ cấp), `ghost`, `icon` (30×30,
  chỉ glyph), `chip`, `segment-item`, `menu-item` `nav-item` `rail-item`.
- **Khối**: `card` (bo 14, viền `line`), `panel`, `hline` `vline`.
- **Nhãn trạng thái**: `chip` `chip-active` `pill` `badge-success` `badge-warn`
  `badge-danger` — cao 26px cố định; lấy lớp qua `format.badge_class(state)`.
- **Widget**: `StatusPip`/`StatusPipLabel`, `ConfirmButton`/`ConfirmToolButton` (xóa hai
  bước — **không** dùng `QMessageBox`), `Toast` qua `ctx.toast(msg, "error")`,
  `progress_bar`, `model_picker`, `profile_switcher`, `sidebar`, `topbar`.
- **Icon**: lucide outline trong `ui/icons.py`; `icon(name, color=..., size=...)` cho nút,
  `pixmap(...)` cho `QLabel`. Thêm glyph = dán nguyên body 24×24 của lucide vào `PATHS`.
  Màu được nung vào pixmap ⇒ view có icon phải dựng lại hàng khi `ctx.themeChanged`.
- **Định dạng**: `format.py` — `format_bytes` `format_file_size` `format_relative_time`
  `format_count` (nghìn ngăn bằng dấu chấm, vi-VN).

Đổi `class` sau khi widget đã hiện → gọi `theme.restyle(widget)`.

## 6. Khuôn mẫu bố cục

**Trang** (`views/*.py`): eyebrow `section-label` → `title` → blurb `muted` một dòng, nút
chính canh phải trên cùng; rồi `QScrollArea` không viền, `contentsMargins(0,…)`, các hàng
cách nhau `SPACE["sm"]`. Ô rỗng dùng `class="empty"`: một câu chuyện gì, một câu làm gì.

**Hàng danh sách** = `QFrame class="card"`, `CARD_MARGINS`/`CARD_SPACING`, mọi cột canh
`AlignTop`:

```
[avatar 32] [tên + chip mặc định / runtime + icon năng lực] [số canh phải] [pip + trạng thái] [icon actions]
```

Xem [models_view.py](../../../src/private_ai/ui/views/models_view.py) làm bản mẫu.

**Card cài đặt** (`settings_view.py`): `_group(title, hint, control)` → `_row(caption,
control, hint)` → `_divider()`. Control rộng (`QComboBox`, `QLineEdit`) tự xuống dòng
riêng; checkbox và segmented ngồi cạnh nhãn.

**Dialog**: `dialogs/_shell.py` — `dialog_layout` → `title_block` → `field` →
`action_row` (đẩy stretch trước, nút chính đặt cuối).

## 7. Chữ tiếng Việt

Câu ngắn, chủ động, ≤ 8 từ cho hint. Nhãn ngắn không chấm câu; hint là câu thì có chấm.
Không "Vui lòng", không "Hệ thống đang…". Lỗi nói **chuyện gì + làm gì tiếp**:
"Không đọc được thư viện mô hình. Khởi động nhà cung cấp rồi thử lại."

## 8. Vòng lặp khi redesign

```bash
python tools/uishot.py views.models_view.ModelsView --out /tmp/shots   # chụp trước
# sửa
ruff format <file> && ruff check <file>
QT_QPA_PLATFORM=offscreen pytest tests/test_theme.py tests/test_alignment.py -q -p no:warnings
python tools/uishot.py views.models_view.ModelsView --out /tmp/shots   # chụp lại, xem cả 2 theme
```

**Luôn xem ảnh trước và sau** — cả `light` lẫn `dark`. Diff không cho biết một hàng có
chật hay không.

## 9. Ràng buộc tự động — đừng phá

- `tests/test_theme.py`: chữ đạt WCAG AA 4.5:1 trên mọi nền, viền control ≥ 2.8:1, hai
  theme cùng bộ token, thang chữ tăng dần và ≥ 11px, mọi control cùng một baseline, badge
  cùng chiều cao.
- `tests/test_alignment.py`: hai `QLabel` cạnh nhau phải **trùng mép trái hoặc cách ≥ 8px**
  — gần bằng nhau đọc như lỗi. Lớp có hình bao (`chip`, `avatar`, `code`…) được miễn. View
  mới nên thêm vào danh sách `VIEWS`.
