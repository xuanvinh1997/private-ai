//! One HTTP GET, on a short leash.
//!
//! Three leashes, in fact, and each exists because of a specific way an unbounded fetch hurts:
//! a byte ceiling enforced *while streaming* (so a 2 GB file costs one buffer, not one machine's
//! memory), a redirect count (so a redirect loop ends), and a timeout well under the tool
//! pipeline's own. Redirects are followed by hand rather than by `reqwest` because that is the
//! only way [`Guard`] gets a say on each hop -- see the module docs of [`crate::guard`].

use std::time::Duration;

use reqwest::{Client, Response, StatusCode};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::guard::{Guard, GuardError};

/// Identify the client honestly. Sites block unnamed agents, and a name is also what lets an
/// administrator see in their logs that a person's assistant fetched a page, not a scraper farm.
const USER_AGENT: &str = concat!("PrivateAI/", env!("CARGO_PKG_VERSION"), " (native web.fetch)");

/// How much of a body is worth downloading. Five megabytes is far more than any article and far
/// less than a video; whatever survives the character ceiling afterwards is a fraction of it.
pub const DEFAULT_MAX_BYTES: usize = 5 * 1024 * 1024;
/// Enough for the `http -> https -> www -> canonical` chain every CMS does, and no more.
pub const DEFAULT_MAX_REDIRECTS: usize = 5;
/// Per-hop budget. Deliberately far below `ToolMeta`'s 120s default: a page that has not answered
/// in twenty seconds is not going to answer usefully, and the turn is waiting.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// How far one fetch may go before it is cut off.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_redirects: usize,
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_bytes: DEFAULT_MAX_BYTES,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// A body, and the facts about it that [`pai_web_core::render`] needs.
#[derive(Clone, Debug)]
pub struct Fetched {
    /// The address the body actually came from, after redirects; this is what relative links resolve against.
    pub url: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
    /// True when the byte ceiling stopped the download early.
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error(transparent)]
    Blocked(#[from] GuardError),
    #[error("URL không hợp lệ: {0}")]
    BadUrl(String),
    #[error("chuyển hướng quá {0} lần; có thể là vòng lặp")]
    TooManyRedirects(usize),
    #[error("chuyển hướng tới địa chỉ không đọc được: {0}")]
    BadRedirect(String),
    #[error("máy chủ trả về HTTP {0}")]
    Status(u16),
    #[error("lỗi mạng: {0}")]
    Transport(String),
    #[error("quá {0} giây mà máy chủ chưa trả lời")]
    Timeout(u64),
    #[error("đã huỷ trước khi tải xong")]
    Cancelled,
}

/// The HTTP client, its policy, and its limits, built once and shared.
pub struct Fetcher {
    client: Client,
    guard: Guard,
    limits: Limits,
}

