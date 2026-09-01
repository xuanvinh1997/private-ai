# Đóng gói và ký

## Chạy

```sh
cd harness
./ui/node_modules/.bin/tauri build
```

Lệnh này build giao diện trước (`beforeBuildCommand`), rồi biên dịch bản release, rồi
đóng gói theo nền tảng đang chạy. Thêm `--no-bundle` để chỉ lấy tệp thực thi.

Sản phẩm nằm ở `harness/target/release/bundle/`.

## Quyết định trung tâm: App Sandbox tắt trên macOS

`app/entitlements.plist` **không** bật App Sandbox. Một coding agent phải đọc và sửa được
thư mục làm việc mà người dùng chỉ định, và phải chạy được toolchain của họ; App Sandbox
không cho làm cả hai, và một danh sách thư mục khai sẵn thì không biết trước người dùng
sẽ mở dự án nào.

Vòng giam thật nằm ở [`pai-sandbox`](../crates/pai-sandbox): mỗi lệnh con chạy qua
`sandbox-exec` với hồ sơ sinh theo workspace. Đó là ranh giới đúng chỗ — quanh **thứ mô
hình chạy**, không phải quanh chính ứng dụng.

Đánh đổi: bản này **không lên được Mac App Store**. Đó là chủ ý, không phải thiếu sót.

Hai quyền còn lại đều bắt buộc: `allow-jit` và `allow-unsigned-executable-memory` cho
JavaScriptCore của WebView. Thiếu chúng thì cửa sổ trắng trơn **sau khi ký**, và triệu
chứng không hề chỉ về đây.

## Ký

Không có dấu vân tay chứng chỉ nào nằm trong repo. Cả ba nền tảng nhận thông tin ký qua
biến môi trường lúc phát hành.

### macOS

```sh
export APPLE_CERTIFICATE="$(base64 -i chung-chi.p12)"
export APPLE_CERTIFICATE_PASSWORD='…'
export APPLE_SIGNING_IDENTITY='Developer ID Application: Tên (TEAMID)'
export APPLE_ID='…' APPLE_PASSWORD='…' APPLE_TEAM_ID='…'   # để công chứng
./ui/node_modules/.bin/tauri build
```

`APPLE_ID` + `APPLE_PASSWORD` (mật khẩu riêng cho ứng dụng) + `APPLE_TEAM_ID` bật công
chứng. **Bỏ qua bước công chứng thì Gatekeeper chặn bản tải về**, kể cả khi đã ký — chữ
ký nói "ai làm ra nó", công chứng nói "Apple đã quét nó".

### Windows

```powershell
$env:TAURI_WINDOWS_CERTIFICATE_THUMBPRINT = '…'
```

`timestampUrl` đã đặt sẵn trong cấu hình: không đóng dấu thời gian thì chữ ký hết hiệu
lực đúng ngày chứng chỉ hết hạn, và mọi bản đã phát hành hỏng cùng lúc.

### Linux

`deb` và `AppImage` không có cơ chế ký nội tại. Ký ở tầng kho (`dpkg-sig`, hoặc chữ ký
tách rời cho AppImage) là việc của quy trình phát hành, không phải của bundler.

## Đã kiểm chứng tới đâu

| | Biên dịch | Đóng gói | Ký | Chạy thử |
|---|---|---|---|---|
| macOS (arm64) | ✅ | ✅ | ✗ chưa có chứng chỉ | ✅ |
| Linux (arm64) | ✅ trong Docker | ✗ | ✗ | ✅ vòng giam đã đo |
| Windows | ✗ | ✗ | ✗ | ✗ |

Hàng Linux: `pai-sandbox` biên dịch và **5/5 bài kiểm chứng vòng giam chạy thật** trong
`rust:1-slim` trên kernel 6.x/7.x (cần `--security-opt seccomp=unconfined`, vì hồ sơ
seccomp mặc định của Docker chặn syscall của Landlock). Phần đóng gói `deb`/AppImage thì
chưa chạy. Hàng Windows là **cấu hình chưa từng chạy**. Chúng theo tài liệu Tauri và sẽ cần sửa khi
lần đầu build thật — viết ra đây để không ai đọc bảng này rồi tưởng chúng đã xong.

`pai-sandbox` trên Windows báo `Enforcement::None` kèm lý do chứ không giả vờ đang giam:
xem [`crates/pai-sandbox/src/lib.rs`](../crates/pai-sandbox/src/lib.rs).

## Cross-build

Máy phát triển hiện tại cài Rust qua Homebrew, **không có `rustup`**, nên chỉ build được
cho chính nó. Cần `rustup` trước khi thêm target nào khác.
