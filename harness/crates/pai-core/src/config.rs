//! The plugin tree, built from layered config.
//!
//! This whole architecture rests on the claim "everything is replaceable from config".
//! While the plugin list lives in code, that claim is only an intention. This file makes
//! it true.
//!
//! Three concepts, taken straight from dsh:
//!
//! - **Row** — one plugin plus its config, carrying a stable `id`.
//! - **Layer** — a set of operations on the row list: insert, replace, or disable.
//! - **Composition** — layers applied in turn onto an empty list, in the given order.
//!
//! Two decisions differ from dsh, both because this is a desktop application rather than a
//! developer CLI:
//!
//! **No expressions in config.** dsh allows `!!js` in the config file, which is arbitrary
//! code execution from a file. Acceptable for a command-line tool; not acceptable for an
//! application a user installs from a `.dmg`.
//!
//! **Disable, never delete.** An upper layer can only **disable** a lower layer's row.
//! With real deletion, a row missing from an upper layer quietly comes back the day
//! somebody reorders the layers, whereas `disabled` is always visible in `dump`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One plugin in the tree, along with its config.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// A stable identity. Upper layers target rows by `id`, so changing an `id` loses
    /// every patch pointing at it.
    pub id: String,
    /// The plugin name, matching what was registered in [`PluginCatalog`].
    pub plugin: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub disabled: bool,
}

/// One operation on the row list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Patch {
    /// Add a new row. Colliding with an existing `id` is a config error, not a silent
    /// overwrite — whoever wrote the layer almost certainly meant `replace`.
    Insert(Row),
    /// Replace a row's config **wholesale**.
    ///
    /// The whole block rather than a merge: with a merge there is no way to remove a field,
    /// and whoever writes the patch has to guess which of the lower layer's fields survive.
    Replace {
        id: String,
        config: Value,
    },
    Disable {
        id: String,
    },
    Enable {
        id: String,
    },
}

/// One config layer, along with where it came from.
#[derive(Clone, Debug, Deserialize)]
pub struct Layer {
    /// A file path, a bundle name, or `"--patch"`. Used only to talk to humans.
    #[serde(skip)]
    pub origin: String,
    #[serde(default)]
    pub patches: Vec<Patch>,
}

impl Layer {
    pub fn new(origin: impl Into<String>, patches: Vec<Patch>) -> Layer {
        Layer {
            origin: origin.into(),
            patches,
        }
    }

    /// A layer that is nothing but new rows. The usual shape of a base layer.
    pub fn base(origin: impl Into<String>, rows: Vec<Row>) -> Layer {
        Layer::new(origin, rows.into_iter().map(Patch::Insert).collect())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{origin}: row `{id}` already exists; use `replace` to change its config")]
    Duplicate { origin: String, id: String },
    #[error("{origin}: no row named `{id}` to {op}")]
    Unknown {
        origin: String,
        id: String,
        op: &'static str,
    },
}

/// The result of composition: the row list, plus a trail of who touched which row.
#[derive(Clone, Debug, Default)]
pub struct Composed {
    pub rows: Vec<Row>,
    /// `id` → where it was created, then everywhere that edited it, in order.
    pub provenance: HashMap<String, Vec<String>>,
}

impl Composed {
    /// The rows that will actually be mounted.
    pub fn active(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|row| !row.disabled)
    }

    /// The human-readable dump — the equivalent of dsh's `--dump-config`.
    ///
    /// Here for the same reason: in an architecture where everything is replaceable from
    /// config, the first question when something is wrong is always "what is actually
    /// running".
    ///
    /// The `[tắt]` marker is parsed by the settings screen (`settings/harness.ts`), so it
    /// is a wire contract, not display text. Changing it silently breaks that parser.
    pub fn dump(&self) -> String {
        self.rows
            .iter()
            .map(|row| {
                let trail = self
                    .provenance
                    .get(&row.id)
                    .map(|origins| origins.join(" → "))
                    .unwrap_or_default();
                let state = if row.disabled { " [tắt]" } else { "" };
                format!("{}: {}{}\n  # {}", row.id, row.plugin, state, trail)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Apply the layers onto an empty list, in the given order.
pub fn compose(layers: &[Layer]) -> Result<Composed, ConfigError> {
    let mut out = Composed::default();

    for layer in layers {
        for patch in &layer.patches {
            match patch {
                Patch::Insert(row) => {
                    if out.rows.iter().any(|existing| existing.id == row.id) {
                        return Err(ConfigError::Duplicate {
                            origin: layer.origin.clone(),
                            id: row.id.clone(),
                        });
                    }
                    out.provenance
                        .insert(row.id.clone(), vec![layer.origin.clone()]);
                    out.rows.push(row.clone());
                }
                Patch::Replace { id, config } => {
                    let row = find(&mut out.rows, id, &layer.origin, "replace")?;
                    row.config = config.clone();
                    note(&mut out.provenance, id, &layer.origin);
                }
                Patch::Disable { id } => {
                    find(&mut out.rows, id, &layer.origin, "disable")?.disabled = true;
                    note(&mut out.provenance, id, &layer.origin);
                }
                Patch::Enable { id } => {
                    find(&mut out.rows, id, &layer.origin, "enable")?.disabled = false;
                    note(&mut out.provenance, id, &layer.origin);
                }
            }
        }
    }
    Ok(out)
}

fn find<'a>(
    rows: &'a mut [Row],
    id: &str,
    origin: &str,
    op: &'static str,
) -> Result<&'a mut Row, ConfigError> {
    rows.iter_mut()
        .find(|row| row.id == id)
        .ok_or_else(|| ConfigError::Unknown {
            origin: origin.to_string(),
            id: id.to_string(),
            op,
        })
}

fn note(provenance: &mut HashMap<String, Vec<String>>, id: &str, origin: &str) {
    provenance
        .entry(id.to_string())
        .or_default()
        .push(origin.to_string());
}

/// Plugin name → how to build it from config.
///
/// This is the only place a string in a config file turns into code. The list is closed and
/// declared at startup: a config file cannot reach anything nobody registered here.
#[derive(Default)]
pub struct PluginCatalog {
    builders: HashMap<String, Builder>,
}

type Builder = Box<dyn Fn(&Value) -> anyhow::Result<Box<dyn crate::Plugin>> + Send + Sync>;

impl PluginCatalog {
    pub fn new() -> PluginCatalog {
        PluginCatalog::default()
    }

    pub fn register(
        &mut self,
        name: impl Into<String>,
        build: impl Fn(&Value) -> anyhow::Result<Box<dyn crate::Plugin>> + Send + Sync + 'static,
    ) {
        self.builders.insert(name.into(), Box::new(build));
    }

    pub fn build(&self, row: &Row) -> anyhow::Result<Box<dyn crate::Plugin>> {
        let builder = self.builders.get(&row.plugin).ok_or_else(|| {
            anyhow::anyhow!("row `{}`: unknown plugin `{}`", row.id, row.plugin)
        })?;
        builder(&row.config)
            .map_err(|err| anyhow::anyhow!("row `{}` ({}): {err}", row.id, row.plugin))
    }

    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.builders.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}