impl Fetcher {
    pub fn new(guard: Guard, limits: Limits) -> anyhow::Result<Fetcher> {
        let client = Client::builder()
            // The whole point: `reqwest` must not follow a redirect behind the guard's back.
            .redirect(reqwest::redirect::Policy::none())
            .timeout(limits.timeout)
            .connect_timeout(limits.timeout)
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Fetcher {
            client,
            guard,
            limits,
        })
    }

    /// GET `raw`, following redirects by hand and checking each hop.
    pub async fn fetch(&self, raw: &str, cancel: &CancellationToken) -> Result<Fetched, FetchError> {
        let mut url = Url::parse(raw).map_err(|err| FetchError::BadUrl(err.to_string()))?;

        // `..=` so `max_redirects` counts hops after the first, which is what a user means by it.
        for _ in 0..=self.limits.max_redirects {
            // Raced against the token like every other wait here: the guard resolves DNS, which is
            // a blocking `getaddrinfo` on a pool thread and can sit for tens of seconds against a
            // sick resolver. A caller who gave up must not be made to wait for one.
            let checked = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(FetchError::Cancelled),
                result = self.guard.check(&url) => result,
            };
            checked?;
            let response = self.send(&url, cancel).await?;

            // The peer of the established connection is the only address that is a fact rather
            // than a prediction; checking it here closes the DNS-rebinding window before any body
            // is read. Dropping the response closes the connection without reading a byte.
            if let Some(peer) = response.remote_addr() {
                self.guard.check_addr(peer.ip())?;
            }

            match redirect_target(&response, &url)? {
                Some(next) => url = next,
                None => return self.body(url, response, cancel).await,
            }
        }
        Err(FetchError::TooManyRedirects(self.limits.max_redirects))
    }

    async fn send(&self, url: &Url, cancel: &CancellationToken) -> Result<Response, FetchError> {
        let request = self.client.get(url.clone()).send();
        // `biased` so a token cancelled before the poll wins instead of racing the request.
        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(FetchError::Cancelled),
            result = request => result.map_err(|err| self.classify(err))?,
        };

        let status = response.status();
        // Redirections are handled by the caller, so they are not failures here.
        if !status.is_success() && !status.is_redirection() {
            return Err(FetchError::Status(status.as_u16()));
        }
        Ok(response)
    }

    /// Read the body chunk by chunk, stopping at the ceiling.
    ///
    /// Streamed rather than `response.bytes()` on purpose: `bytes()` allocates whatever the server
    /// decides to send before anyone can object, and `Content-Length` is a claim, not a promise.
    async fn body(
        &self,
        url: Url,
        mut response: Response,
        cancel: &CancellationToken,
    ) -> Result<Fetched, FetchError> {
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        let mut bytes = Vec::new();
        let mut truncated = false;
        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(FetchError::Cancelled),
                chunk = response.chunk() => chunk.map_err(|err| self.classify(err))?,
            };
            let Some(chunk) = chunk else { break };
            let room = self.limits.max_bytes.saturating_sub(bytes.len());
            // Strictly greater, not `>=`: a body that ends exactly on the ceiling arrived whole,
            // and reporting it as cut would have the tool tell the model to expect more page.
            if chunk.len() > room {
                bytes.extend_from_slice(&chunk[..room]);
                truncated = true;
                // Stop pulling; dropping `response` here tears the connection down mid-body,
                // which is the point -- the remaining gigabytes are never transferred.
                break;
            }
            bytes.extend_from_slice(&chunk);
        }

        Ok(Fetched {
            url: url.to_string(),
            status,
            content_type,
            bytes,
            truncated,
        })
    }

    /// Separate a timeout from every other transport failure, since only one of the two is worth retrying.
    fn classify(&self, err: reqwest::Error) -> FetchError {
        if err.is_timeout() {
            return FetchError::Timeout(self.limits.timeout.as_secs());
        }
        FetchError::Transport(err.to_string())
    }
}

