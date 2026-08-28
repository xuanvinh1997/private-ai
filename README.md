# Private AI

Private AI là desktop control plane cho mô hình, tài liệu và memory chạy cục bộ. UI native và AI runtime cùng chạy trên máy người dùng.

## Phần đã chạy được

- FastAPI gateway bind mặc định vào `127.0.0.1`.
- Ollama health, model inventory, pull stream, unload, delete có xác nhận và chat không stream.
- SQLite lưu workspace, hội thoại, message, memory, document, chunk và background job.
- Workspace và lịch sử chat có CRUD thật; chat stream token từ Ollama qua SSE, cho phép dừng
  generation và lưu lại câu hỏi cùng câu trả lời đã hoàn thành.
- Tài liệu thuộc về đúng một workspace: thư viện, tìm kiếm, ngữ cảnh chat và các tool MCP
  đều bị giới hạn trong workspace đang mở, và xóa workspace sẽ xóa luôn tài liệu, file trên
  đĩa và node graph của nó.
- Upload theo luồng, giới hạn kích thước, SHA-256 và deduplicate trong phạm vi workspace; PDF, DOCX, PPTX,
  XLSX, JPG/JPEG, PNG, WebP, TIFF, BMP, GIF, Markdown và text được trích xuất bởi worker cục bộ.
- Tài liệu được chia đoạn, tìm kiếm cục bộ và tự đưa các đoạn liên quan vào chat kèm yêu cầu
  dẫn nguồn. Retrieval kết hợp keyword và embedding bằng reciprocal-rank fusion; vector được
  lưu trong SQLite nên không cần extension native riêng trên macOS hoặc Windows.
- LightRAG lo chunking, embedding, trích xuất entity/quan hệ và truy xuất; mỗi không gian làm
  việc là một namespace riêng nên không gian này không bao giờ trả lời bằng tài liệu của không
  gian kia. Citation trong chat dẫn theo tên tệp nguồn.
- `embeddinggemma` là embedding model mặc định. Có thể đổi bằng
  `PRIVATE_AI_EMBEDDING_MODEL`; khi model không sẵn sàng, hệ thống tự fallback về keyword.
- MarkItDown và plugin `markitdown-ocr` được bật trong worker. OCR **chỉ chạy bằng LLM
  vision**, không dùng engine OCR cổ điển: plugin gọi model vision qua OpenAI-compatible API
  của nhà cung cấp đang bật, giữ heading/list/table, đọc ảnh nhúng trong PDF/Office và OCR cả
  trang cho bản scan; JPG/PNG cũng đi qua đường này. Tick OCR là đủ: nếu chưa đặt mặc định cho
  tác vụ `vision`, worker tự lấy model có khả năng đọc ảnh trong kho của nhà cung cấp đang
  bật. Không có model nào đọc được ảnh thì tài liệu vào `needs_ocr` kèm lý do, chứ không đoán
  bừa.
- Memory có tạo, sửa, bật/tắt, xóa và semantic search. SQLite giữ bản chuẩn cùng embedding
  cache và tự xếp hạng bằng vector đã lưu. Chat chỉ lấy top-k memory liên quan và fallback về
  keyword/recent khi nhà cung cấp ngoại tuyến; UI cho phép xuất toàn bộ memory thành JSON.
- `GpuLeaseManager` đồng bộ model Ollama đang chạy từ `/api/ps`, reserve trước chat/embedding.
  ASR batch giữ 2 GiB trong lúc transcription; ASR streaming giữ lease trong lúc native model
  được cache và giải phóng khi API shutdown. Request mới bị từ chối trước khi load nếu vượt
  capacity cấu hình.
- SolidJS dashboard responsive, hiển thị health/model/VRAM; các màn hình Chat Workspaces,
  Library, Memory, Models và Settings đều nối với API.
- Màn hình chính Chat Workspaces, light mode mặc định, dark mode tùy chọn và chế độ chữ lớn; lựa chọn được lưu cục bộ trên thiết bị.
- `pywebview` desktop launcher với API local chạy trong cùng Python/Conda environment.
- MCP Python SDK v2 server tại `http://127.0.0.1:8010/mcp`, có bearer token cục bộ,
  Origin/Host validation và 25 tool cho document, GraphRAG, memory, model inventory/default,
  tìm kiếm web, thông số máy và đọc file cục bộ.
