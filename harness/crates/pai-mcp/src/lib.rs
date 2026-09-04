//! MCP, both directions of one trust boundary: [`hub`] is a client of third-party servers,
//! [`expose`] is a server over our own registry. External tools are always prefixed
//! `ext.<server>.`, always assumed hostile, and their failures never reach the user's tools.

pub mod catalog;
pub mod config;
pub mod dial;
pub mod expose;
pub mod hub;
pub mod naming;
pub mod plugin;
pub mod remote;
pub mod seam;
pub mod serve;
pub mod store;
pub mod token;

pub use catalog::{CATALOG, CatalogEntry, EnvVar, instantiate};
pub use config::{ConfigError, McpTransport, ServerConfig};
pub use dial::{ConfigDialers, Dialer, DialerFactory, Reach};
pub use expose::RegistryServer;
pub use hub::{McpHub, Mount, RetryPolicy, ServerState, ServerStatus};
pub use naming::{EXTERNAL_PREFIX, is_external, namespace, qualify, remote_of};
pub use plugin::{ExposeOptions, McpPlugin};
pub use remote::{Link, RemoteTool};
pub use seam::{Mcp, McpConfig};
pub use serve::{Denied, HttpEndpoint, HttpGuard, serve_http, serve_stdio};
pub use store::{McpStore, StoreError, apply, merge};
pub use token::{McpToken, TOKEN_FILE, constant_time_eq, token_path};
