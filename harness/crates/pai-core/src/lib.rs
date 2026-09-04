//! Plugin core: everything else in the harness is a plugin mounted here.
//! Seams address a capability by marker type, and dependencies are `wait_for` needs
//! rather than a startup order. Events are typed; registration is an undoable effect.

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
