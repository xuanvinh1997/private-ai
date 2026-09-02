//! The plugin core: everything else in the harness is a plugin that mounts here.
//!
//! Four ideas, borrowed from Cordis but rewritten for Rust's type system:
//!
//! - **Seams** — a capability addressed by a marker type, not by an implementation.
//!   Swapping a provider does not touch its consumers. See [`service::ServiceKey`].
//! - **Dependencies are needs, not ordering** — a plugin calls `wait_for` on the services
//!   it needs, so startup order sorts itself out. See [`context::Context::wait_for`].
//! - **Typed events** — observation, first-responder, and surrounding middleware.
//!   See [`event`].
//! - **Registration is an undoable effect** — an RAII guard by default, an explicit scope
//!   when cleanup has to `await`. See [`effect`].

pub mod config;
pub mod context;
pub mod effect;
pub mod event;
pub mod plugin;
pub mod scope;
pub mod service;

pub use config::{Composed, ConfigError, Layer, Patch, PluginCatalog, Row, compose};
pub use context::{Context, ProvideError};
pub use effect::{EffectScope, Guard};
pub use event::{First, Middleware, Next, Notify, Waterfall};
pub use plugin::Plugin;
pub use scope::ScopeKey;
pub use service::{Realm, ServiceKey};
