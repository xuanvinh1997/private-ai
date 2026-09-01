//! Đưa [`RegistryServer`] ra hai cửa: stdio và streamable HTTP.
//!
//! stdio không cần canh gì: ai chạy được tiến trình này thì đã ở trong máy rồi, và kênh
//! chỉ nối đúng hai đầu.
//!
//! HTTP thì khác hẳn — nó là một cổng mở trên một máy có trình duyệt đang chạy. Bốn lớp,
//! và mỗi lớp chặn một thứ mà ba lớp kia không chặn:
//!
//! 1. **Chỉ bind loopback.** Không có lớp này thì `0.0.0.0` biến sổ đăng ký tool thành một
//!    dịch vụ của cả mạng LAN.
//! 2. **`Host` phải là loopback.** Chống DNS rebinding: một trang web trỏ tên miền của nó
//!    về `127.0.0.1` rồi cho JavaScript gọi vào đây — kết nối *là* loopback, nhưng `Host`
//!    mang tên miền của kẻ tấn công. Lớp 1 không thấy được chuyện đó.
//! 3. **`Origin` phải nằm trong danh sách trắng.** Mặc định danh sách rỗng, nghĩa là **mọi
//!    request có `Origin` đều bị từ chối** — không client MCP thật nào gửi `Origin`, chỉ
//!    trình duyệt gửi. Đây là chỗ cố tình chặt hơn mặc định của `rmcp`, vốn cho request
//!    thiếu `Origin` đi qua và bỏ qua kiểm tra khi danh sách rỗng.
//! 4. **Bearer token.** Ba lớp trên nói *ai gọi được*; lớp này nói *ai được phép*. Một
//!    tiến trình khác của cùng người dùng vẫn nói chuyện được với loopback, nên không có
//!    lớp này thì bất kỳ chương trình nào trên máy cũng gọi được `bash`.

use std::convert::Infallible;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http::{Request, Response, StatusCode, header};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use rmcp::service::ServiceExt;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::expose::RegistryServer;
use crate::token::McpToken;

type Body = BoxBody<Bytes, Infallible>;

/// Chạy server trên stdin/stdout. Trả về khi kênh đóng hoặc `ct` bị huỷ.
pub async fn serve_stdio(server: RegistryServer, ct: CancellationToken) -> anyhow::Result<()> {
    let service = server.serve_with_ct(rmcp::transport::stdio(), ct).await?;
    service.waiting().await?;
    Ok(())
}

/// Vì sao một request bị chặn.
///
/// Ba biến thể để log nói được chuyện gì đã xảy ra. Cái đi ra tới khách thì **không** phân
/// biệt chi tiết đến thế: một thông báo nói rõ sai ở đâu là một máy dò cấu hình.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    /// `Host` không phải loopback — dấu hiệu của DNS rebinding.
    Host,
    /// `Origin` không nằm trong danh sách trắng.
    Origin,
    /// Thiếu, sai định dạng, hoặc sai token.
    Auth,
}

impl Denied {
    fn status(self) -> StatusCode {
        match self {
            Denied::Auth => StatusCode::UNAUTHORIZED,
            Denied::Host | Denied::Origin => StatusCode::FORBIDDEN,
        }
    }
}

/// Ba lớp kiểm của cổng HTTP, tách khỏi mọi thứ dính tới socket.
///
/// Tách ra để kiểm chứng được: một bài test dựng một `http::Request` rồi hỏi thẳng, không
/// mở cổng nào và không chạm tới mạng. Một luật bảo mật chỉ kiểm được qua một cổng thật là
/// một luật sẽ không có ai kiểm.
pub struct HttpGuard {
    token: McpToken,
    allowed_origins: Vec<String>,
}

impl HttpGuard {
    /// Danh sách `Origin` rỗng = từ chối mọi request có mang `Origin`. Xem ghi chú đầu
    /// module: cấu hình trống là cấu hình chặt nhất, giống hệt `FileRoots` không có gốc
    /// nào thì không đọc được gì.
    pub fn new(token: McpToken, allowed_origins: Vec<String>) -> HttpGuard {
        HttpGuard {
            token,
            allowed_origins,
        }
    }

    pub fn check(&self, parts: &http::request::Parts) -> Result<(), Denied> {
        self.check_host(parts)?;
        self.check_origin(parts)?;
        self.check_auth(parts)
    }

    fn check_host(&self, parts: &http::request::Parts) -> Result<(), Denied> {
        // HTTP/1.1 bắt buộc có `Host`; HTTP/2 mang nó trong `:authority`, thứ `http` đặt
        // vào URI. Thiếu cả hai thì không kiểm được, và không kiểm được là từ chối.
        let authority = parts
            .headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .or_else(|| parts.uri.authority().map(|a| a.as_str().to_string()))
            .ok_or(Denied::Host)?;
        if is_loopback_authority(&authority) {
            Ok(())
        } else {
            Err(Denied::Host)
        }
    }

