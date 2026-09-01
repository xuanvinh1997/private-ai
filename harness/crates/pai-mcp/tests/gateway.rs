//! Cổng HTTP: bốn lớp canh, kiểm từng lớp một mà không mở cổng nào.
//!
//! [`HttpGuard`] cố tình không biết gì về socket, nên bài kiểm chứng dựng thẳng một
//! `http::Request` rồi hỏi. Một luật bảo mật chỉ kiểm được qua một cổng thật là một luật
//! sẽ không có ai kiểm.

use std::net::SocketAddr;

use pai_mcp::{Denied, HttpGuard, McpToken, constant_time_eq, token_path};

const SECRET: &str = "cf3a9d0e11b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c";

fn guard(origins: &[&str]) -> HttpGuard {
    HttpGuard::new(
        McpToken::from_value(SECRET),
        origins.iter().map(|s| s.to_string()).collect(),
    )
}

/// Dựng phần đầu của một request. `headers` là các cặp `(tên, giá trị)`.
fn head(headers: &[(&str, &str)]) -> http::request::Parts {
    let mut builder = http::Request::builder().uri("/mcp");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(()).expect("dựng được request").into_parts().0
}

fn bearer() -> String {
    format!("Bearer {SECRET}")
}

/// Token đúng, `Host` loopback, không `Origin` — đường của một client MCP thật.
#[test]
fn duong_hop_le_di_qua() {
    let parts = head(&[("host", "127.0.0.1:9000"), ("authorization", &bearer())]);
    assert_eq!(guard(&[]).check(&parts), Ok(()));
}

/// **Token sai bị từ chối.** Không có lớp này thì mọi tiến trình trên máy đều gọi được `bash`.
#[test]
fn token_sai_bi_tu_choi() {
    let wrong = format!("Bearer {}", "0".repeat(SECRET.len()));
    let parts = head(&[("host", "127.0.0.1"), ("authorization", &wrong)]);
    assert_eq!(guard(&[]).check(&parts), Err(Denied::Auth));
}

/// Thiếu token, sai lược đồ, hay token đúng nhưng cụt một ký tự — đều là từ chối.
#[test]
fn moi_kieu_token_hong_deu_bi_tu_choi() {
    let g = guard(&[]);
    assert_eq!(g.check(&head(&[("host", "localhost")])), Err(Denied::Auth));

    let basic = format!("Basic {SECRET}");
    assert_eq!(
        g.check(&head(&[("host", "localhost"), ("authorization", &basic)])),
        Err(Denied::Auth)
    );

    let truncated = format!("Bearer {}", &SECRET[..SECRET.len() - 1]);
    assert_eq!(
        g.check(&head(&[
            ("host", "localhost"),
            ("authorization", &truncated)
        ])),
        Err(Denied::Auth)
    );

    let extended = format!("Bearer {SECRET}0");
    assert_eq!(
        g.check(&head(&[
            ("host", "localhost"),
            ("authorization", &extended)
        ])),
        Err(Denied::Auth)
    );
}

/// **Chống DNS rebinding**: kết nối *là* loopback, nhưng `Host` mang tên miền kẻ tấn công.
///
/// Đây là thứ mà "chỉ bind 127.0.0.1" không nhìn thấy được: trang web trỏ tên miền của nó
/// về loopback rồi cho JavaScript gọi vào đây, và socket trông hoàn toàn bình thường.
#[test]
fn host_khong_phai_loopback_bi_tu_choi() {
    let g = guard(&[]);
    for host in ["evil.example.com", "evil.example.com:9000", "10.0.0.7:9000"] {
        assert_eq!(
            g.check(&head(&[("host", host), ("authorization", &bearer())])),
            Err(Denied::Host),
            "{host} không được coi là loopback"
        );
    }
    // Không có `Host` thì không kiểm được, và không kiểm được là từ chối.
    assert_eq!(
        g.check(&head(&[("authorization", &bearer())])),
        Err(Denied::Host)
    );
}