/// Where a response says to go next, if it says anything.
fn redirect_target(response: &Response, current: &Url) -> Result<Option<Url>, FetchError> {
    let status = response.status();
    if !matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    ) {
        return Ok(None);
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| FetchError::BadRedirect("thiếu header `Location`".to_string()))?;
    // Relative `Location` is legal and common; resolving it against the *current* hop rather than
    // the original URL is what makes a chain of relative redirects land where the server meant.
    current
        .join(location)
        .map(Some)
        .map_err(|err| FetchError::BadRedirect(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Reply, routes, serve};

    /// Loopback is exactly what [`Guard::lenient`] opens, and nothing else; every other refusal in
    /// these tests is the production filter doing its real job.
    fn fetcher(limits: Limits) -> Fetcher {
        Fetcher::new(Guard::lenient(), limits).expect("dựng được Fetcher")
    }

    #[tokio::test]
    async fn tai_duoc_trang_va_giu_content_type() {
        let addr = serve(routes([("/bai", Reply::html("<p>xin chào</p>"))])).await;
        let fetched = fetcher(Limits::default())
            .fetch(&format!("http://{addr}/bai"), &CancellationToken::new())
            .await
            .expect("tải được");
        assert_eq!(fetched.status, 200);
        assert_eq!(fetched.bytes, b"<p>xin ch\xc3\xa0o</p>");
        assert_eq!(
            fetched.content_type.as_deref(),
            Some("text/html; charset=utf-8")
        );
        assert!(!fetched.truncated);
    }

    #[tokio::test]
    async fn theo_chuyen_huong_tuong_doi_va_bao_dia_chi_cuoi() {
        let addr = serve(routes([
            ("/cu", Reply::redirect("/moi")),
            ("/moi", Reply::html("<p>đã chuyển</p>")),
        ]))
        .await;
        let fetched = fetcher(Limits::default())
            .fetch(&format!("http://{addr}/cu"), &CancellationToken::new())
            .await
            .expect("tải được");
        assert!(fetched.url.ends_with("/moi"), "{}", fetched.url);
        assert!(String::from_utf8_lossy(&fetched.bytes).contains("đã chuyển"));
    }

    /// The case every naive SSRF filter fails: the first hop is fine, the second is the cloud
    /// metadata endpoint. `Guard::lenient` opens loopback only, so this refusal is the real rule.
    #[tokio::test]
    async fn chan_chuyen_huong_ve_metadata_endpoint() {
        let addr = serve(routes([(
            "/thoat",
            Reply::redirect("http://169.254.169.254/latest/meta-data/"),
        )]))
        .await;
        let err = fetcher(Limits::default())
            .fetch(&format!("http://{addr}/thoat"), &CancellationToken::new())
            .await
            .expect_err("chuyển hướng ra mạng nội bộ phải bị chặn");
        assert!(matches!(err, FetchError::Blocked(_)), "{err}");
        assert!(err.to_string().contains("169.254"), "{err}");
    }

    #[tokio::test]
    async fn chan_chuyen_huong_sang_scheme_khac() {
        let addr = serve(routes([("/ra", Reply::redirect("file:///etc/passwd"))])).await;
        let err = fetcher(Limits::default())
            .fetch(&format!("http://{addr}/ra"), &CancellationToken::new())
            .await
            .expect_err("phải bị chặn");
        assert!(matches!(err, FetchError::Blocked(GuardError::Scheme(_))), "{err}");
    }

    #[tokio::test]
    async fn vong_chuyen_huong_co_diem_dung() {
        let addr = serve(routes([("/vong", Reply::redirect("/vong"))])).await;
        let limits = Limits {
            max_redirects: 3,
            ..Limits::default()
        };
        let err = fetcher(limits)
            .fetch(&format!("http://{addr}/vong"), &CancellationToken::new())
            .await
            .expect_err("vòng lặp phải dừng");
        assert!(matches!(err, FetchError::TooManyRedirects(3)), "{err}");
    }

    #[tokio::test]
    async fn cat_khi_qua_tran_dung_luong() {
        let big = "x".repeat(50_000);
        let addr = serve(routes([("/to", Reply::html(&big))])).await;
        let limits = Limits {
            max_bytes: 100,
            ..Limits::default()
        };
        let fetched = fetcher(limits)
            .fetch(&format!("http://{addr}/to"), &CancellationToken::new())
            .await
            .expect("tải được phần đầu");
        assert!(fetched.truncated, "phải báo là đã cắt");
        assert_eq!(fetched.bytes.len(), 100, "không được vượt trần");
    }

    /// The boundary the other test cannot see: a body exactly the size of the ceiling is complete,
    /// and a false `truncated` here becomes a false "chưa về hết" in front of the model.
    #[tokio::test]
    async fn vua_dung_tran_thi_khong_bao_la_da_cat() {
        let body = "x".repeat(100);
        let addr = serve(routes([("/vua", Reply::html(&body))])).await;
        let limits = Limits {
            max_bytes: 100,
            ..Limits::default()
        };
        let fetched = fetcher(limits)
            .fetch(&format!("http://{addr}/vua"), &CancellationToken::new())
            .await
            .expect("tải được");
        assert_eq!(fetched.bytes.len(), 100);
        assert!(!fetched.truncated, "vừa đủ trần thì không phải là bị cắt");
    }

    #[tokio::test]
    async fn huy_thi_dung_ngay() {
        let addr = serve(routes([("/bai", Reply::html("<p>x</p>"))])).await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = fetcher(Limits::default())
            .fetch(&format!("http://{addr}/bai"), &cancel)
            .await
            .expect_err("đã huỷ thì không được trả kết quả");
        assert!(matches!(err, FetchError::Cancelled), "{err}");
    }

    #[tokio::test]
    async fn loi_http_duoc_bao_bang_ma_so() {
        let addr = serve(routes([])).await;
        let err = fetcher(Limits::default())
            .fetch(&format!("http://{addr}/khong-co"), &CancellationToken::new())
            .await
            .expect_err("404 là lỗi");
        assert!(matches!(err, FetchError::Status(404)), "{err}");
    }

    #[tokio::test]
    async fn url_hong_bao_ngay_khong_can_mo_socket() {
        let err = fetcher(Limits::default())
            .fetch("khong phai url", &CancellationToken::new())
            .await
            .expect_err("URL hỏng phải bị từ chối");
        assert!(matches!(err, FetchError::BadUrl(_)), "{err}");
    }
}
