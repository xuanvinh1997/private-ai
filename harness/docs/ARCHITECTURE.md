# Harness — kiến trúc

Bản viết lại của Private AI thành một **coding & working agent** trên Rust + Tauri, theo
triết lý *everything is a plugin* của [deepseek-harness](https://github.com/deepseek-ai/deepseek-harness).

Không có lõi đặc quyền. Vòng lặp agent, bộ chuyển đổi mô hình, sổ tay phiên, sổ đăng ký
tool — tất cả đều là plugin cắm vào cùng một cây, và đều thay được từ cấu hình.

## Bốn ý nền

| Ý | Ở đâu | Khác Cordis chỗ nào |
|---|---|---|
| **Seam** — khả năng được đánh địa chỉ bằng marker type, không phải bằng bản cài đặt | `pai-core::service::ServiceKey` | Cordis dùng chuỗi. Ở Rust, chuỗi chỉ đổi lỗi biên dịch lấy lỗi lúc chạy. Chuỗi `NAME` vẫn còn, nhưng chỉ để nói với con người và tệp cấu hình |
| **Phụ thuộc là nhu cầu, không phải trình tự** | `Context::wait_for::<K>()` | Giống `inject`, nhưng không unload/reload consumer khi provider bị thay |
| **Sự kiện có kiểu** | `pai-core::event` | Rút 5 chế độ của Cordis xuống 3: `notify`, `first`, `waterfall`. `serial` và `bail` chỉ tách nhau vì JS phân biệt `T` với `Promise<T>` tại chỗ gọi |
| **Đăng ký là hiệu ứng gỡ lại được** | `Guard` (RAII, `#[must_use]`) + `EffectScope` | Quên disposer là **cảnh báo biên dịch**, không phải rò rỉ lúc chạy. `EffectScope` chỉ dành cho việc dọn cần `await` và cho thứ tự LIFO |

Ba chỗ bản Rust an toàn hơn bản gốc, và đều là cố ý:

1. `Next` **tiêu thụ chính nó** — không thể uỷ quyền hai lần. Cordis cho phép, và đó là nguồn lỗi.
2. Chuỗi listener được **chụp lại** trước khi chạy. Plugin gỡ tải giữa chừng không làm hỏng lượt đang chạy; Cordis cắt thẳng vào mảng dùng chung.
3. `Guard` bắt buộc phải được dùng. Muốn đăng ký sống bằng plugin thì `ctx.keep(guard)`.

## Cấu hình theo lớp

Danh sách plugin **là cấu hình**, không phải mã. Lớp nền dựng sẵn trong ứng dụng; lớp của
người dùng nằm ở `data_dir/patch.yaml` và áp lên trên. Một lớp trên có thể chèn hàng mới,
thay toàn bộ cấu hình của một hàng, hoặc tắt một hàng.

```yaml
# ~/.private-ai/patch.yaml — tắt tool chạy lệnh, không sửa một dòng mã nào
patches:
  - op: disable
    id: shell
```

Ba luật:

- **Thay cả khối, không trộn.** Trộn thì không có cách nào xoá một trường, và người viết
  bản vá phải đoán xem trường nào của lớp dưới còn sót lại.
- **Tắt chứ không xoá.** Một hàng vắng mặt sẽ lặng lẽ sống lại vào ngày ai đó đổi thứ tự
  lớp; một hàng `disabled` thì luôn nhìn thấy trong bản in cấu hình.
- **Nhắm nhầm là lỗi có tên.** Vá vào một `id` không tồn tại, hay chèn trùng `id`, đều
  dừng khởi động. "Không có gì xảy ra" là câu trả lời tệ nhất cho một lỗi gõ nhầm.

Tên plugin trong cấu hình được tra trong một danh sách đóng khai báo lúc khởi động, nên
một tệp vá không gọi được thứ chưa ai đăng ký — tệ nhất nó làm được là dựng sai cây, chứ
không phải chạy mã lạ. Đây cũng là lý do không có biểu thức trong cấu hình: dsh cho phép
`!!js`, chấp nhận được cho một CLI của lập trình viên, không chấp nhận được cho một ứng
dụng cài từ tệp `.dmg`.

## Dự án, và hai tầng plugin

Một dự án là một thư mục, và **danh tính của nó là đường dẫn đã chuẩn hoá** — không phải
cái tên. Hai lối vào cùng một thư mục, qua symlink hay qua `..`, phải là một dự án; nếu
không người dùng có hai hàng trỏ cùng một chỗ, mỗi hàng nhớ một nửa lịch sử.

Cây plugin chia làm hai tầng, và tiêu chí chia là một câu hỏi duy nhất: **plugin này có
cần một đường dẫn không?**

| Tầng | Plugin | Vòng đời |
|---|---|---|
| Ứng dụng | `tools` `agent` `compaction` `hooks` `sandbox` `mcp` `providers` | dựng một lần, sống bằng tiến trình |
| Dự án | `fs` `shell` `terminal` `index` `lsp` `rag` `skills` `subagent` | tháo và cắm lại mỗi lần đổi dự án |

Đổi dự án **là** tháo tầng dưới rồi cắm lại với đường dẫn mới. Không có bước "cập nhật gốc
của fs", "đổi cwd của shell", "trỏ chỉ mục sang chỗ khác". Mỗi bước như thế là một chỗ để
quên, và cái quên đó chỉ lộ ra khi một tool đọc nhầm repo — muộn, và không giải thích được
cho ai. Cắm lại thì không quên được: plugin nào cũng đi qua đúng một đường khởi tạo, đường
mà nó đã đi lúc khởi động.

Đây là chỗ kiến trúc plugin trả nợ. Nếu đổi dự án cần một đường "cấu hình lại mọi thứ"
chạy song song với đường cắm plugin, hai đường đó sẽ trôi ra khỏi nhau, và đường thứ hai
sẽ luôn thiếu một thứ.

Việc phân tầng làm theo **tên plugin**, không theo tệp cấu hình và không theo `id`. Theo
tệp thì bản vá của người dùng phải biết chuyện chia tầng; theo `id` thì đổi tên một hàng
là lặng lẽ đổi tầng của nó.

### Hai loại dự án

Tầng dự án còn chia tiếp một lần nữa, theo **loại** của dự án đang mở:

| Loại | Plugin được cắm | Không có |
|---|---|---|
| Mã nguồn | `skills` `fs` `subagent` `index` `lsp` `shell` `terminal` | — |
| Tài liệu | `skills` `rag` `subagent` | `fs` `shell` `terminal` `index` `lsp` |

Danh sách của dự án tài liệu ngắn hơn hẳn, và mỗi cái vắng mặt là một quyết định chứ không
phải một chỗ chưa làm. Một thư viện tài liệu là một chồng tệp **do người khác gửi tới**.
Cấp cho nó `shell` và `edit` nghĩa là thứ duy nhất đứng giữa một câu trong một tệp PDF và
một lệnh chạy trên máy người dùng là việc mô hình có nghe theo câu đó hay không — và điều
đó không phải một ranh giới, nó là một hy vọng. `index` và `lsp` cũng vắng mặt vì lý do
đơn giản hơn: chúng phân tích mã nguồn, và ở đây không có mã nguồn.

Điều quan trọng về mặt thi hành: **không có bước "tắt tool cho dự án tài liệu"**. Tool
không hợp thì không được cắm ngay từ đầu, nên không có gì để tắt và không có gì để quên
tắt. Một bước lọc chạy song song với đường cắm plugin sẽ trôi ra khỏi nó — đó chính là
cái bẫy mà việc chia hai tầng đã tránh được một lần rồi, và chia theo loại tránh lại lần
nữa bằng đúng cách ấy.

Loại là thuộc tính của **hàng trong kho**, không phải của thư mục. Mở lại một dự án bằng
`touch` giữ nguyên loại; chỉ `create` mới đặt nó. Nếu mở lại mà loại đổi được thì tập tool
sẽ đổi dưới chân người dùng vì một lý do họ không nhìn thấy.

## Cây crate

```
pai-core       Context, seam, event bus, effect scope. Không phụ thuộc crate nào khác của dự án.
pai-session    Sổ tay phiên chỉ-ghi-thêm. Nguồn duy nhất của ngữ cảnh mà mô hình thấy.
pai-llm        Từ vựng message/stream + seam adapter. Ollama và OpenAI-compatible.
pai-tools      Sổ đăng ký tool có phạm vi + đường ống thi hành có canh gác.
pai-fs         Seam hệ tệp + tool read/write/edit/glob/grep.
pai-shell      Seam thi hành lệnh + tool bash.
pai-sandbox    Seam giam tiến trình. Seatbelt (macOS), Landlock (Linux), restricted token (Windows).
pai-mcp        Client cho server bên thứ ba + một server duy nhất phơi cả sổ đăng ký ra ngoài.
pai-agent      Vòng lặp turn/step. Là plugin, thay được.
pai-index      Chỉ mục ký hiệu + đồ thị bộ nhớ mã nguồn (tree-sitter, SQLite FTS5).
pai-rag        Thư viện tài liệu: rút chữ, cắt đoạn, nhúng vector, tìm lai ghép.
pai-providers  Kho provider mô hình, danh mục dựng sẵn, đổi provider lúc đang chạy.
pai-project    Danh sách dự án, clone từ Git, cây tệp và nội dung tệp cho *người đọc*.
pai-app        Vỏ Tauri: lệnh invoke và kênh sự kiện. Cố tình mỏng.
```

Quy tắc phụ thuộc, chép từ dsh: **plugin mở rộng phụ thuộc vào Service Definition, không
bao giờ vào provider cụ thể.** `pai-agent` thay được mà không ai phải sửa import.

## Sổ tay phiên

Nguồn duy nhất của ngữ cảnh. Bất biến trung tâm:

> **Cái gì mô hình thấy được thì phải nằm trong sổ.** Mọi thứ đi vào một request đều phải
> dựng lại được từ sổ. Vì thế thêm một loại đầu vào mới là thêm một loại sự kiện mới.

Trong 53 loại sự kiện của dsh chỉ có **ba** loại sinh ra message cho mô hình —
`user/message`, `assistant/message`, `tool/result`. `derive_messages()` chỉ gấp ba loại
đó. Nén ngữ cảnh không xoá gì cả: nó ghi một thao tác `replace(start, end)` che dải cũ,
nên bản ghi vẫn đầy đủ để phát lại.

## Vòng đời một lượt

```
turn/start
  nhận đầu vào cho bước kế + một message trong hàng đợi
  ráp prompt + schema tool
  -> agent/pre-step        waterfall: từ chối, hoặc nhận (và được sửa message)
     step/start
     dựng lịch sử mô hình từ sổ
     agent/request -> llm/stream -> assistant/chunk* -> assistant/message
     tool/call* -> tools/pre-execute -> guards -> tools/execute -> tools/post-execute -> tool/result*
     step/end
  -> agent/turn-stopping   serial, không uỷ quyền được
turn/end
```

Chính sách cắm ở các waterfall, không ở trong vòng lặp. Vòng lặp không biết gì về
approval, sandbox, hook hay nén ngữ cảnh.

## Đường ống thi hành tool

```
tool/call (ghi sổ TRƯỚC khi chạy)
  → tools/pre-execute   waterfall: hook, quyền, sandbox → allow | deny | ask
      ask → ctx.approval, một lần duy nhất; không trả lời được → deny
  → guards              đơn điệu: chỉ deny hoặc bỏ qua
  → tools/execute       waterfall bao quanh: timeout, retry, đo đạc
      → thân tool
  → tools/post-execute  waterfall: nhận | chặn | thay | thêm ngữ cảnh
  → finalize            đồng bộ, chỉ đụng content
  → tool/result         đóng băng, ghi sổ
```

**Guard đơn điệu là có chủ ý**: chúng không có nhánh "allow", nên thứ tự đăng ký không
thể biến một lệnh từ chối thành cho phép.

Hai luật giữ nguyên từ bản Python vì bản gốc làm đúng hơn dsh:

- **Lọc hai tầng.** Kiểm tra quyền lúc liệt kê tool *và* một lần nữa lúc gọi, sau khi đã
  giải mã tên. Danh sách quảng cáo chỉ là gợi ý: một mô hình đoán ra `documents__delete`
  sẽ đi thẳng vào hàm gọi.
- **Tham số mô hình không thấy là tham số nó không thể làm sai.** Khi lượt đã ghim vào
  một workspace thì `workspace_id` bị **xoá khỏi schema** và bị **ghi đè** lúc gọi, chứ
  không phải điền giá trị mặc định.

Và một luật nữa: **từ chối trả về dưới dạng văn bản, không phải lỗi.** Một exception chỉ
kết thúc lượt trong im lặng; mô hình phải đọc được vì sao nó không được chạy.

## Ranh giới tin cậy

Trích đoạn tài liệu, kết quả web và dữ liệu đồ thị là **dữ liệu không đáng tin cậy**. Lời
cảnh báo được lặp lại trong **mô tả của từng tool truy hồi**, vì mô tả tool là thứ duy
nhất mô hình đọc đúng vào lúc nó quyết định làm gì với đoạn văn bản trả về.

Nội dung skill thì ngược lại: do người vận hành viết, nên được chèn vào như **chỉ dẫn
đáng tin cậy**. Vì thế không có đường nào từ ingestion hay retrieval được phép tạo, đặt
tên hay sửa một skill.

## Những chỗ cố ý không sao chép

| Của Cordis / dsh | Ở đây | Vì sao |
|---|---|---|
| `!!js` trong tệp cấu hình | Không có; tên plugin tra trong một danh sách đóng | Đó là thực thi mã tuỳ ý từ tệp cấu hình. Chấp nhận được cho một CLI cho lập trình viên; không chấp nhận được cho một ứng dụng desktop |
| Plugin nạp bằng dylib | Không có | Rust không có ABI ổn định; `TypeId` không giữ nguyên qua ranh giới dylib, nên downcast có thể *thành công sai*. Bên thứ ba đi qua MCP |
| `serde_yaml` | `serde_norway` | `serde_yaml` đã ngừng bảo trì |
| `presentCall` / `presentResult` thuộc tool | Giao diện tự render từ sự kiện thô | dsh khai báo chúng nhưng bản web không dùng; diff đến từ `tool/result.meta.diffs` |