/// Ba cách viết loopback đều được nhận, có cổng hay không.
#[test]
fn moi_cach_viet_loopback_deu_duoc_nhan() {
    let g = guard(&[]);
    for host in [
        "127.0.0.1",
        "127.0.0.1:9000",
        "localhost",
        "LOCALHOST:9000",
        "[::1]",
        "[::1]:9000",
    ] {
        assert_eq!(
            g.check(&head(&[("host", host), ("authorization", &bearer())])),
            Ok(()),
            "{host} phải được coi là loopback"
        );
    }
}

/// Danh sách `Origin` rỗng = từ chối mọi request có mang `Origin`.
///
/// Chỉ trình duyệt gửi `Origin`; một client MCP thì không. Cấu hình trống là cấu hình chặt
/// nhất, đúng như `FileRoots` không có gốc nào thì không đọc được gì.
#[test]
fn origin_khong_trong_danh_sach_trang_bi_tu_choi() {
    let parts = head(&[
        ("host", "127.0.0.1:9000"),
        ("origin", "https://evil.example.com"),
        ("authorization", &bearer()),
    ]);
    assert_eq!(guard(&[]).check(&parts), Err(Denied::Origin));
    assert_eq!(
        guard(&["http://localhost:1420"]).check(&parts),
        Err(Denied::Origin)
    );
    assert_eq!(
        guard(&["https://evil.example.com"]).check(&parts),
        Ok(()),
        "một origin được khai tường minh thì đi qua"
    );
}

/// Lớp `Host` chạy trước lớp token: một request rebinding bị chặn trước khi ta nhìn tới bí mật.
#[test]
fn host_duoc_kiem_truoc_token() {
    let parts = head(&[
        ("host", "evil.example.com"),
        ("authorization", "Bearer sai"),
    ]);
    assert_eq!(guard(&[]).check(&parts), Err(Denied::Host));
}

