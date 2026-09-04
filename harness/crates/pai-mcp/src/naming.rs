//! The `ext.<server>.` prefix, and both directions of it.
//! A remote name is never seen bare: prefixed where the list is read, stripped only where a
//! call is forwarded, so no third-party tool can collide with or shadow an internal one.

use pai_tools::ToolName;

/// The namespace for everything external; no internal tool may start with it.
pub const EXTERNAL_PREFIX: &str = "ext";

/// `ext.<server>` — the common head of every tool from one server.
pub fn namespace(server: &str) -> String {
    format!("{EXTERNAL_PREFIX}.{server}")
}

/// Add the prefix; called in exactly one place, when the tool list is read.
pub fn qualify(server: &str, remote: &str) -> ToolName {
    ToolName::new(format!("{EXTERNAL_PREFIX}.{server}.{remote}"))
}

/// Strip the prefix, called only when forwarding a call; `None` is a programming error the caller may reject.
pub fn remote_of<'a>(server: &str, name: &'a ToolName) -> Option<&'a str> {
    name.as_str()
        .strip_prefix(&format!("{}.", namespace(server)))
}

/// Whether the name came from outside; our own server never re-exposes third-party tools.
pub fn is_external(name: &ToolName) -> bool {
    name.as_str().starts_with(&format!("{EXTERNAL_PREFIX}."))
}
