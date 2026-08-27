# Private AI

Private AI là desktop control plane cho mô hình, tài liệu và memory chạy cục bộ. UI native nằm trên máy người dùng; AI runtime có thể chạy local khi phát triển trên macOS hoặc trong Ubuntu WSL2 khi phát hành cho Windows.

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
- Chunk giữ metadata section và trang cho tài liệu mới. Neo4j tạo chuỗi
  `Document -> Section -> Chunk`, nhãn entity theo loại, mở rộng graph có giới hạn 1-2 hop và
  rerank cục bộ; citation trong chat kèm file, trang, section và chunk ID khi nguồn có dữ liệu.
- `embeddinggemma` là embedding model mặc định. Có thể đổi bằng
  `PRIVATE_AI_EMBEDDING_MODEL`; khi model không sẵn sàng, hệ thống tự fallback về keyword.
- MarkItDown và plugin `markitdown-ocr` được bật trong worker. Khi chọn vision model, plugin
  gọi Ollama qua OpenAI-compatible API để giữ heading/list/table và đọc ảnh nhúng trong
  PDF/Office; JPG/PNG cũng đi qua MarkItDown vision. Nếu không có vision model, ảnh và PDF
  scan tự fallback sang Tesseract/Poppler. Ngôn ngữ OCR cấu hình bằng
  `PRIVATE_AI_OCR_LANGUAGES=vie+eng`.
- Memory có tạo, sửa, bật/tắt, xóa và semantic search. SQLite giữ bản chuẩn cùng embedding
  cache; Neo4j giữ `Memory`/`User`, quan hệ `BELONGS_TO` và vector index 768 chiều. Chat chỉ
  lấy top-k memory liên quan và fallback về keyword/recent khi Ollama hoặc Neo4j ngoại tuyến;
  UI cho phép xuất toàn bộ memory thành JSON.
- `GpuLeaseManager` đồng bộ model Ollama đang chạy từ `/api/ps`, reserve trước chat/embedding.
  ASR batch giữ 2 GiB trong lúc transcription; ASR streaming giữ lease trong lúc native model
  được cache và giải phóng khi API shutdown. Request mới bị từ chối trước khi load nếu vượt
  capacity cấu hình.
- SolidJS dashboard responsive, hiển thị health/model/VRAM; các màn hình Chat Workspaces,
  Library, Memory, Models và Settings đều nối với API.
- Màn hình chính Chat Workspaces, light mode mặc định, dark mode tùy chọn và chế độ chữ lớn; lựa chọn được lưu cục bộ trên thiết bị.
- `pywebview` desktop launcher với runtime adapter local/WSL.
- MCP Python SDK v2 server tại `http://127.0.0.1:8010/mcp`, có bearer token cục bộ,
  Origin/Host validation và 21 tool cho document, GraphRAG, memory, model inventory/default.
- Neo4j GraphRAG đồng bộ `Document`/`Chunk`/`Entity`, tạo vector + full-text index và dùng
  `HybridRetriever`; chat vẫn fallback về SQLite khi Neo4j tắt.
- Khi đặt `PRIVATE_AI_GRAPH_ENTITY_MODEL`, worker gọi Ollama structured output để trích xuất
  entity/relation, lưu facts cùng provenance model trong SQLite rồi tạo quan hệ Neo4j. Nếu
  không cấu hình, ingestion nhanh vẫn dùng heuristic entity/co-occurrence.
- Neo4j Compose chỉ expose loopback và không còn mật khẩu mặc định.
- Nút microphone ưu tiên AudioWorklet, resample trực tiếp thành PCM float32 mono 16 kHz và gửi
  khung 320 ms qua binary WebSocket. FastAPI dùng binding shared-library của `transcribe.cpp`,
  cache Nemotron trong tiến trình và trả committed/tentative partial cùng transcript cuối vào
  composer. Webview cũ tự fallback về MediaRecorder + FFmpeg + CLI batch.
- Tải model Ollama có parser SSE chịu được network chunk bị chia nhỏ, hiển thị tiến trình và
  hủy request thật khi người dùng đóng hoặc bấm Hủy.
- Model Manager gộp model Ollama và Nemotron ASR, lưu default theo tác vụ, trạng thái
  load/unload, update/delete có xác nhận, SHA-256 của ASR và lịch sử thao tác/lỗi. UI chỉ đưa
  language model vào hộp chọn chat và hiển thị runtime/default/checksum tương ứng.

## Phần chưa hoàn tất so với `GOAL.md`

- ASR microphone đã stream PCM 16 kHz trực tiếp vào shared library và trả partial/final thật,
  nhưng chưa có VAD/endpoint detector riêng hoặc jitter buffer thích nghi như pipeline đích.