/// So sánh hằng thời gian vẫn phải là một phép so sánh đúng.
#[test]
fn so_sanh_hang_thoi_gian_van_dung_ket_qua() {
    assert!(constant_time_eq(b"abc", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abd"));
    assert!(!constant_time_eq(b"abc", b"ab"));
    assert!(!constant_time_eq(b"ab", b"abc"));
    assert!(constant_time_eq(b"", b""));
    assert!(!constant_time_eq(b"", b"a"));
}

/// Token sinh một lần, cất ở `data_dir/mcp-token`, và **không** đổi giữa hai lần khởi động.
#[test]
fn token_sinh_mot_lan_va_giu_nguyen() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = token_path(dir.path());
    assert!(path.ends_with("mcp-token"));

    let first = McpToken::load_or_create(&path).expect("sinh được token");
    assert_eq!(first.as_str().len(), 64, "256 bit viết dạng hex");
    assert!(first.as_str().chars().all(|c| c.is_ascii_hexdigit()));

    let second = McpToken::load_or_create(&path).expect("đọc lại token");
    assert_eq!(first.as_str(), second.as_str());
    assert!(second.matches(first.as_str()));
    assert!(!second.matches("sai"));
}

/// Tệp token không in bí mật ra khi bị `Debug`. Một token lọt vào log là một token đã mất.
#[test]
fn token_khong_lot_vao_log() {
    let token = McpToken::from_value(SECRET);
    assert!(!format!("{token:?}").contains(SECRET));
}

#[cfg(unix)]
/// Tệp token sinh ra **đã** là `0600`, và một tệp lỏng quyền bị siết lại lúc đọc.
#[test]
fn tep_token_luon_la_0600() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = token_path(dir.path());

    let token = McpToken::load_or_create(&path).expect("sinh được token");
    let mode = std::fs::metadata(&path)
        .expect("đọc được metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);

    // Ai đó nới quyền ra; lần đọc sau phải siết lại.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("đổi được quyền");
    let again = McpToken::load_or_create(&path).expect("đọc lại token");
    assert_eq!(again.as_str(), token.as_str());
    let mode = std::fs::metadata(&path)
        .expect("đọc được metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// Cổng HTTP từ chối bind ra ngoài loopback — sổ đăng ký tool không phải một dịch vụ mạng.
#[tokio::test]
async fn khong_bind_duoc_ra_ngoai_loopback() {
    use std::sync::Arc;

    use pai_core::Context;
    use pai_mcp::{RegistryServer, serve_http};
    use pai_tools::{ToolPipeline, ToolRegistry};
    use tokio_util::sync::CancellationToken;

    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let server = RegistryServer::new(Arc::new(ToolPipeline::new(&ctx, registry)));
    let bind: SocketAddr = "0.0.0.0:0".parse().expect("địa chỉ hợp lệ");

    let result = serve_http(
        server,
        bind,
        McpToken::from_value(SECRET),
        Vec::new(),
        CancellationToken::new(),
    )
    .await;
    assert!(result.is_err(), "bind 0.0.0.0 phải bị từ chối");
}

// --- cổng thật, trên loopback ------------------------------------------------------------
//
// Bốn bài dưới đây mở một cổng ở `127.0.0.1:0` và gõ HTTP thô vào nó bằng một `TcpStream`.
// Không có gói tin nào rời máy và không có phụ thuộc nào vào Internet — cái được kiểm là
// đoạn keo giữa `hyper` và `HttpGuard`, thứ mà một bài kiểm chứng thuần hàm không chạm tới.

use std::sync::Arc;

use pai_core::Context;
use pai_mcp::{HttpEndpoint, RegistryServer, serve_http};
use pai_tools::{ToolPipeline, ToolRegistry};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn open_gateway(origins: &[&str]) -> (HttpEndpoint, Context) {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let server = RegistryServer::new(Arc::new(ToolPipeline::new(&ctx, registry)));
    let endpoint = serve_http(
        server,
        "127.0.0.1:0".parse().expect("địa chỉ hợp lệ"),
        McpToken::from_value(SECRET),
        origins.iter().map(|s| s.to_string()).collect(),
        CancellationToken::new(),
    )
    .await
    .expect("mở được cổng loopback");
    // `ctx` phải sống tiếp: thả nó là thả cả sổ đăng ký mà server đang cầm.
    (endpoint, ctx)
}

/// Gõ một `initialize` thô vào cổng và trả về dòng trạng thái.
async fn post(addr: SocketAddr, headers: &[(&str, &str)]) -> String {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAccept: application/json, text/event-stream\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("nối được vào cổng");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("gửi được request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("đọc được phản hồi");
    String::from_utf8_lossy(&response)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Token đúng đi qua được cổng thật.
#[tokio::test]
async fn cong_that_cho_token_dung_di_qua() {
    let (endpoint, _ctx) = open_gateway(&[]).await;
    let status = post(endpoint.addr(), &[("Authorization", &bearer())]).await;
    assert!(
        !status.contains(" 401") && !status.contains(" 403"),
        "token đúng không được bị chặn, nhận: {status}"
    );
    endpoint.shutdown().await;
}

/// Token sai bị cổng thật từ chối, trước khi chạm tới bất cứ thứ gì có trạng thái.
#[tokio::test]
async fn cong_that_tu_choi_token_sai() {
    let (endpoint, _ctx) = open_gateway(&[]).await;
    assert!(
        post(endpoint.addr(), &[("Authorization", "Bearer sai")])
            .await
            .contains(" 401")
    );
    // Không có header nào cũng vậy.
    assert!(post(endpoint.addr(), &[]).await.contains(" 401"));
    endpoint.shutdown().await;
}

/// `Origin` của một trang web bị từ chối dù token có đúng.
#[tokio::test]
async fn cong_that_tu_choi_origin_la() {
    let (endpoint, _ctx) = open_gateway(&[]).await;
    let status = post(
        endpoint.addr(),
        &[
            ("Authorization", &bearer()),
            ("Origin", "https://evil.example.com"),
        ],
    )
    .await;
    assert!(status.contains(" 403"), "nhận: {status}");
    endpoint.shutdown().await;
}
