//! The SSRF gate.
//!
//! `web.fetch` is the first native code in this product that opens a socket to an address a model
//! chose, so this filter is load-bearing rather than tidy. Naive versions of it fail in three
//! places, and all three are handled here:
//!
//! 1. **Scheme.** `file:`, `gopher:` and `data:` are not the web; only `http` and `https` pass.
//! 2. **DNS.** `evil.example` resolving to `127.0.0.1` defeats any check on the hostname alone, so
//!    the name is resolved and every address it answers with is checked.
//! 3. **Redirects.** A public first hop that 302s to `169.254.169.254` is the classic cloud
//!    metadata theft. [`crate::fetch`] therefore drives redirects by hand and calls [`Guard::check`]
//!    on every hop instead of letting `reqwest` follow them.
//!
//! What is left is the window between resolving a name and connecting to it -- DNS rebinding.
//! Closing it properly needs a connector that pins the resolved address, which `reqwest` does not
//! expose per request. Instead the peer address of the established connection is checked with
//! [`Guard::check_addr`] before a single byte of the body is read, so a rebind can waste a
//! connection but cannot return data.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Hostname suffixes that name this machine or this LAN by convention rather than by address.
/// Blocking them by name matters because they may resolve through mDNS or a search domain that
/// the address check below never sees.
const LOCAL_SUFFIXES: &[&str] = &["localhost", "local", "internal", "home.arpa"];

#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error("URL không hợp lệ: {0}")]
    BadUrl(String),
    #[error("chỉ chấp nhận `http` và `https`; `{0}:` bị từ chối")]
    Scheme(String),
    #[error("URL không có tên máy chủ")]
    NoHost,
    #[error("không phân giải được tên `{host}`: {reason}")]
    Dns { host: String, reason: String },
    /// The message names the address, because a refused fetch the user cannot explain is a bug report.
    #[error("địa chỉ `{addr}` nằm trong mạng nội bộ ({reason}); tool này chỉ được ra Internet công cộng")]
    Private { addr: String, reason: &'static str },
}

/// Decides whether a URL may be dialled.
#[derive(Clone, Debug)]
pub struct Guard {
    /// Test-only hole; see [`Guard::lenient`]. Cannot be set outside a test build, because the
    /// only function that sets it is compiled away.
    allow_loopback: bool,
}

impl Default for Guard {
    fn default() -> Guard {
        Guard::strict()
    }
}

impl Guard {
    /// What production uses: nothing private, nothing local, nothing but `http(s)`.
    pub fn strict() -> Guard {
        Guard {
            allow_loopback: false,
        }
    }

    /// Loopback allowed, so a test can point [`crate::fetch::Fetcher`] at a `TcpListener` on
    /// 127.0.0.1 instead of at the real Internet.
    ///
    /// `#[cfg(test)]` rather than a feature flag or a public constructor: a feature can be turned
    /// on by a dependent crate and a public constructor can be called by mistake, whereas this
    /// function does not exist in any build the user runs. Note that it opens loopback *only* --
    /// the RFC1918 ranges and the cloud metadata address stay blocked even in tests, which is what
    /// lets the redirect test below assert on a real refusal.
    #[cfg(test)]
    pub(crate) fn lenient() -> Guard {
        Guard {
            allow_loopback: true,
        }
    }

    /// Check a URL: scheme, then hostname, then every address that name resolves to.
    pub async fn check(&self, url: &Url) -> Result<(), GuardError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(GuardError::Scheme(url.scheme().to_string()));
        }
        let Some(host) = url.host() else {
            return Err(GuardError::NoHost);
        };

        match host {
            // An address literal needs no lookup, and must not get one: `lookup_host` on
            // "127.0.0.1" happily succeeds and would only launder the same address.
            Host::Ipv4(addr) => return self.check_addr(IpAddr::V4(addr)),
            Host::Ipv6(addr) => return self.check_addr(IpAddr::V6(addr)),
            Host::Domain(name) => {
                let lowered = name.to_ascii_lowercase();
                if self.local_name(&lowered) {
                    return Err(GuardError::Private {
                        addr: lowered,
                        reason: "tên máy nội bộ",
                    });
                }
            }
        }

        // 80/443 as the fallback port keeps `lookup_host` happy; the port plays no part in the decision.
        let host = url.host_str().unwrap_or_default().to_string();
        let port = url.port_or_known_default().unwrap_or(80);
        let resolved = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|err| GuardError::Dns {
                host: host.clone(),
                reason: err.to_string(),
            })?;

        let mut seen = false;
        for addr in resolved {
            seen = true;
            // Every answer, not just the first: a split-horizon name that returns one public and
            // one private address is exactly the trick this is here to stop.
            self.check_addr(addr.ip())?;
        }
        if !seen {
            return Err(GuardError::Dns {
                host,
                reason: "không có địa chỉ nào".to_string(),
            });
        }
        Ok(())
    }

    /// Check one address. Called again on the peer of an established connection, which is the only
    /// address that is a fact rather than a prediction.
    pub fn check_addr(&self, addr: IpAddr) -> Result<(), GuardError> {
        let addr = match addr {
            // `::ffff:127.0.0.1` is 127.0.0.1 wearing a hat; judge the address underneath.
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => IpAddr::V4(v4),
                None => IpAddr::V6(v6),
            },
            other => other,
        };
        let Some(reason) = blocked_reason(addr) else {
            return Ok(());
        };
        if self.allow_loopback && addr.is_loopback() {
            return Ok(());
        }
        Err(GuardError::Private {
            addr: addr.to_string(),
            reason,
        })
    }

    fn local_name(&self, lowered: &str) -> bool {
        if self.allow_loopback {
            // The test hole covers the name too, or a test server addressed as "localhost" would
            // be refused by the name rule before the address rule ever got the chance to allow it.
            return false;
        }
        LOCAL_SUFFIXES
            .iter()
            .any(|suffix| lowered == *suffix || lowered.ends_with(&format!(".{suffix}")))
    }
}

