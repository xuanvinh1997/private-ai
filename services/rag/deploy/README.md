# Hạ tầng lưu trữ cho tầng RAG

Hai container: **Qdrant** giữ vector dense của từng đoạn tài liệu, **Neo4j** giữ knowledge
graph (`Document → Section → Chunk → Entity`). Client duy nhất là tiến trình
`pai-rag-service` chạy cùng máy.

Cả hai chỉ nghe trên `127.0.0.1`. Không có cluster, không có replica: một máy, một người
dùng, và mọi con số tinh chỉnh trong `docker-compose.yml` được chọn cho đúng hình dạng đó
— lý do viết ngay cạnh từng con số, đọc file để biết vì sao.

Chạy các lệnh dưới đây **trong WSL2** (Ubuntu), nơi Docker và phần còn lại của AI runtime
sống. Chạy từ Windows cũng được nhưng lúc đó `127.0.0.1` của bạn không phải `127.0.0.1`
của tiến trình RAG.

## Khởi động

```bash
cd services/rag/deploy
cp .env.example .env

# Sinh hai bí mật rồi dán vào NEO4J_PASSWORD và QDRANT_API_KEY
python3 -c "import secrets; print(secrets.token_urlsafe(32))"

docker compose up -d
```

Lần đầu sẽ tải image (~600 MB) — đây là lần duy nhất cần mạng. Sau đó:

```bash
docker compose ps
```

Chờ đến khi cả hai dòng đều `healthy`. Neo4j mất khoảng 30–60 giây cho lần khởi động đầu
(JVM, nạp APOC, đặt mật khẩu ban đầu).

Nếu `.env` thiếu một trong hai mật khẩu, compose dừng ngay với thông báo tên biến còn
thiếu. Đó là chủ ý: thà không chạy còn hơn chạy mà không có khoá.

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

**Neo4j.** Bolt cộng một truy vấn thật — cổng mở chưa chứng minh database đã online:

```bash
docker exec pai-neo4j cypher-shell -u neo4j -p "$NEO4J_PASSWORD" "RETURN 1 AS ok"
# ok
# 1

# APOC đã nạp
docker exec pai-neo4j cypher-shell -u neo4j -p "$NEO4J_PASSWORD" "RETURN apoc.version()"

# Heap và page cache có đúng như đặt trong .env không
docker exec pai-neo4j cypher-shell -u neo4j -p "$NEO4J_PASSWORD" \
  "CALL dbms.listConfig('server.memory') YIELD name, value RETURN name, value"
```

**Page cache có còn đủ không.** Con số duy nhất cần theo dõi theo thời gian: kích thước
store trên đĩa. Khi nó tiến gần `NEO4J_PAGECACHE_SIZE`, truy vấn bắt đầu chạm đĩa và
graph traversal chậm hẳn — lúc đó nâng page cache lên.

```bash
docker exec pai-neo4j du -sh /data/databases/neo4j
```

Xem log khi có gì đó không ổn:

```bash
docker compose logs -f neo4j
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

**Cả hai — nguội, đầy đủ.** Cách này sao lưu đúng những gì trên đĩa, kể cả cấu hình
collection và toàn bộ graph. Phải dừng container trước: chép một store đang có ghi dở là
chép một bản hỏng.

```bash
mkdir -p backups
docker compose stop

docker run --rm --user root --entrypoint tar \
  -v pai-qdrant-storage:/data:ro -v "$PWD/backups:/backup" \
  qdrant/qdrant:v1.19.0 czf /backup/qdrant-storage.tar.gz -C /data .

docker run --rm --user root --entrypoint tar \
  -v pai-neo4j-data:/data:ro -v "$PWD/backups:/backup" \
  neo4j:5.26.30-community czf /backup/neo4j-data.tar.gz -C /data .

docker compose start
```

**Khôi phục.** Đổ ngược vào volume rỗng — xoá volume cũ trước, nếu không hai bản dữ liệu
sẽ trộn vào nhau:

```bash
docker compose down
docker volume rm pai-qdrant-storage pai-neo4j-data
docker volume create pai-qdrant-storage
docker volume create pai-neo4j-data

docker run --rm --user root --entrypoint tar \
  -v pai-qdrant-storage:/data -v "$PWD/backups:/backup" \
  qdrant/qdrant:v1.19.0 xzf /backup/qdrant-storage.tar.gz -C /data

docker run --rm --user root --entrypoint tar \
  -v pai-neo4j-data:/data -v "$PWD/backups:/backup" \
  neo4j:5.26.30-community xzf /backup/neo4j-data.tar.gz -C /data

docker compose up -d
```

## Đổi mật khẩu

`NEO4J_AUTH` chỉ có tác dụng ở lần khởi tạo đầu tiên; sau đó mật khẩu nằm trong volume.
Đổi thật thì phải đổi trong database, rồi cập nhật `.env` để healthcheck vẫn đăng nhập
được:

```bash
docker exec pai-neo4j cypher-shell -u neo4j -p "$NEO4J_PASSWORD" -d system \
  "ALTER CURRENT USER SET PASSWORD FROM '$NEO4J_PASSWORD' TO 'mật-khẩu-mới'"
# sửa NEO4J_PASSWORD trong .env, rồi:
docker compose up -d
```

API key của Qdrant thì đơn giản hơn — nó chỉ sống trong biến môi trường:

```bash
# sửa QDRANT_API_KEY trong .env
docker compose up -d qdrant
```

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

Hai image đều được ghim đúng một phiên bản trong `docker-compose.yml`, cố ý. Cả Qdrant lẫn
Neo4j đều nâng cấp định dạng store một chiều — lên được, không xuống được. Khi muốn nâng:
sao lưu nguội trước, đổi tag, `docker compose up -d`, rồi chạy lại phần kiểm tra sức khoẻ
ở trên. Không nhảy cách nhánh minor của Neo4j (5.26 → 2026.x là nâng cấp lớn, đọc release
notes trước).