- `system.info` trả OS, CPU, RAM, ngân sách GPU và dung lượng đĩa còn trống; `system.time` trả
  ngày giờ local lẫn UTC kèm múi giờ, để mô hình không phải đoán hôm nay là ngày mấy. Cả hai
  luôn bật, không cần cấu hình và không gửi gì ra khỏi máy.
- Chat trong ứng dụng gọi được chính các tool đó. API dựng MCP server ngay trong tiến trình của
  mình trên đúng bộ service đang chạy, nên không cần tiến trình thứ hai và không đi qua mạng —
  bản desktop đóng gói vẫn dùng được dù không chạy cổng 8010. Model nhận tool spec kèm mỗi lượt,
  máy chủ thực thi `tool_calls` rồi hỏi lại, tối đa 4 vòng; vòng cuối không đưa tool nữa để buộc
  ra câu trả lời. Câu hỏi không cần tool thì không phát sinh vòng nào. Tên tool có dấu chấm được
  đổi thành `__` vì tên function trên wire format không cho phép dấu chấm.
- Chat chỉ được đưa **19 tool chỉ-đọc**. Ingest, xóa tài liệu, ghi/xóa memory và đổi model mặc
  định không nằm trong danh sách: chúng chỉ chạy khi người dùng tự bấm trong UI, nên một tài
  liệu độc hại không thể dụ mô hình xóa dữ liệu.
- `files.list` và `files.read` đọc file cục bộ nhưng chỉ trong phạm vi người dùng cho phép.
  Thư mục duyệt sẵn khai báo qua `PRIVATE_AI_FILE_ROOTS`; đường dẫn nằm ngoài thì tool hỏi
  người dùng ngay lúc đó bằng MCP elicitation, và người dùng có thể chọn nhớ thư mục để lần
  sau khỏi hỏi lại (quyền đã nhớ lưu trong SQLite, xem bằng `files.allowed`). Đường dẫn được
  resolve trước mọi lần kiểm tra nên `..` và symlink không thoát ra khỏi thư mục được phép;
  file nhị phân bị từ chối thay vì trả về ký tự rác; file token MCP không bao giờ đọc được.
- Tìm kiếm web là tính năng duy nhất gửi nội dung ra khỏi máy, nên mặc định tắt và chỉ chạy khi
  người dùng tự bật nút **Tìm web** ở khung soạn tin. Ba nguồn được hỗ trợ, xếp theo mức riêng
  tư giảm dần: SearXNG tự dựng (không API key, phải bật `json` trong `search.formats` của
  `settings.yml`), DuckDuckGo không cần cấu hình (không có API chính thức nên có thể bị chặn
  tạm thời khi hỏi dày) và OpenAI web search có trả phí (~10 USD cho mỗi 1.000 lượt, chưa kể
  token). Kết quả được đưa vào prompt như dữ liệu không đáng tin cậy kèm yêu cầu dẫn nguồn theo
  URL; link quảng cáo của DuckDuckGo bị loại trước khi mô hình nhìn thấy. Nguồn tìm kiếm hỏng
  chỉ tạo một thông báo trên luồng SSE chứ không làm hỏng câu trả lời. Cùng khả năng này được
  mở cho MCP client qua tool `web.search`, và API key lưu trong SQLite không bao giờ được API
  trả ngược ra ngoài.
- Knowledge graph chạy bằng LightRAG nhúng thẳng trong tiến trình API, lưu graph/vector/KV
  bằng file dưới `.local-data/lightrag`. Không có database server nào phải chạy kèm. LLM và
  embedding của LightRAG đi qua đúng nhà cung cấp AI đang bật. RAG-Anything điều phối content
  block và insertion; callback của cả hai tầng được đưa vào tiến độ xử lý tài liệu.
