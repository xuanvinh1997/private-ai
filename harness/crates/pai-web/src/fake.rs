//! Test doubles: a hand-rolled HTTP server and a search provider that answers from a list.
//!
//! Compiled only under `cfg(test)`, so nothing here can be reached by a shipped build. The server
//! is written by hand rather than pulled from a crate because the whole point is to control the
//! exact bytes on the wire -- a redirect with a relative `Location`, a body larger than the
//! ceiling, a 404 -- and any convenience layer would normalise away the cases worth testing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::search::{SearchError, SearchHit, SearchProvider};

/// One canned response.
#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn html(body: &str) -> Reply {
        Reply {
            status: 200,
            headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn redirect(location: &str) -> Reply {
        Reply {
            status: 302,
            headers: vec![("Location".into(), location.to_string())],
            body: Vec::new(),
        }
    }

    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

/// Start a server on an ephemeral loopback port and return its address.
///
/// The accept loop is detached: the test's own runtime shuts down when the test ends, which takes
/// the loop with it, so there is nothing to join and nothing to leak between tests.
pub async fn serve(routes: HashMap<String, Reply>) -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind loopback trong test");
    let addr = listener.local_addr().expect("địa chỉ của listener");
    let routes = Arc::new(routes);

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            tokio::spawn(async move {
                // The request line is all this server understands, and it is all the tests send.
                let mut buffer = [0u8; 2048];
                let Ok(read) = socket.read(&mut buffer).await else {
                    return;
                };
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();

                let reply = routes
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| Reply::status(404));

                let mut head = format!("HTTP/1.1 {} X\r\n", reply.status);
                for (name, value) in &reply.headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
                // No keep-alive: one request per connection keeps this server three lines long.
                head.push_str("Connection: close\r\n\r\n");

                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&reply.body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    addr
}

pub fn routes(entries: impl IntoIterator<Item = (&'static str, Reply)>) -> HashMap<String, Reply> {
    entries
        .into_iter()
        .map(|(path, reply)| (path.to_string(), reply))
        .collect()
}

/// A [`SearchProvider`] that answers from memory, so tool tests need neither key nor network.
pub struct FakeSearch {
    hits: Vec<SearchHit>,
    error: Option<fn() -> SearchError>,
}

impl FakeSearch {
    pub fn hits(hits: Vec<SearchHit>) -> FakeSearch {
        FakeSearch { hits, error: None }
    }

    pub fn failing(error: fn() -> SearchError) -> FakeSearch {
        FakeSearch {
            hits: Vec::new(),
            error: Some(error),
        }
    }
}

#[async_trait]
impl SearchProvider for FakeSearch {
    fn name(&self) -> &'static str {
        "Nhà cung cấp giả"
    }

    async fn search(
        &self,
        _query: &str,
        limit: usize,
        _cancel: &CancellationToken,
    ) -> Result<Vec<SearchHit>, SearchError> {
        if let Some(error) = self.error {
            return Err(error());
        }
        Ok(self.hits.iter().take(limit).cloned().collect())
    }
}