    fn check_origin(&self, parts: &http::request::Parts) -> Result<(), Denied> {
        let Some(origin) = parts.headers.get(header::ORIGIN) else {
            // Không có `Origin` nghĩa là không phải trình duyệt gửi. Đó là trường hợp
            // thường của một client MCP, và lớp bearer token bên dưới vẫn phải qua.
            return Ok(());
        };
        let origin = origin.to_str().map_err(|_| Denied::Origin)?;
        if self
            .allowed_origins
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(origin))
        {
            Ok(())
        } else {
            Err(Denied::Origin)
        }
    }

    fn check_auth(&self, parts: &http::request::Parts) -> Result<(), Denied> {
        let presented = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(Denied::Auth)?;
        if self.token.matches(presented.trim()) {
            Ok(())
        } else {
            Err(Denied::Auth)
        }
    }
}

/// `127.0.0.1`, `[::1]`, `localhost`, có hoặc không có cổng.
fn is_loopback_authority(authority: &str) -> bool {
    let host = match authority.strip_prefix('[') {
        // Dạng `[::1]` hoặc `[::1]:8080`. Không có `]` là một authority hỏng, và một
        // authority hỏng không được coi là loopback.
        Some(rest) => match rest.split_once(']') {
            Some((host, _)) => host,
            None => return false,
        },
        None => authority.split(':').next().unwrap_or(authority),
    };
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Cổng HTTP đang chạy.
pub struct HttpEndpoint {
    addr: SocketAddr,
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

impl HttpEndpoint {
    /// Địa chỉ thật sự đang nghe — khác cái yêu cầu khi cổng được xin là `0`.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn shutdown(self) {
        self.cancel.cancel();
        let _ = self.handle.await;
    }
}

/// Mở cổng HTTP. `bind` **phải** là loopback.
pub async fn serve_http(
    server: RegistryServer,
    bind: SocketAddr,
    token: McpToken,
    allowed_origins: Vec<String>,
    ct: CancellationToken,
) -> anyhow::Result<HttpEndpoint> {
    if !bind.ip().is_loopback() {
        anyhow::bail!(
            "MCP HTTP chỉ được bind loopback, không phải {}: sổ đăng ký tool không phải một dịch vụ mạng",
            bind.ip()
        );
    }

    let mut config = StreamableHttpServerConfig::default();
    config.cancellation_token = ct.child_token();
    // `rmcp` tự kiểm `Host` và `Origin` nữa. Trùng việc với [`HttpGuard`] là cố ý: hai lần
    // kiểm độc lập, và cái nào chặt hơn thì cái đó thắng.
    config.allowed_origins = allowed_origins.clone();

    let inner = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let guard = Arc::new(HttpGuard::new(token, allowed_origins));

    let listener = TcpListener::bind(bind).await?;
    let addr = listener.local_addr()?;
    let cancel = ct.clone();

    let handle = tokio::spawn(async move {
        loop {
            let accepted = tokio::select! {
                () = ct.cancelled() => break,
                accepted = listener.accept() => accepted,
            };
            let Ok((stream, _peer)) = accepted else {
                // Một lần accept hỏng — hết file descriptor chẳng hạn — không được làm sập
                // cổng: lần sau có thể lại được.
                continue;
            };
            let service = Guarded {
                inner: inner.clone(),
                guard: guard.clone(),
            };
            let conn_ct = ct.clone();
            tokio::spawn(async move {
                let conn = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service);
                tokio::select! {
                    () = conn_ct.cancelled() => {}
                    result = conn => {
                        if let Err(err) = result {
                            tracing::debug!(%err, "kết nối MCP HTTP kết thúc");
                        }
                    }
                }
            });
        }
    });

    tracing::info!(%addr, "MCP HTTP đang nghe");
    Ok(HttpEndpoint {
        addr,
        cancel,
        handle,
    })
}

/// [`StreamableHttpService`] với ba lớp kiểm đặt trước.
///
/// Đứng trước chứ không đứng sau, và đó là toàn bộ điểm của nó: một request không qua được
/// [`HttpGuard`] thì không bao giờ chạm tới tầng phiên, tầng phân giải tool, hay bất cứ
/// thứ gì có trạng thái.
#[derive(Clone)]
struct Guarded {
    inner: StreamableHttpService<RegistryServer, LocalSessionManager>,
    guard: Arc<HttpGuard>,
}

fn refuse(denied: Denied) -> Response<Body> {
    // Một câu duy nhất cho cả ba lý do. Nói rõ "sai token" và "sai Origin" bằng hai câu
    // khác nhau là biến cổng này thành một máy dò cấu hình cho người gõ thử.
    Response::builder()
        .status(denied.status())
        .body(Full::new(Bytes::from_static(b"forbidden")).boxed())
        .expect("phản hồi tĩnh luôn dựng được")
}

impl hyper::service::Service<Request<hyper::body::Incoming>> for Guarded {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, request: Request<hyper::body::Incoming>) -> Self::Future {
        let inner = self.inner.clone();
        let guard = self.guard.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            if let Err(denied) = guard.check(&parts) {
                tracing::warn!(?denied, "từ chối một request MCP HTTP");
                return Ok(refuse(denied));
            }
            Ok(inner.handle(Request::from_parts(parts, body)).await)
        })
    }
}
