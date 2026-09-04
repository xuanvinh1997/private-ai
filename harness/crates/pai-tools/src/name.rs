//! Tool names: two projections of one identity.
//! The dotted form is canonical and every permission decision speaks it; the `__` form is
//! what the model sees. The type system keeps them apart, or the permission filter compares
//! two different things and filters nothing.

use std::fmt;

use serde::Serialize;

/// What replaces the dot on the wire.
pub const WIRE_SEPARATOR: &str = "__";

/// A tool's canonical identity — always the dotted form.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolName(String);

impl ToolName {
    /// Build from the canonical, dotted form.
    pub fn new(name: impl Into<String>) -> ToolName {
        ToolName(name.into())
    }

    /// The canonical form, spoken by the registry, restrictions and logs alike.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The form the model sees.
    pub fn wire(&self) -> String {
        self.0.replace('.', WIRE_SEPARATOR)
    }

    /// Decode a name from the model; a decode, not a lookup, so existence and permission are still the registry's questions.
    pub fn from_wire(wire: &str) -> ToolName {
        ToolName(wire.replace(WIRE_SEPARATOR, "."))
    }

    /// Reversible only when the canonical form has no `__`; a wire-name collision is a way around the permission filter.
    pub fn round_trips(&self) -> bool {
        !self.0.contains(WIRE_SEPARATOR)
    }
}

impl From<&str> for ToolName {
    fn from(value: &str) -> ToolName {
        ToolName::new(value)
    }
}

impl From<String> for ToolName {
    fn from(value: String) -> ToolName {
        ToolName(value)
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ToolName({})", self.0)
    }
}

/// Serialises canonically; the wire form is produced only in [`crate::schema::ToolSchema`].
impl Serialize for ToolName {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