- Màn hình Tri thức vẽ đồ thị của không gian đang mở: `GET /api/v1/graph` trả node/edge từ
  LightRAG (`*` cho toàn bộ, hoặc một thực thể kèm độ sâu), `GET /api/v1/graph/entities` cấp
  gợi ý cho ô tìm kiếm. UI tự sắp xếp bằng force layout vẽ trên SVG, không thêm thư viện đồ
  thị nào: kéo/thả node, lăn chuột để phóng to, bấm để xem mô tả và nguồn, bấm đúp để chỉ
  xem lân cận, chú giải cho phép tắt bớt loại thực thể. Đồ thị bị cắt vì giới hạn số node thì
  có cảnh báo ngay dưới khung vẽ.
- Nút microphone ưu tiên AudioWorklet, resample trực tiếp thành PCM float32 mono 16 kHz và gửi
  khung 320 ms qua binary WebSocket. FastAPI dùng binding shared-library của `transcribe.cpp`,
  cache Nemotron trong tiến trình và trả committed/tentative partial cùng transcript cuối vào
  composer. Webview cũ tự fallback về MediaRecorder + FFmpeg + CLI batch.
- Tải model Ollama có parser SSE chịu được network chunk bị chia nhỏ, hiển thị tiến trình và
  hủy request thật khi người dùng đóng hoặc bấm Hủy.
- Model Manager gộp model Ollama và Nemotron ASR, lưu default theo tác vụ, trạng thái
  load/unload, update/delete có xác nhận, SHA-256 của ASR và lịch sử thao tác/lỗi. UI chỉ đưa
  language model vào hộp chọn chat và hiển thị runtime/default/checksum tương ứng.
- Cài đặt có mục Nhà cung cấp AI: ngoài Ollama cục bộ dựng sẵn, người dùng thêm được máy chủ
  nói chuẩn OpenAI API (vLLM, LM Studio, LiteLLM, OpenAI…) kèm base URL và API key, kiểm tra
  kết nối trước khi lưu rồi chuyển sang dùng. Chat, embedding và trích xuất tri thức đều đi
  qua nhà cung cấp đang bật; model default cho embedding lưu trong SQLite nên giữ sau khi
  khởi động lại. Model từ xa không cho pull/unload/delete và trả 422 kèm lý do. Ollama cục bộ
  chỉ là bản ghi được seed sẵn ở lần chạy đầu: sửa tên, đổi địa chỉ (WSL2 hoặc máy khác trong
  mạng) hay xóa hẳn đều được, xóa rồi thì không seed lại. Đổi địa chỉ thì GPU lease và health
  đi theo host mới. Xóa hết nhà cung cấp thì health trả `not_configured` và chat trả 503 "No
  AI provider is configured" thay vì báo mất kết nối.
- Trạng thái hệ thống nói theo nhà cung cấp đang bật, không lấy Ollama làm đại diện cho AI:
  `services.provider` là trạng thái của endpoint đang dùng, còn `services.local_runtime` là máy
  chủ mô hình cục bộ và chỉ hiện trên bảng khi nhà cung cấp thật sự chạy trên máy. Nhãn "trên
  thiết bị" đọc từ `provider.on_device`, tính theo base URL có phải loopback hay không, nên bản
  ghi Ollama cục bộ trỏ sang WSL2 hoặc máy khác vẫn được cảnh báo là dữ liệu rời khỏi máy.

## Phần chưa hoàn tất so với `GOAL.md`

- ASR microphone đã stream PCM 16 kHz trực tiếp vào shared library và trả partial/final thật,
  nhưng chưa có VAD/endpoint detector riêng hoặc jitter buffer thích nghi như pipeline đích.
- Thanh GPU phản ánh reservation và `size_vram` từ Ollama, nhưng chưa đọc temperature hay
  utilization phần cứng. Hai process API/MCP đều kiểm tra inventory thật, song chưa có
  distributed lock để loại bỏ hoàn toàn race khi chúng cùng load model đúng một thời điểm.
- Desktop shell hiện có cửa sổ pywebview và local runtime; file picker, system tray và native
  notification của Windows chưa được triển khai vì các thao tác chính đang dùng web UI/API.
- Local runtime dùng argument list và đã có unit test, nhưng chưa được chạy end-to-end trên máy
  Windows trong đợt kiểm thử macOS này.

PDF scan chỉ được đánh dấu `needs_ocr` khi công cụ OCR chưa được cài hoặc không nhận ra chữ;
health luôn phản ánh trạng thái runtime thật, không giả lập service đang online.