/// Why an address is off limits, or `None` if it is ordinary public Internet.
///
/// Hand-rolled from the octets rather than built on `Ipv4Addr::is_global`, because that method and
/// several of its neighbours (`is_shared`, `is_documentation`, `is_benchmarking`) are still
/// unstable, and this crate does not build on nightly.
fn blocked_reason(addr: IpAddr) -> Option<&'static str> {
    match addr {
        IpAddr::V4(v4) => blocked_v4(v4),
        IpAddr::V6(v6) => blocked_v6(v6),
    }
}

fn blocked_v4(addr: Ipv4Addr) -> Option<&'static str> {
    let [a, b, ..] = addr.octets();
    if addr.is_unspecified() {
        // 0.0.0.0 means "this host" to the kernel, which is a shorter route to localhost.
        return Some("0.0.0.0");
    }
    if addr.is_loopback() {
        return Some("127.0.0.0/8, loopback");
    }
    if addr.is_private() {
        return Some("RFC1918, mạng riêng");
    }
    if addr.is_link_local() {
        // 169.254.169.254 is the metadata endpoint on AWS, GCP and Azure alike: one GET away from
        // the machine's cloud credentials, which is why the whole /16 goes.
        return Some("169.254.0.0/16, link-local và metadata endpoint của cloud");
    }
    if a == 100 && (64..128).contains(&b) {
        return Some("100.64.0.0/10, CGNAT");
    }
    if a == 192 && b == 0 {
        return Some("192.0.0.0/24, IETF protocol assignments");
    }
    if addr.is_broadcast() || addr.is_multicast() {
        return Some("broadcast/multicast");
    }
    if a >= 240 {
        return Some("240.0.0.0/4, dành riêng");
    }
    None
}

fn blocked_v6(addr: Ipv6Addr) -> Option<&'static str> {
    // An IPv6 address may be an IPv4 address in disguise, and on a host with a NAT64 or 6to4 route
    // the disguise works: `64:ff9b::7f00:1` is a spelling of 127.0.0.1 that no rule below matches.
    // Judged by the address inside, so a wrapper around a public address still passes.
    if let Some(v4) = embedded_v4(addr)
        && let Some(reason) = blocked_v4(v4)
    {
        return Some(reason);
    }
    if addr.is_unspecified() {
        return Some("::");
    }
    if addr.is_loopback() {
        return Some("::1, loopback");
    }
    if addr.is_multicast() {
        return Some("multicast");
    }
    let head = addr.segments()[0];
    if head & 0xfe00 == 0xfc00 {
        return Some("fc00::/7, unique local");
    }
    if head & 0xffc0 == 0xfe80 {
        return Some("fe80::/10, link-local");
    }
    if head == 0x0100 {
        return Some("100::/64, discard-only");
    }
    None
}

