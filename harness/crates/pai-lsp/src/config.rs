//! Language table: adding a server means adding one row.
//! The protocol is standard, so [`crate::client`] knows no server names; the only
//! differences are the launch command line and a few initialization options.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How long to wait for the handshake before answering "not ready"; workspace indexing happens after `initialize` and is reported via `$/progress`.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// Deadline for a sent query. Sixty seconds, taken from dsh's configuration.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for `publishDiagnostics` after opening a file; it is a push notification, so only our patience can time out.
pub const DIAGNOSTICS_WAIT: Duration = Duration::from_secs(5);

/// Cap on locations returned per query; `references` on `String::new` yields thousands, and the model needs the first few plus the knowledge that more exist.
pub const MAX_LOCATIONS: usize = 100;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub startup: Duration,
    pub request: Duration,
    pub diagnostics: Duration,
    pub max_locations: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            startup: STARTUP_TIMEOUT,
            request: REQUEST_TIMEOUT,
            diagnostics: DIAGNOSTICS_WAIT,
            max_locations: MAX_LOCATIONS,
        }
    }
}

/// One language server, exactly as the user declared it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct LanguageConfig {
    /// Row name. It also appears in error messages, so make it readable.
    pub id: String,
    /// Extensions this server accepts; on a duplicate the first row wins, in the order the user wrote them.
    pub extensions: Vec<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// `initializationOptions` sent with the handshake; raw JSON, since this crate has no business understanding it.
    #[serde(default)]
    pub initialization_options: Option<Value>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// The default table. Three rows, all of them servers people really do install.
pub fn defaults() -> Vec<LanguageConfig> {
    vec![
        LanguageConfig {
            id: "rust".into(),
            extensions: vec!["rs".into()],
            command: "rust-analyzer".into(),
            args: Vec::new(),
            initialization_options: None,
            enabled: true,
        },
        LanguageConfig {
            id: "typescript".into(),
            extensions: vec![
                "ts".into(),
                "tsx".into(),
                "mts".into(),
                "cts".into(),
                "js".into(),
                "jsx".into(),
                "mjs".into(),
                "cjs".into(),
            ],
            command: "typescript-language-server".into(),
            args: vec!["--stdio".into()],
            initialization_options: None,
            enabled: true,
        },
        LanguageConfig {
            id: "python".into(),
            extensions: vec!["py".into(), "pyi".into()],
            command: "pyright-langserver".into(),
            args: vec!["--stdio".into()],
            initialization_options: None,
            enabled: true,
        },
    ]
}

/// The spec `languageId` for a file, by extension; kept out of [`LanguageConfig`] because the two are not one-to-one - one server serves `.ts` and `.js`, but `didOpen` must still say `"javascript"`.
pub fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => "rust",
        Some("ts") | Some("mts") | Some("cts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js") | Some("mjs") | Some("cjs") => "javascript",
        Some("jsx") => "javascriptreact",
        Some("py") | Some("pyi") => "python",
        Some("go") => "go",
        Some("c") | Some("h") => "c",
        Some("cc") | Some("cpp") | Some("hpp") => "cpp",
        // Unknown means plain text; the server will skip the file, which beats it being adopted into a project it does not belong to.
        _ => "plaintext",
    }
}