## Yêu cầu

- Python 3.12 trở lên.
- Node.js 22 trở lên và Corepack (hoặc pnpm).
- Ollama nếu cần model local.
- Một model đọc được ảnh nếu cần OCR; worker tự nhận ra, hoặc chỉ định bằng nút "Dùng cho OCR".
- Git, CMake và FFmpeg nếu dùng voice-to-text; Windows cần Visual Studio Build Tools có C++.

Không cần Bash, Make, symlink hay đường dẫn cố định để chạy development workflow.

## Phát triển trên macOS

```text
python3 -m venv .venv
.venv/bin/python -m pip install -e "services/api[dev]" -e "apps/desktop[dev]"
npx --yes pnpm@10.17.1 --dir apps/web install
.venv/bin/python tools/dev.py
```

Mở `http://127.0.0.1:5173`. Chạy desktop shell sau khi build web:

```text
npx --yes pnpm@10.17.1 --dir apps/web build
.venv/bin/private-ai-desktop
```

## Cài đặt và build trên Windows PowerShell

```text
powershell -ExecutionPolicy Bypass -File .\tools\install-windows.ps1
```

Script mặc định tạo hoặc cập nhật Conda environment `private-ai` với Python 3.12 và Node.js 22;
cài API kèm RAG-Anything, desktop và frontend, chạy test + lint + typecheck, build frontend và
tạo wheel Python trong `dist\python`. Có thể tùy chỉnh:

```text
.\tools\install-windows.ps1 -EnvironmentName private-ai-dev -PythonVersion 3.13
.\tools\install-windows.ps1 -SkipChecks
```

Chỉ dùng Python 3.12 hoặc 3.13 cho bản có RAG-Anything; dependency MinerU hiện chưa hỗ trợ
Python 3.14.

Sau khi build, chạy development services hoặc desktop shell mà không cần activate environment:

```text
conda run --no-capture-output -n private-ai python tools\dev.py
conda run --no-capture-output -n private-ai private-ai-desktop
```

`tools/dev.py` gọi process bằng argument list, dùng `os.pathsep` và `pathlib`, nên tên thư mục
có khoảng trắng không bị lỗi shell quoting. Lệnh development khởi động cả API, MCP và frontend;
dùng `--no-mcp` nếu chỉ cần API + web.

## MCP local

MCP server dùng Streamable HTTP và chỉ bind loopback:

```text
.venv/bin/private-ai-mcp
```

Trên Windows dùng `conda run --no-capture-output -n private-ai private-ai-mcp`. Bearer token được tạo một lần tại
`.local-data/mcp-token`; MCP client kết nối tới `http://127.0.0.1:8010/mcp` và gửi header
`Authorization: Bearer <token>`. Server không expose `models.delete`; xóa document hoặc memory
đều yêu cầu `confirmed=true`.

## Chạy desktop Windows với API local

Sau khi cài các package vào Conda environment `private-ai`, desktop launcher tự khởi động API
local bằng chính Python executable của environment đó:

```text
conda run --no-capture-output -n private-ai private-ai-desktop
```

Launcher yêu cầu **Microsoft Edge WebView2 Runtime**. Thiếu runtime này pywebview tự hạ xuống
engine Internet Explorer (MSHTML) và không chạy được giao diện, nên launcher dừng sớm kèm link
tải thay vì mở một cửa sổ trắng.

Launcher chỉ đợi `GET /api/v1/health/live`, không đợi `GET /api/v1/health`: endpoint đầy đủ còn
gọi sang Ollama và provider đang chọn nên có thể mất vài giây khi các service đó chưa sẵn sàng.
Probe cũng bỏ qua system proxy, vì proxy Windows không tự loại trừ `127.0.0.1` (`<local>` chỉ
khớp host không có dấu chấm) và sẽ nuốt luôn request tới API local.

Khi API không khởi động được, launcher in nguyên nhân kèm phần cuối log
`.local-data/desktop-api.log`. Biến môi trường liên quan: `PRIVATE_AI_HOST`, `PRIVATE_AI_PORT`,
`PRIVATE_AI_PROJECT_DIR`, `PRIVATE_AI_DATA_DIR`, `PRIVATE_AI_FRONTEND_DIST`.