- Thanh GPU phản ánh reservation và `size_vram` từ Ollama, nhưng chưa đọc temperature hay
  utilization phần cứng. Hai process API/MCP đều kiểm tra inventory thật, song chưa có
  distributed lock để loại bỏ hoàn toàn race khi chúng cùng load model đúng một thời điểm.
- Desktop shell hiện có cửa sổ pywebview và runtime adapter; file picker, system tray và native
  notification của Windows chưa được triển khai vì các thao tác chính đang dùng web UI/API.
- Adapter Windows/WSL dùng argument list và đã có unit test, nhưng chưa được chạy end-to-end
  trên máy Windows trong đợt kiểm thử macOS này.

PDF scan chỉ được đánh dấu `needs_ocr` khi công cụ OCR chưa được cài hoặc không nhận ra chữ;
health luôn phản ánh trạng thái runtime thật, không giả lập service đang online.

## Yêu cầu

- Python 3.12 trở lên.
- Node.js 22 trở lên và Corepack (hoặc pnpm).
- Ollama nếu cần model local.
- Docker Desktop/Engine nếu cần Neo4j.
- Poppler và Tesseract là fallback cục bộ cho ảnh/PDF scan khi chưa chọn vision model. Script
  Windows cài cả hai vào Conda environment; nếu dùng `-SkipNativeTools`, hãy tự thêm thư mục
  chứa `pdftoppm.exe` và `tesseract.exe` vào `PATH`.
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

Script mặc định tạo hoặc cập nhật Conda environment `private-ai` với Python 3.12, Node.js 22,
Poppler và Tesseract; cài API/desktop/frontend, chạy test + lint + typecheck, build frontend và
tạo wheel Python trong `dist\python`. Có thể tùy chỉnh:

```text
.\tools\install-windows.ps1 -EnvironmentName private-ai-dev -PythonVersion 3.13
.\tools\install-windows.ps1 -SkipChecks
.\tools\install-windows.ps1 -SkipNativeTools
```

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

## Chạy desktop Windows với API trong WSL2

Cài project và virtualenv vào filesystem Linux, ví dụ `/opt/private-ai`, rồi đặt biến môi trường trên Windows:

```text
PRIVATE_AI_DESKTOP_RUNTIME=wsl
PRIVATE_AI_WSL_DISTRO=Ubuntu-24.04
PRIVATE_AI_WSL_PROJECT_DIR=/opt/private-ai
PRIVATE_AI_WSL_API_EXECUTABLE=/opt/private-ai/.venv/bin/private-ai-api
```

Launcher gọi trực tiếp `wsl.exe --distribution ... --cd ... -- executable`, không tạo command string qua `cmd.exe` hay Bash. Trên macOS, `auto` luôn chọn runtime `local`.

## Neo4j

```text
.venv/bin/private-ai-neo4j up
```

Trên Windows dùng `conda run --no-capture-output -n private-ai private-ai-neo4j up`. Helper tạo mật khẩu mạnh tại
`.local-data/neo4j-password`, truyền bằng environment cho Compose và không in secret ra màn
hình. Dùng action `status`, `logs` hoặc `down` để quản lý container. Có thể override bằng
`PRIVATE_AI_NEO4J_PASSWORD`; đặt `PRIVATE_AI_NEO4J_ENABLED=false` để tắt hoàn toàn GraphRAG.

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

Capacity GPU mặc định là 96 GiB theo máy đích trong `GOAL.md`. Có thể cấu hình cho máy khác:

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

Không cấu hình vision model vẫn hỗ trợ JPG/PNG qua Tesseract; tài liệu không bị báo
`Unsupported document type`.

Structured output được validate, có một lần retry JSON cho model không tuân thủ schema hoàn
toàn; chunk thất bại được ghi vào job `graph_extraction` và sẽ được thử lại thay vì thay bằng
dữ liệu giả.

## Kiểm tra

Lần kiểm tra runtime gần nhất trên macOS (2026-08-27) đạt 43 test, Ruff, TypeScript
typecheck và Vite production build. Smoke test qua HTTP/WebSocket đã xác nhận workspace/chat
với Qwen, lưu lại message, upload/search/xóa document, CRUD/search memory, Neo4j health,
Nemotron load/unload, ASR partial/final và JPG OCR thật qua Tesseract; dữ liệu tạm của smoke
test được xóa sau khi chạy.

```text
.venv/bin/python -m pytest
.venv/bin/python -m ruff check .
npx --yes pnpm@10.17.1 --dir apps/web typecheck
npx --yes pnpm@10.17.1 --dir apps/web build
```

Trên Windows, chạy các lệnh Python qua `conda run --no-capture-output -n private-ai python ...`.
