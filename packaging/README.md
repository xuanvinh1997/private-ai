# Đóng gói cho macOS

```bash
.venv/bin/python -m pip install "pyinstaller>=6.16"
./packaging/build.sh
open "dist/Private AI.app"
```

Kết quả: `dist/Private AI.app` — bundle id `com.vinhpx.private-ai`, **445 MB**, arm64,
macOS 26 trở lên. Bản `.dmg` nén còn 211 MB, bản `.tar.gz` còn 183 MB.

| Cờ | Tác dụng |
| --- | --- |
| *(không có)* | Ký ad-hoc. Vẫn chạy được trên máy khác — xem [Chạy trên máy khác](#chạy-trên-máy-khác). |
| `--sign "<identity>"` | Ký bằng chứng chỉ thật, kèm hardened runtime. |
| `--dmg` | Đóng thêm `dist/Private AI.dmg`. |

## Một app, ba vai trò

Bundle chỉ có một tệp thực thi, nhưng ứng dụng là ba chương trình. Trong bản cài từ mã
nguồn, `pyproject.toml` sinh ra `private-ai`, `private-ai-worker`, `private-ai-mcp…`;
trong `.app` không có `bin/` và không có PATH, nên vai trò được chọn từ `argv`
([entry.py](entry.py)):

```bash
"Private AI.app/Contents/MacOS/private-ai"                 # giao diện
"Private AI.app/Contents/MacOS/private-ai" --worker        # tiến trình nạp tài liệu
"Private AI.app/Contents/MacOS/private-ai" --mcp core      # MCP server qua stdio
"Private AI.app/Contents/MacOS/private-ai" --asr status    # chẩn đoán nhận dạng giọng nói
```

Giao diện **tự khởi động worker** rồi tắt nó khi thoát. Đọc một tài liệu là Python thuần
giữ GIL suốt thời gian xử lý — đó chính là lý do worker tồn tại như một tiến trình riêng —
nên một bản đóng gói bỏ qua nó sẽ tự đóng băng cửa sổ của mình mỗi lần tải tệp lên. Ngược
lại, worker chết không phải chuyện chí mạng: hộp thoại tải lên tự xử lý khi không ai giữ
claim, nên worker hỏng chỉ làm chậm, không làm mất chức năng. Đặt `PRIVATE_AI_NO_WORKER=1`
để tắt hẳn.

Dòng `--mcp` cho phép trỏ một MCP client bên ngoài thẳng vào bundle. Tên hợp lệ: `core`,
`artifacts`, `vector`, `keyword`, `hybrid`, `graph`, `summary`, `web`.

## Dữ liệu nằm ở đâu

```
~/.private-ai/
```

Một đường dẫn duy nhất trên macOS, Linux và Windows — không phải `Application Support`,
`XDG_DATA_HOME` hay `LOCALAPPDATA` như quy ước từng nền tảng. Đây là ứng dụng cục bộ một
người dùng, và dữ liệu của nó là thứ người ta thật sự mở terminal ra xem, backup bằng
script, hoặc xoá thẳng; một đường dẫn giống nhau ở mọi nơi và không phải escape khoảng
trắng đáng giá hơn việc chiều quy ước của từng hệ điều hành.

Bản chạy từ mã nguồn vẫn dùng `.local-data` cạnh mã nguồn như trước. Hai bên **không dùng
chung dữ liệu** — chuyển từ bản dev sang bản đóng gói là bắt đầu lại từ kho rỗng. Muốn
dùng chung thì đặt biến môi trường:

```bash
PRIVATE_AI_DATA_DIR="$HOME/Workspaces/private-ai/.local-data" open "dist/Private AI.app"
```

Quy tắc nằm ở `default_data_dir` trong [config.py](../src/private_ai/config.py): app đóng
gói được mở với thư mục làm việc là `/`, còn thư mục của chính nó thì chỉ đọc và đã ký, nên
đường dẫn tương đối `.local-data` không dùng được.

Ghi được hay không thì không phải vấn đề ở đây: app **không bật App Sandbox**, nên không có
container nào chuyển hướng đường ghi, và TCC chỉ chắn `~/Desktop`, `~/Documents`,
`~/Downloads` chứ không chắn thư mục gốc `~/`. Chọn `~/.private-ai` là chọn cho tiện, không
phải để né một hạn chế nào.
## Chạy trên máy khác

Được, nhưng máy nhận phải qua được ba cửa **độc lập** nhau. Trượt cửa nào cũng hỏng, và
mỗi cửa báo lỗi một kiểu khác nhau.

### Cửa 1 — phần cứng và phiên bản

Bản build hiện tại là **arm64, macOS 26 trở lên**. Không chạy trên Mac Intel, không chạy
trên macOS 15.

`LSMinimumSystemVersion` trong Info.plist do `build.sh` **đo** chứ không phải gõ tay: nó
lấy `minos` cao nhất trong toàn bộ 324 tệp Mach-O của bundle. Ghi một con số thấp hơn sự
thật không làm app chạy được trên máy cũ hơn — chỉ làm LaunchServices khởi động nó rồi để
dyld giết một giây sau bằng thông báo không ai xử lý được.

Sàn 26.0 đến từ **Python của Homebrew**: Homebrew chỉ build cho macOS đang chạy, nên 57
tệp trong `python3.14/` cùng `libssl`/`libcrypto`/`libsqlite3`/`libzstd` đều đòi 26.0. Muốn
hạ sàn thì đổi Python dùng để build:

```bash
# Python từ python.org nhắm deployment target thấp
/Library/Frameworks/Python.framework/Versions/3.14/bin/python3 -m venv .venv-build
.venv-build/bin/python -m pip install -e . "pyinstaller>=6.16"
PYTHON=.venv-build/bin/python ./packaging/build.sh
```

Sàn khi đó tụt xuống **15.0**, do PySide6 6.11 và shiboken6. Muốn thấp hơn nữa thì phải hạ
phiên bản PySide6.

### Cửa 2 — chữ ký

Chữ ký ad-hoc **hợp lệ trên mọi máy Mac**. Apple Silicon bắt buộc mọi mã thực thi phải có
*một* chữ ký, không bắt buộc phải là chữ ký *được tin cậy*, và bundle này đã có. Nên cửa
này bản hiện tại đã qua.

### Cửa 3 — Gatekeeper và cờ quarantine

Đây mới là cửa thật sự chặn. Gatekeeper chỉ chặn thứ mang thuộc tính mở rộng
`com.apple.quarantine` — do trình duyệt, AirDrop, Messages gắn vào khi tải về. Có hai cách
qua:

**Chuyển bằng đường không gắn quarantine.** `scp`, `rsync`, `tar` qua ssh, hoặc USB định
dạng exFAT/FAT (không lưu được xattr) đều không gắn cờ. App chạy thẳng, không hỏi gì.

```bash
tar -czf private-ai.tgz -C dist "Private AI.app"
scp private-ai.tgz may-kia:~/           # rồi giải nén vào /Applications trên máy kia
```

**Hoặc gỡ cờ trên máy nhận** — chỉ làm khi họ tin nguồn:

```bash
xattr -dr com.apple.quarantine "/Applications/Private AI.app"
```

Trên macOS 15 trở lên, mẹo Control-click → Open đã bị Apple bỏ. Đường trong giao diện bây
giờ là **System Settings → Privacy & Security → "Open Anyway"**, và phải bấm sau khi đã thử
mở một lần.

Nên chép app vào `/Applications` trước khi mở. Chạy thẳng từ DMG hoặc từ `~/Downloads` khi
còn cờ quarantine sẽ kích hoạt App Translocation: macOS chạy bản sao ở một đường dẫn ngẫu
nhiên chỉ đọc.

> Không kiểm chứng được phán quyết Gatekeeper từ máy build này: `spctl --status` cho
> "assessments disabled", nên `spctl -a` ở đây luôn trả về "accepted" bất kể app thế nào.
> Phần trên là cơ chế của macOS, không phải kết quả đo.

### Cách đàng hoàng: Developer ID + notarize

Đây là mức duy nhất người nhận **không phải làm gì cả**. Cần tài khoản Apple Developer
Program có trả phí — chứng chỉ **Apple Development** trên máy này *không* đủ, nó chỉ dùng
được cho chính máy đã ký.

```bash
./packaging/build.sh --sign "Developer ID Application: … (TEAMID)" --dmg
xcrun notarytool submit "dist/Private AI.dmg" \
    --apple-id you@example.com --team-id TEAMID --password <app-specific-password> --wait
xcrun stapler staple "dist/Private AI.dmg"
```

`stapler` đính kết quả notarize vào chính tệp DMG, nên máy nhận không cần mạng để kiểm tra.

### Còn thiếu gì để app thật sự dùng được

Qua đủ ba cửa mới chỉ là **mở được cửa sổ**. Máy nhận vẫn cần:

- **Ollama** đang chạy, hoặc một provider đã cấu hình trong Cài đặt. Không có thì app mở
  lên rồi báo chưa cấu hình nhà cung cấp AI.
- **transcribe.cpp** biên dịch tại chỗ nếu muốn đọc chính tả. Việc này cần toolchain nên
  không nằm trong bundle; thiếu nó thì mọi thứ khác vẫn chạy.
- Khoảng **450 MB** trống, cộng với chỗ cho `~/.private-ai`.

## Micro

`NSMicrophoneUsageDescription` nằm trong Info.plist và `com.apple.security.device.audio-input`
nằm trong [entitlements.plist](entitlements.plist). Thiếu khoá đầu, macOS từ chối micro mà
**không hiện hộp thoại nào** — tính năng đọc chính tả sẽ im lặng không hoạt động.

### Nhận dạng giọng nói trong bundle

Bundle **có sẵn** runtime transcribe.cpp đã biên dịch — 4,3 MB gồm `libtranscribe` và bốn
thư viện ggml — nên máy đích không cần git, cmake hay compiler. Phần duy nhất còn thiếu là
473 MB trọng số, và người dùng tải nó ngay trong **Cài đặt → Mô hình**, hàng "Nhận dạng
giọng nói". Tải xong là nút micro trong ô soạn sáng lên.

Máy **build** thì vẫn phải chạy `private-ai-asr setup` một lần, vì `build.sh` chỉ đóng gói
thứ đã được dựng sẵn. Chưa dựng thì bước nhúng in cảnh báo và bỏ qua — app vẫn build được,
chỉ là không có giọng nói.

Chẩn đoán khi ai đó báo nút micro tối:

```bash
"/Applications/Private AI.app/Contents/MacOS/private-ai" --asr status
```

Cái bẫy ở [bundle_asr.py](bundle_asr.py): mỗi `.dylib` được link với `LC_RPATH` là đường
dẫn **tuyệt đối** trỏ vào `.local-data` của máy build. Chép nguyên vào bundle thì chạy được
trên đúng máy đã build và không đâu khác — `@rpath/libggml.0.dylib` phân giải qua một thư
mục người nhận không có. Nên sau khi chép, mỗi rpath được viết lại thành `@loader_path`
tương ứng. Việc này phải làm **trước** bước ký, vì `install_name_tool` sửa header Mach-O và
làm hỏng mọi chữ ký đã có.

## Hai cái bẫy trong [private_ai.spec](private_ai.spec)

**Import động.** `VIEW_SPECS` trong `ui/main_window.py` và `BUILTIN_SERVERS` trong
`mcp/client.py` đều đặt tên module bằng chuỗi, và cả hai đều nuốt `ImportError` để app nửa
vời vẫn chạy được. Bản build đầu tiên vì thế *khởi động thành công* với năm màn hình
placeholder và không một tool nào — không có dòng log nào trông giống lỗi chí mạng. Nên
spec dùng `collect_submodules("private_ai")` để gom toàn bộ gói, thay vì liệt kê tay.

**Kích thước.** PySide6 mang theo mọi module Qt: 1,2 GB, riêng QtWebEngine chiếm khoảng một
nửa. Ứng dụng chỉ dùng năm module (`QtCore`, `QtGui`, `QtWidgets`, `QtSvg`, `QtMultimedia`),
nên phần còn lại được liệt kê thẳng trong `excludes`. Đó là lý do bundle 440 MB thay vì hơn
1 GB. Nếu sau này có ai import nhầm `QtQml`, build sẽ hỏng ngay thay vì lặng lẽ phình thêm
vài trăm megabyte.

## Icon

[make_icon.py](make_icon.py) vẽ lại đúng ba thanh của `_BrandMark` trong sidebar rồi gọi
`iconutil`. Icon trong Dock và mark trên đầu thanh điều hướng là cùng một hình, và không cái
nào lệch khỏi cái kia được. `build.sh` tự chạy bước này.
