# Đóng gói và ký

## Chạy

```sh
cd harness
node scripts/package.mjs
```

Đây là lối vào production bắt buộc. Nó chạy Tauri với `app/tauri.production.conf.json`;
hook build tải đúng Qdrant **v1.19.0**, SurrealDB **v3.2.4** và bản INT8 ONNX của
`BAAI/bge-reranker-v2-m3`, kiểm kích thước cùng SHA-256, rồi đưa chúng vào
`.app`/installer. Máy người dùng không cần Docker, inference API hay mạng khi chạy.
Binary/model sinh ra trong `app/binaries/` và `app/models/` bị gitignore vì có thể tái
tạo từ revision + digest đã ghim.

Thêm tham số Tauri sau lệnh, chẳng hạn `node scripts/package.mjs --no-bundle` hoặc:

```sh
# Apple Silicon
node scripts/package.mjs --target aarch64-apple-darwin

# Intel Mac
node scripts/package.mjs --target x86_64-apple-darwin

# Universal Mac (cần cài cả hai Rust target)
node scripts/package.mjs --target universal-apple-darwin

# Chạy lệnh này trên máy/runner Windows x64
node scripts/package.mjs --target x86_64-pc-windows-msvc
```

Không phát hành bằng `tauri build` trần: script production mới ghép cấu hình sidecar và
tài nguyên model vào đúng lúc đóng gói. Model lượng tử chiếm khoảng 571 MB, tokenizer
khoảng 17 MB; lần build đầu cần mạng, các lần sau dùng bản đã kiểm digest trong cache
cục bộ.

Reranker được ghim ở revision
`a3046abee880d6e78833e4e885939754355156bd` của
`onnx-community/bge-reranker-v2-m3-ONNX`; graph `model_quantized.onnx` phải có SHA-256
`912fc1215c2dbff6499700534bd8d31253af01573861abbfc43afd1fab6cce5d`. Có thể chạy riêng
`node scripts/prepare-reranker.mjs` để tải và kiểm toàn bộ năm tệp trước khi đóng gói.

Sản phẩm host mặc định nằm ở `harness/target/release/bundle/`; khi truyền `--target`,
nó nằm ở `harness/target/<target>/release/bundle/`.

Workflow thủ công `.github/workflows/package-smoke.yml` dựng **macOS universal** và
**Windows x64** trên đúng hệ điều hành, rồi lưu toàn bộ bundle làm artifact. Đây là bài
smoke unsigned; bản phát hành thật vẫn phải truyền chứng chỉ như phần dưới.

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
node scripts/package.mjs
```

`APPLE_ID` + `APPLE_PASSWORD` (mật khẩu riêng cho ứng dụng) + `APPLE_TEAM_ID` bật công
chứng. **Bỏ qua bước công chứng thì Gatekeeper chặn bản tải về**, kể cả khi đã ký — chữ
ký nói "ai làm ra nó", công chứng nói "Apple đã quét nó".

### Windows

```powershell
$env:TAURI_WINDOWS_CERTIFICATE_THUMBPRINT = '…'
node scripts/package.mjs --target x86_64-pc-windows-msvc
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
| Windows x64 | ✅ sidecar PE x64 | ⚙️ workflow đúng OS | ✗ chưa có chứng chỉ | ✗ chưa chạy app trên Windows |

Hàng Linux: `pai-sandbox` biên dịch và **7/7 bài kiểm chứng vòng giam chạy thật** trong
`rust:1-slim` trên kernel 6.x/7.x (cần `--security-opt seccomp=unconfined`, vì hồ sơ
seccomp mặc định của Docker chặn syscall của Landlock). Phần đóng gói `deb`/AppImage thì
chưa chạy. Hai sidecar Windows đã được tải, kiểm SHA-256 và giải nén thử thành PE32+
x86-64 trên máy phát triển. Installer phải được dựng trên Windows; workflow smoke là
cổng kiểm tra đó, nhưng bảng không đánh dấu chạy thử cho tới khi artifact được mở trên
máy Windows.

Hai bài mới trong số đó là phần **giam mạng**: một bài nối TCP tới cổng do chính nó mở rồi
kiểm rằng `deny_network` chặn được, một bài kiểm rằng bật giam mạng không nới lỏng phần giam
tệp. Cả hai chạy trên kernel 7.x, tức Landlock ABI ≥ 4. Giam mạng ở Linux là **TCP thôi** —
Landlock không có động từ cho UDP — và `network_confinable()` trả `false` dưới ABI 4.

`pai-sandbox` trên Windows báo `Enforcement::None` kèm lý do chứ không giả vờ đang giam:
xem [`crates/pai-sandbox/src/lib.rs`](../crates/pai-sandbox/src/lib.rs).

## Cross-build

Máy phát triển hiện tại cài Rust qua Homebrew, **không có `rustup`**, nên chỉ build được
cho chính nó. Cần `rustup` trước khi thêm target nào khác.
