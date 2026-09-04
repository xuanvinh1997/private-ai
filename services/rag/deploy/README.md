# Hạ tầng lưu trữ cho tầng RAG

Một container: **Qdrant**, giữ vector dense của từng đoạn tài liệu. Client duy nhất là
tiến trình `pai-rag-service` chạy cùng máy.

Đồ thị thực thể **không** ở đây nữa. Nó chạy bằng SurrealDB nhúng: `pai-rag-service` mở
thẳng một thư mục cạnh SQLite của từng dự án, không container, không JVM, không mật khẩu.
Muốn đưa nó ra thành tiến trình riêng (để nhiều tiến trình cùng đọc, hoặc để ứng dụng giữ
vòng đời của nó) thì chạy `surreal` rồi trỏ `PAI_RAG_GRAPH_URL` vào đấy.

Qdrant chỉ nghe trên `127.0.0.1`. Không có cluster, không có replica: một máy, một người
dùng, và mọi con số tinh chỉnh trong `docker-compose.yml` được chọn cho đúng hình dạng đó
— lý do viết ngay cạnh từng con số, đọc file để biết vì sao.

Chạy các lệnh dưới đây **trong WSL2** (Ubuntu), nơi Docker và phần còn lại của AI runtime
sống. Chạy từ Windows cũng được nhưng lúc đó `127.0.0.1` của bạn không phải `127.0.0.1`
của tiến trình RAG.

## Khởi động

```bash
cd services/rag/deploy
cp .env.example .env

# Sinh một bí mật rồi dán vào QDRANT_API_KEY
python3 -c "import secrets; print(secrets.token_urlsafe(32))"

docker compose up -d
```

Lần đầu sẽ tải image (~150 MB) — đây là lần duy nhất cần mạng. Sau đó:

```bash
docker compose ps
```

Chờ đến khi dòng `qdrant` báo `healthy`.

Nếu `.env` thiếu `QDRANT_API_KEY`, compose dừng ngay với thông báo tên biến còn thiếu. Đó
là chủ ý: thà không chạy còn hơn chạy mà không có khoá.

## Kiểm tra sức khoẻ

Nạp `.env` vào shell trước:

```bash
set -a; . ./.env; set +a
```

**Qdrant.** `/readyz` không cần api-key (Qdrant miễn xác thực cho endpoint sức khoẻ) và
chỉ trả 200 sau khi mọi shard đã nạp xong:

```bash
curl -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:6333/readyz
# 200

# Danh sách collection — chỗ này thì cần khoá
curl -sS -H "api-key: $QDRANT_API_KEY" http://127.0.0.1:6333/collections
# {"result":{"collections":[...]},"status":"ok",...}

# Không có khoá thì phải bị từ chối. Nếu lệnh này trả 200, api-key chưa có tác dụng.
curl -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:6333/collections
# 401
```

Tạo thử một collection để xác nhận quantization mặc định đã được áp (2560 chiều ứng với
`qwen3-embedding:4b`; đổi theo model embedding bạn dùng):

```bash
curl -sS -X PUT -H "api-key: $QDRANT_API_KEY" -H 'content-type: application/json' \
  http://127.0.0.1:6333/collections/probe \
  -d '{"vectors":{"size":2560,"distance":"Cosine"}}'

curl -sS -H "api-key: $QDRANT_API_KEY" \
  http://127.0.0.1:6333/collections/probe | grep -o '"quantization_config":[^}]*}[^}]*}'
# ..."scalar":{"type":"int8","quantile":0.99,"always_ram":true}...

curl -sS -X DELETE -H "api-key: $QDRANT_API_KEY" http://127.0.0.1:6333/collections/probe
```

**Đồ thị.** Không có container để hỏi — kho nằm trong tiến trình RAG. `pai-rag doctor` mở
nó và in số thực thể cùng số quan hệ; đó là phép kiểm duy nhất có nghĩa:

```bash
cd services/rag && uv run pai-rag doctor
# graph         : OK  surrealkv:///…/thu-vien/graph → 128 thực thể, 341 quan hệ
```

Kho khoá độc quyền thư mục của nó, nên "không mở được" gần như luôn có nghĩa là ứng dụng
đang chạy và đang giữ nó. Đóng ứng dụng rồi thử lại.

Xem log khi có gì đó không ổn:

```bash
docker compose logs -f qdrant
```

## Sao lưu

**Qdrant — nóng, không cần dừng dịch vụ.** Qdrant tự tạo snapshot nhất quán của một
collection:

```bash
mkdir -p backups
curl -sS -X POST -H "api-key: $QDRANT_API_KEY" \
  http://127.0.0.1:6333/collections/pai_chunks/snapshots
docker cp pai-qdrant:/qdrant/snapshots ./backups/qdrant-snapshots
```

**Nguội, đầy đủ.** Cách này sao lưu đúng những gì trên đĩa, kể cả cấu hình collection.
Phải dừng container trước: chép một store đang có ghi dở là chép một bản hỏng.

```bash
mkdir -p backups
docker compose stop

docker run --rm --user root --entrypoint tar \
  -v pai-qdrant-storage:/data:ro -v "$PWD/backups:/backup" \
  qdrant/qdrant:v1.19.0 czf /backup/qdrant-storage.tar.gz -C /data .

docker compose start
```

Đồ thị nằm ngoài Docker nên nó không đi theo hai lệnh trên. Nó là thư mục cạnh SQLite của
từng dự án, và cách sao lưu là chép cả thư mục dữ liệu của ứng dụng lúc ứng dụng đang
đóng — cùng lúc với `rag.sqlite`, để hai kho không lệch nhau về một tài liệu.

**Khôi phục.** Đổ ngược vào volume rỗng — xoá volume cũ trước, nếu không hai bản dữ liệu
sẽ trộn vào nhau:

```bash
docker compose down
docker volume rm pai-qdrant-storage
docker volume create pai-qdrant-storage

docker run --rm --user root --entrypoint tar \
  -v pai-qdrant-storage:/data -v "$PWD/backups:/backup" \
  qdrant/qdrant:v1.19.0 xzf /backup/qdrant-storage.tar.gz -C /data

docker compose up -d
```

## Đổi api-key

Khoá của Qdrant chỉ sống trong biến môi trường, nên đổi nó là sửa `.env` rồi dựng lại:

```bash
# sửa QDRANT_API_KEY trong .env
docker compose up -d qdrant
```

Đồ thị nhúng không có khoá nào để đổi: nó là một thư mục, và quyền trên thư mục ấy là
quyền của hệ điều hành.

## Hạ xuống

```bash
docker compose stop     # tạm dừng, dữ liệu và container còn nguyên
docker compose down     # xoá container, GIỮ volume — dựng lại là có lại dữ liệu
```

Xoá sạch cả dữ liệu (làm lại chỉ mục từ đầu). Không hoàn tác được:

```bash
docker compose down -v
```

## Nâng cấp phiên bản

Image được ghim đúng một phiên bản trong `docker-compose.yml`, cố ý: Qdrant nâng cấp định
dạng store một chiều — lên được, không xuống được. Khi muốn nâng: sao lưu nguội trước, đổi
tag, `docker compose up -d`, rồi chạy lại phần kiểm tra sức khoẻ ở trên.

SurrealDB thì đi theo wheel `surrealdb` trong `pyproject.toml`, nên nó nâng cùng lúc với
phần còn lại của service — cùng một ràng buộc một chiều, và cùng một cách phòng: sao lưu
thư mục dữ liệu trước khi nâng.