## Knowledge graph

Không cần cài gì thêm. LightRAG chạy trong tiến trình API và ghi chỉ mục xuống
`.local-data/lightrag/<workspace>`. Chỉ mục dùng model embedding đang đặt mặc định cho tác vụ
`embedding` và model chat đang đặt mặc định cho tác vụ `chat`; đổi model embedding sẽ dựng lại
chỉ mục vì số chiều vector thay đổi. Hộp soạn tin có hai retrieval mode: `RAG nhanh` dùng vector
search (`naive`), còn `Graph RAG` kết hợp vector và knowledge graph (`mix`). Bản cài Windows bật
RAG-Anything mặc định; môi trường phát triển khác có thể cài bằng `pip install -e
'services/api[rag]'`. Nếu extra chưa có, ingestion tự dùng LightRAG trực tiếp. Đặt
`PRIVATE_AI_EMBEDDING_ENABLED=false` để tắt hẳn.

## Voice-to-text

Sau khi cài package API, build `transcribe.cpp` và tải model Nemotron Q4 bằng một lệnh:

```text
.venv/bin/private-ai-asr setup
```

Trên Windows dùng `conda run --no-capture-output -n private-ai private-ai-asr setup`. Lệnh dùng CMake bằng argument
list, tự nhận Metal trên Apple Silicon, tạo CLI batch và một build shared riêng cho binding
Python; MSVC đặt DLL trong thư mục `bin/Release`. Runtime/model được lưu trong
`.local-data/asr`; `private-ai-asr status` báo riêng trạng thái batch và native streaming.
Ngôn ngữ mặc định là `vi-VN`, có thể đổi qua `PRIVATE_AI_ASR_LANGUAGE`.

Capacity GPU được nhận diện theo máy. Trên Apple Silicon không có VRAM riêng nên ngân sách
lấy từ `iogpu.wired_limit_mb` nếu được đặt, ngược lại là phần RAM macOS dành mặc định cho GPU
(khoảng 75% với máy nhiều RAM). Các nền tảng khác giữ mặc định 96 GiB. Có thể ép giá trị khác:

```text
PRIVATE_AI_GPU_CAPACITY_BYTES=103079215104
PRIVATE_AI_GPU_MODEL_OVERHEAD_RATIO=1.1
PRIVATE_AI_ASR_VRAM_RESERVATION_BYTES=2147483648
```

Entity/relation extraction bằng LLM là tùy chọn vì có thể tốn thời gian với tài liệu dài:

```text
PRIVATE_AI_GRAPH_ENTITY_MODEL=qwen3.8:27b-mlx
```

Để OCR vision cho ảnh và ảnh nhúng trong PDF/Office, cài một model Ollama có capability
`vision`, bấm **Dùng cho OCR** trong màn hình Models hoặc đặt:

```text
PRIVATE_AI_VISION_MODEL=qwen3-vl:8b
```

Chưa chọn vision model thì JPG/PNG và PDF scan vào `needs_ocr` kèm lý do; tài liệu không bị
báo `Unsupported document type`.

Structured output được validate, có một lần retry JSON cho model không tuân thủ schema hoàn
toàn; chunk thất bại được ghi vào job `graph_extraction` và sẽ được thử lại thay vì thay bằng
dữ liệu giả.

## Kiểm tra

Lần kiểm tra runtime gần nhất trên macOS (2026-08-27) đạt 43 test, Ruff, TypeScript
typecheck và Vite production build. Smoke test qua HTTP/WebSocket đã xác nhận workspace/chat
với Qwen, lưu lại message, upload/search/xóa document, CRUD/search memory, health,
Nemotron load/unload, ASR partial/final và JPG OCR qua vision model; dữ liệu tạm của smoke
test được xóa sau khi chạy.

```text
.venv/bin/python -m pytest
.venv/bin/python -m ruff check .
npx --yes pnpm@10.17.1 --dir apps/web typecheck
npx --yes pnpm@10.17.1 --dir apps/web build
```

Trên Windows, chạy các lệnh Python qua `conda run --no-capture-output -n private-ai python ...`.
