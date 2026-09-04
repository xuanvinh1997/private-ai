//! The scoped tool registry, holding the crate's central invariant: two-stage filtering.
//! The advertised list is only a hint, so permission is rechecked at call time after name
//! decoding, and both stages call the same [`ToolRegistry::permits`] so they cannot drift.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pai_core::scope::ScopeTree;
use pai_core::{Context, Guard, ScopeKey};
use parking_lot::RwLock;
use serde_json::{Map, Value};

use crate::name::ToolName;
use crate::pipeline::ToolGuard;
use crate::schema::ToolSchema;
use crate::tool::Tool;

/// A restriction on one scope; `allow = None` means "no allow-list", not "deny everything".
#[derive(Clone, Debug, Default)]
pub struct ToolRestriction {
    pub allow: Option<HashSet<ToolName>>,
    pub deny: HashSet<ToolName>,
}

impl ToolRestriction {
    /// These names and nothing else.
    pub fn allow_only<I: IntoIterator<Item = N>, N: Into<ToolName>>(names: I) -> ToolRestriction {
        ToolRestriction {
            allow: Some(names.into_iter().map(Into::into).collect()),
            deny: HashSet::new(),
        }
    }

    /// Everything except these names.
    pub fn deny_only<I: IntoIterator<Item = N>, N: Into<ToolName>>(names: I) -> ToolRestriction {
        ToolRestriction {
            allow: None,
            deny: names.into_iter().map(Into::into).collect(),
        }
    }

    /// `deny` beats `allow`: a name on both sides is a contradiction, and refusing is the only safe resolution.
    pub fn permits(&self, name: &ToolName) -> bool {
        if self.deny.contains(name) {
            return false;
        }
        match &self.allow {
            Some(allow) => allow.contains(name),
            None => true,
        }
    }
}

/// The result of resolving a name that came from the model.
pub enum Resolution {
    Found(Arc<dyn Tool>, ToolName),
    /// The tool exists, but this scope may not use it.
    Denied(ToolName),
    /// No tool of that name exists in this scope.
    Unknown(ToolName),
}

struct ToolEntry {
    id: u64,
    /// `None` means a global registration.
    scope: Option<ScopeKey>,
    name: ToolName,
    tool: Arc<dyn Tool>,
}

struct RestrictionEntry {
    id: u64,
    scope: ScopeKey,
    restriction: ToolRestriction,
}

struct PinEntry {
    id: u64,
    scope: ScopeKey,
    field: String,
    value: Value,
}

struct GuardEntry {
    id: u64,
    scope: Option<ScopeKey>,
    guard: Arc<dyn ToolGuard>,
}

/// Every tool the application can reach, plus the policy attached per scope.
#[derive(Default)]
pub struct ToolRegistry {
    /// The core's scope tree, asked directly, because lookups have no `Context` on the right branch.
    scopes: Arc<ScopeTree>,
    tools: RwLock<Vec<ToolEntry>>,
    restrictions: RwLock<Vec<RestrictionEntry>>,
    pins: RwLock<Vec<PinEntry>>,
    guards: RwLock<Vec<GuardEntry>>,
    next_id: AtomicU64,
}

impl ToolRegistry {
    pub fn new(ctx: &Context) -> Arc<ToolRegistry> {
        Arc::new(ToolRegistry {
            scopes: ctx.scopes(),
            ..Default::default()
        })
    }

    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    // --- registration --------------------------------------------------------------------

    /// A global registration: every scope sees it unless a restriction hides it.
    pub fn register(self: &Arc<Self>, tool: Arc<dyn Tool>) -> Guard {
        self.install(None, tool)
    }

    /// A scoped registration shadowing the global one of the same name, so a child can swap in its own version.
    pub fn register_in(self: &Arc<Self>, scope: ScopeKey, tool: Arc<dyn Tool>) -> Guard {
        self.install(Some(scope), tool)
    }

    fn install(self: &Arc<Self>, scope: Option<ScopeKey>, tool: Arc<dyn Tool>) -> Guard {
        let name = tool.schema().name;

        // A name containing `__` makes the wire mapping irreversible, so refusing to register is the fail-closed answer.
        if !name.round_trips() {
            tracing::error!(
                tool = %name,
                "refusing registration: the name contains `__`, so the wire encoding is not reversible"
            );
            return Guard::new(|| {});
        }

        let id = self.id();
        self.tools.write().push(ToolEntry {
            id,
            scope,
            name,
            tool,
        });
        let registry = self.clone();
        Guard::new(move || {
            registry.tools.write().retain(|entry| entry.id != id);
        })
    }

    /// Restrict a scope; several restrictions on one scope intersect, and the scope is required rather than global.
    pub fn restrict(self: &Arc<Self>, scope: ScopeKey, restriction: ToolRestriction) -> Guard {
        let id = self.id();
        self.restrictions.write().push(RestrictionEntry {
            id,
            scope,
            restriction,
        });
        let registry = self.clone();
        Guard::new(move || {
            registry.restrictions.write().retain(|entry| entry.id != id);
        })
    }