/// The IPv4 address an IPv6 address carries, for the two prefixes that carry one.
///
/// `64:ff9b::/96` is the well-known NAT64 prefix (RFC 6052) and `2002::/16` is 6to4 (RFC 3056).
/// The IPv4-mapped form `::ffff:0:0/96` is not here because [`Guard::check_addr`] unwraps it before
/// any of this runs. Everything else is left alone: guessing at an embedded address where no
/// prefix says there is one would block legitimate hosts on a coincidence of digits.
fn embedded_v4(addr: Ipv6Addr) -> Option<Ipv4Addr> {
    let seg = addr.segments();
    if seg[0] == 0x0064 && seg[1] == 0xff9b && seg[2..6] == [0, 0, 0, 0] {
        let octets = addr.octets();
        return Some(Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]));
    }
    if seg[0] == 0x2002 {
        let [a, b] = seg[1].to_be_bytes();
        let [c, d] = seg[2].to_be_bytes();
        return Some(Ipv4Addr::new(a, b, c, d));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL mẫu trong test phải parse được")
    }

    #[tokio::test]
    async fn chan_scheme_khong_phai_http() {
        for raw in ["file:///etc/passwd", "gopher://vidu.test/1", "ftp://vidu.test/x"] {
            let err = Guard::strict()
                .check(&url(raw))
                .await
                .expect_err("phải bị chặn");
            assert!(matches!(err, GuardError::Scheme(_)), "{raw}: {err}");
        }
    }

    #[tokio::test]
    async fn chan_dia_chi_noi_bo_viet_bang_so() {
        let cases = [
            "http://127.0.0.1/",
            // Same host, written the short way a substring filter never catches.
            "http://127.1/",
            "http://10.1.2.3/",
            "http://172.16.0.1/",
            "http://172.31.255.254/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://0.0.0.0/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[fc00::1]/",
            "http://[fe80::1]/",
            // IPv4-mapped IPv6: the same loopback in another notation.
            "http://[::ffff:127.0.0.1]/",
        ];
        for raw in cases {
            let err = Guard::strict()
                .check(&url(raw))
                .await
                .expect_err("phải bị chặn");
            assert!(matches!(err, GuardError::Private { .. }), "{raw}: {err}");
        }
    }

    /// The same private addresses written as IPv6, which is how they slip past a filter that only
    /// knows the dotted form.
    #[tokio::test]
    async fn chan_dia_chi_noi_bo_giau_trong_ipv6() {
        for raw in [
            // NAT64: 127.0.0.1
            "http://[64:ff9b::7f00:1]/",
            // NAT64: 169.254.169.254, the cloud metadata endpoint
            "http://[64:ff9b::a9fe:a9fe]/",
            // 6to4: 192.168.1.1
            "http://[2002:c0a8:101::]/",
        ] {
            let err = Guard::strict()
                .check(&url(raw))
                .await
                .expect_err("phải bị chặn");
            assert!(matches!(err, GuardError::Private { .. }), "{raw}: {err}");
        }
        // Precision matters as much as coverage: 6to4 around a public address is still public.
        Guard::strict()
            .check(&url("http://[2002:808:808::]/"))
            .await
            .expect("6to4 bọc 8.8.8.8 vẫn là địa chỉ công cộng");
    }

    #[tokio::test]
    async fn chan_ten_may_noi_bo() {
        for raw in [
            "http://localhost:8080/",
            "http://LOCALHOST/",
            "http://printer.local/",
            "http://vault.internal/",
        ] {
            let err = Guard::strict()
                .check(&url(raw))
                .await
                .expect_err("phải bị chặn");
            assert!(matches!(err, GuardError::Private { .. }), "{raw}: {err}");
        }
    }

    #[tokio::test]
    async fn cho_qua_dia_chi_cong_cong() {
        // An address literal, so this test needs no DNS and therefore no network.
        Guard::strict()
            .check(&url("https://8.8.8.8/"))
            .await
            .expect("địa chỉ công cộng phải đi được");
        Guard::strict()
            .check(&url("https://[2001:4860:4860::8888]/"))
            .await
            .expect("IPv6 công cộng phải đi được");
    }

    #[tokio::test]
    async fn userinfo_khong_lam_lac_huong_bo_loc() {
        // `http://vidu.test@127.0.0.1/` reads as a public host to anyone matching on strings.
        let err = Guard::strict()
            .check(&url("http://vidu.test@127.0.0.1/"))
            .await
            .expect_err("phải bị chặn");
        assert!(matches!(err, GuardError::Private { .. }), "{err}");
    }

    #[tokio::test]
    async fn lo_hong_test_chi_mo_dung_loopback() {
        let guard = Guard::lenient();
        guard
            .check(&url("http://127.0.0.1:9/"))
            .await
            .expect("loopback mở trong test");
        // Everything else stays shut, which is what makes the redirect test meaningful.
        for raw in ["http://169.254.169.254/", "http://10.0.0.1/"] {
            assert!(guard.check(&url(raw)).await.is_err(), "{raw}");
        }
        assert!(guard.check(&url("file:///etc/passwd")).await.is_err());
    }

    #[test]
    fn kiem_tra_peer_sau_khi_ket_noi() {
        let guard = Guard::strict();
        assert!(guard.check_addr("93.184.216.34".parse().expect("ip")).is_ok());
        assert!(guard.check_addr("192.168.0.5".parse().expect("ip")).is_err());
    }
}
