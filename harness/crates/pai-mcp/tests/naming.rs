//! The prefix, and the names that may not exist.
//! Reads like a rulebook: each test locks one sentence, written on its doc line.

use pai_mcp::{ServerConfig, is_external, namespace, qualify, remote_of};
use pai_tools::ToolName;

/// The prefix goes on as `ext.<server>.`, and stripping returns exactly the published name.
#[test]
fn tien_to_dat_va_cat_dung_chieu() {
    let name = qualify("github", "search_issues");
    assert_eq!(name.as_str(), "ext.github.search_issues");
    assert_eq!(remote_of("github", &name), Some("search_issues"));
    assert_eq!(namespace("github"), "ext.github");
}

/// A dotted remote name survives intact: stripping never touches the tail.
#[test]
fn ten_tu_xa_co_dau_cham_khong_bi_dong_vao() {
    let name = qualify("srv", "rag.vector.search");
    assert_eq!(name.as_str(), "ext.srv.rag.vector.search");
    assert_eq!(remote_of("srv", &name), Some("rag.vector.search"));
}

/// Strip once, at the front only, so a tool named `ext.other.thing` cannot borrow server `other`'s identity.
#[test]
fn cat_tien_to_chi_cat_mot_lan() {
    let name = qualify("srv", "ext.other.thing");
    assert_eq!(name.as_str(), "ext.srv.ext.other.thing");
    assert_eq!(remote_of("srv", &name), Some("ext.other.thing"));
    // And it is not `other`'s tool.
    assert_eq!(remote_of("other", &name), None);
    assert_ne!(name, qualify("other", "thing"));
}

/// Asking the wrong server returns nothing; no name drifts to another server.
#[test]
fn khong_cat_duoc_tien_to_cua_server_khac() {
    let name = qualify("alpha", "ping");
    assert_eq!(remote_of("beta", &name), None);
    // A partial prefix match does not count: `al` is not `alpha`.
    assert_eq!(remote_of("al", &name), None);
}

/// An internal tool is never taken for an external one, or the reverse.
#[test]
fn phan_biet_duoc_tool_trong_voi_tool_ngoai() {
    assert!(is_external(&qualify("srv", "read")));
    assert!(!is_external(&ToolName::new("read")));
    // `extra` starts with `ext` but not `ext.`, which is where a careless comparison goes wrong.
    assert!(!is_external(&ToolName::new("extra.thing")));
}

/// The server name enters a tool's identity, so it is checked as an identity, not as a label.
#[test]
fn ten_server_bi_kiem() {
    assert!(ServerConfig::stdio("github", "npx").validate().is_ok());
    assert!(ServerConfig::stdio("my_server-2", "npx").validate().is_ok());

    // Empty: `ext..search` identifies no server.
    assert!(ServerConfig::stdio("", "npx").validate().is_err());
    // A dot: `a.b` + tool `c` and `a` + tool `b.c` produce the same full name.
    assert!(ServerConfig::stdio("a.b", "npx").validate().is_err());
    // `__` breaks the reversibility of the wire-name mapping.
    assert!(ServerConfig::stdio("a__b", "npx").validate().is_err());
    // Spaces, quotes and slashes all belong to names that cannot be checked.
    assert!(ServerConfig::stdio("a b", "npx").validate().is_err());
    assert!(ServerConfig::stdio("a/b", "npx").validate().is_err());
}

/// A name passing the config check also passes the registry's; the two live in different crates and never call each other.
#[test]
fn ten_qua_kiem_cau_hinh_thi_qua_duoc_so_dang_ky() {
    for server in ["github", "my_server-2", "a1"] {
        assert!(ServerConfig::stdio(server, "npx").validate().is_ok());
        assert!(qualify(server, "search").round_trips());
    }
}

/// The transport is checked too: an empty command or a non-http url is bad config, not a background dial failure.
#[test]
fn transport_bi_kiem() {
    assert!(ServerConfig::stdio("srv", "   ").validate().is_err());
    assert!(ServerConfig::http("srv", "ftp://x").validate().is_err());
    assert!(ServerConfig::http("srv", "ws://x").validate().is_err());
    assert!(
        ServerConfig::http("srv", "https://example.com/mcp")
            .validate()
            .is_ok()
    );
}

/// Config parses from JSON, and the defaults are what the user does not have to type.
#[test]
fn cau_hinh_doc_duoc_tu_json() {
    let stdio: ServerConfig = serde_json::from_str(
        r#"{"name":"github","transport":"stdio","command":"npx","args":["-y","x"]}"#,
    )
    .expect("đọc được cấu hình stdio");
    assert!(stdio.enabled);
    assert_eq!(stdio.connect_timeout().as_secs(), 20);
    assert!(stdio.validate().is_ok());

    let http: ServerConfig = serde_json::from_str(
        r#"{"name":"remote","transport":"http","url":"https://example.com/mcp",
            "headers":{"authorization":"Bearer x"},"max_retries":1}"#,
    )
    .expect("đọc được cấu hình http");
    assert_eq!(http.max_retries, 1);
    assert!(http.validate().is_ok());
}