    /// Pin a parameter across a scope: it leaves the schema and is overridden at call time, never merely defaulted.
    pub fn pin(self: &Arc<Self>, scope: ScopeKey, field: impl Into<String>, value: Value) -> Guard {
        let id = self.id();
        self.pins.write().push(PinEntry {
            id,
            scope,
            field: field.into(),
            value,
        });
        let registry = self.clone();
        Guard::new(move || {
            registry.pins.write().retain(|entry| entry.id != id);
        })
    }

    /// Add a monotonic guard; global is allowed here, unlike [`Self::restrict`], because a guard can only narrow.
    pub fn add_guard(
        self: &Arc<Self>,
        scope: Option<ScopeKey>,
        guard: Arc<dyn ToolGuard>,
    ) -> Guard {
        let id = self.id();
        self.guards.write().push(GuardEntry { id, scope, guard });
        let registry = self.clone();
        Guard::new(move || {
            registry.guards.write().retain(|entry| entry.id != id);
        })
    }

    // --- lookup --------------------------------------------------------------------------

    /// Whether scope `at` sees a registration tagged `tag`; it walks up the tree, since narrowing is `ToolRestriction`'s job.
    fn admits(&self, at: Option<ScopeKey>, tag: Option<ScopeKey>) -> bool {
        self.scopes.admits(at, tag)
    }

    fn find(&self, scope: Option<ScopeKey>, name: &ToolName) -> Option<Arc<dyn Tool>> {
        let tools = self.tools.read();
        // Scoped registrations shadow global ones; within a tier, the newest wins.
        let scoped = tools
            .iter()
            .rfind(|e| e.scope.is_some() && self.admits(scope, e.scope) && &e.name == name);
        let global = || tools.iter().rfind(|e| e.scope.is_none() && &e.name == name);
        scoped.or_else(global).map(|entry| entry.tool.clone())
    }

    /// Whether this scope may use this tool; one function for both filtering stages.
    pub fn permits(&self, scope: Option<ScopeKey>, name: &ToolName) -> bool {
        let Some(scope) = scope else {
            // With no scope, no restriction can apply: this is the host's path, not an agent's.
            return true;
        };
        self.restrictions
            .read()
            .iter()
            .filter(|entry| entry.scope == scope)
            // Intersection: a name has to pass every restriction on the scope.
            .all(|entry| entry.restriction.permits(name))
    }

    /// The second filter: takes the model's name and returns an approved tool, trying an exact match before decoding `__`.
    pub fn resolve(&self, scope: Option<ScopeKey>, raw: &str) -> Resolution {
        let direct = ToolName::new(raw);
        let name = if self.find(scope, &direct).is_some() {
            direct
        } else {
            ToolName::from_wire(raw)
        };

        match self.find(scope, &name) {
            None => Resolution::Unknown(name),
            Some(tool) => {
                if self.permits(scope, &name) {
                    Resolution::Found(tool, name)
                } else {
                    Resolution::Denied(name)
                }
            }
        }
    }

    /// The first filter: what gets advertised to this scope.
    pub fn visible(&self, scope: Option<ScopeKey>) -> Vec<Arc<dyn Tool>> {
        let mut seen: HashSet<ToolName> = HashSet::new();
        let mut names: Vec<ToolName> = Vec::new();
        for entry in self.tools.read().iter() {
            if !self.admits(scope, entry.scope) || !self.permits(scope, &entry.name) {
                continue;
            }
            if seen.insert(entry.name.clone()) {
                names.push(entry.name.clone());
            }
        }
        names.sort();
        names
            .iter()
            .filter_map(|name| self.find(scope, name))
            .collect()
    }

    /// Schemas exactly as the model will see them: permission-filtered, notice-framed, pins hidden.
    pub fn schemas(&self, scope: Option<ScopeKey>) -> Vec<ToolSchema> {
        let pinned = self.pinned_fields(scope);
        self.visible(scope)
            .into_iter()
            .map(|tool| {
                let mut schema = tool.schema();
                schema.description = tool.meta().frame(&schema.description);
                for field in &pinned {
                    schema.hide_parameter(field);
                }
                schema
            })
            .collect()
    }

    /// The names of the parameters pinned in this scope.
    pub fn pinned_fields(&self, scope: Option<ScopeKey>) -> Vec<String> {
        let Some(scope) = scope else {
            return Vec::new();
        };
        let mut fields: Vec<String> = self
            .pins
            .read()
            .iter()
            .filter(|entry| entry.scope == scope)
            .map(|entry| entry.field.clone())
            .collect();
        fields.sort();
        fields.dedup();
        fields
    }

    /// Overwrite the model's arguments with the pinned values.
    pub fn apply_pins(&self, scope: Option<ScopeKey>, arguments: &mut Map<String, Value>) {
        let Some(scope) = scope else { return };
        for entry in self.pins.read().iter().filter(|entry| entry.scope == scope) {
            arguments.insert(entry.field.clone(), entry.value.clone());
        }
    }

    /// Guards applying to this scope, in registration order, which only affects which reason is reported first.
    pub fn guards(&self, scope: Option<ScopeKey>) -> Vec<Arc<dyn ToolGuard>> {
        self.guards
            .read()
            .iter()
            .filter(|entry| self.admits(scope, entry.scope))
            .map(|entry| entry.guard.clone())
            .collect()
    }
}
