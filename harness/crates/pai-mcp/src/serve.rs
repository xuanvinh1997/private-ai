//! Serve [`RegistryServer`] over stdio and streamable HTTP.
//! stdio needs no guard; HTTP is an open port on a machine running a browser, so it gets
//! four layers: loopback bind, loopback `Host`, allow-listed `Origin`, and a bearer token.

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

/// Run the server on stdin/stdout; returns when the channel closes or `ct` is cancelled.
pub async fn serve_stdio(server: RegistryServer, ct: CancellationToken) -> anyhow::Result<()> {
    let service = server.serve_with_ct(rmcp::transport::stdio(), ct).await?;
    service.waiting().await?;
    Ok(())
}

/// Why a request was blocked; three variants for the log, one undifferentiated reply for the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    /// `Host` is not loopback — the mark of DNS rebinding.
    Host,
    /// `Origin` is not on the allow-list.
    Origin,
    /// Missing, malformed, or wrong token.
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

/// The HTTP gate's three checks, separated from sockets so a test can ask them with a plain `http::Request`.
pub struct HttpGuard {
    token: McpToken,
    allowed_origins: Vec<String>,
}

impl HttpGuard {
    /// An empty `Origin` list rejects every request carrying one: empty config is the strictest config.
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
        // HTTP/1.1 requires `Host`, HTTP/2 puts `:authority` in the URI; with neither, unable to check means refuse.
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
            // No `Origin` means no browser sent it, the normal MCP client case; the bearer check below still applies.
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

/// `127.0.0.1`, `[::1]`, `localhost`, with or without a port.
fn is_loopback_authority(authority: &str) -> bool {
    let host = match authority.strip_prefix('[') {
        // `[::1]` or `[::1]:8080`; a missing `]` is a malformed authority, and malformed is not loopback.
        Some(rest) => match rest.split_once(']') {
            Some((host, _)) => host,
            None => return false,
        },
        None => authority.split(':').next().unwrap_or(authority),
    };
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// The running HTTP endpoint.
pub struct HttpEndpoint {
    addr: SocketAddr,
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

impl HttpEndpoint {
    /// The address actually being listened on, which differs from the request when port `0` was asked for.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn shutdown(self) {
        self.cancel.cancel();
        let _ = self.handle.await;
    }
}

/// Open the HTTP port; `bind` must be loopback.
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
    // `rmcp` checks `Host` and `Origin` too; the overlap with [`HttpGuard`] is deliberate, and the stricter one wins.
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
                // One failed accept, say out of file descriptors, must not bring the port down.
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
                            tracing::debug!(%err, "MCP HTTP connection ended");
                        }
                    }
                }
            });
        }
    });

    tracing::info!(%addr, "MCP HTTP listening");
    Ok(HttpEndpoint {
        addr,
        cancel,
        handle,
    })
}

/// [`StreamableHttpService`] behind the three checks, so a rejected request never reaches any stateful layer.
#[derive(Clone)]
struct Guarded {
    inner: StreamableHttpService<RegistryServer, LocalSessionManager>,
    guard: Arc<HttpGuard>,
}

fn refuse(denied: Denied) -> Response<Body> {
    // One sentence for all three reasons: distinct messages would turn this port into a configuration oracle.
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
                tracing::warn!(?denied, "refused an MCP HTTP request");
                return Ok(refuse(denied));
            }
            Ok(inner.handle(Request::from_parts(parts, body)).await)
        })
    }
}
