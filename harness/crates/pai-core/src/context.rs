//! The context: service store, event bus, and ownership of effects.
//!
//! `Context` is a light handle — cloning it bumps a few `Arc`s. Every registration
//! function takes `&self` rather than `&mut self`, because every plugin needs to capture a
//! `Context` into an async listener, and `&mut` would not allow that.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::future::BoxFuture;
use parking_lot::RwLock;

use crate::effect::{EffectScope, Guard};
use crate::event::{First, Middleware, Next, Notify, Waterfall};
use crate::scope::{ScopeKey, ScopeTree};
use crate::service::{Realm, RealmChain, ServiceCatalog, ServiceCell, ServiceKey};

/// A single registration inside a listener chain.
struct Slot<T> {
    id: u64,
    scope: Option<ScopeKey>,
    item: T,
}

type NotifyFn<E> = Arc<dyn Fn(&<E as Notify>::Payload) + Send + Sync>;
type FirstFn<E> = Arc<
    dyn for<'a> Fn(&'a <E as First>::Payload) -> BoxFuture<'a, Option<<E as First>::Out>>
        + Send
        + Sync,
>;

/// The part shared by the whole application. One instance, pointed at by every `Context`.
#[derive(Default)]
pub struct Core {
    services: RwLock<HashMap<(TypeId, Realm), ServiceCell>>,
    catalog: RwLock<ServiceCatalog>,
    /// Listener chains, type-erased once by the event's `TypeId`.
    chains: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    /// Wakes whoever is waiting for a seam to appear.
    ready: RwLock<HashMap<TypeId, Arc<tokio::sync::Notify>>>,
    scopes: Arc<ScopeTree>,
    next_id: AtomicU64,
}

impl Core {
    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn wake(&self, key: TypeId) {
        if let Some(notify) = self.ready.read().get(&key) {
            notify.notify_waiters();
        }
    }

    fn gate(&self, key: TypeId) -> Arc<tokio::sync::Notify> {
        self.ready.write().entry(key).or_default().clone()
    }

    /// Read a listener chain, filter it by scope, and return a snapshot.
    ///
    /// A snapshot rather than a held lock: a plugin unloading mid-flight must not corrupt
    /// a chain that is already running. Cordis splices the shared array directly; here Rust
    /// is safer.
    fn snapshot<E: 'static, T: Clone + 'static>(&self, at: Option<ScopeKey>) -> Vec<T> {
        let chains = self.chains.read();
        let Some(any) = chains.get(&TypeId::of::<E>()) else {
            return Vec::new();
        };
        let Some(slots) = any.downcast_ref::<Vec<Slot<T>>>() else {
            return Vec::new();
        };
        slots
            .iter()
            .filter(|slot| self.scopes.admits(at, slot.scope))
            .map(|slot| slot.item.clone())
            .collect()
    }

    fn attach<E: 'static, T: Send + Sync + 'static>(
        self: &Arc<Self>,
        scope: Option<ScopeKey>,
        item: T,
        prepend: bool,
    ) -> Guard {
        let key = TypeId::of::<E>();
        let id = self.id();
        {
            let mut chains = self.chains.write();
            let entry = chains
                .entry(key)
                .or_insert_with(|| Box::new(Vec::<Slot<T>>::new()));
            let slots = entry
                .downcast_mut::<Vec<Slot<T>>>()
                .expect("one event TypeId maps to exactly one listener type");
            let slot = Slot { id, scope, item };
            if prepend {
                slots.insert(0, slot);
            } else {
                slots.push(slot);
            }
        }
        let core = self.clone();
        Guard::new(move || {
            let mut chains = core.chains.write();
            if let Some(any) = chains.get_mut(&key)
                && let Some(slots) = any.downcast_mut::<Vec<Slot<T>>>()
            {
                slots.retain(|slot| slot.id != id);
            }
        })
    }
}

/// A plugin's way in.
#[derive(Clone)]
pub struct Context {
    core: Arc<Core>,
    realms: RealmChain,
    scope: Option<ScopeKey>,
    effects: Arc<EffectScope>,
}

impl Context {
    /// The root of the tree. Called exactly once, at startup.
    pub fn root() -> Context {
        Context {
            core: Arc::new(Core::default()),
            realms: RealmChain::default(),
            scope: None,
            effects: EffectScope::new("root"),
        }
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    pub fn effects(&self) -> &Arc<EffectScope> {
        &self.effects
    }

    pub fn scope_key(&self) -> Option<ScopeKey> {
        self.scope
    }

    /// The shared scope tree.
    ///
    /// Exposed because things that hold scopes inside their own data — the tool registry,
    /// for one — have to answer parent–child questions without holding a `Context` on the
    /// right branch. Not exposing it would force them to match exactly, which makes an
    /// authority decision out of what is reachable rather than out of intent.
    pub fn scopes(&self) -> Arc<ScopeTree> {
        self.core.scopes.clone()
    }

    /// A context for one plugin: its effects belong to it, and unloading cleans up.
    pub fn plugin(&self, label: &'static str) -> Context {
        Context {
            effects: self.effects.child(label),
            ..self.clone()
        }
    }

    /// A context for a child agent. Listeners attached here do not touch other agents.
    pub fn scoped(&self, label: &'static str) -> Context {
        let scope = self.core.scopes.create(self.scope);
        let effects = self.effects.child(label);
        let core = self.core.clone();
        effects.defer("scope", move || core.scopes.remove(scope));
        Context {
            scope: Some(scope),
            effects,
            ..self.clone()
        }
    }

    /// Push a seam into a fresh realm. Providers registered afterwards do not leak out.
    pub fn isolate<K: ServiceKey>(&self) -> Context {
        self.isolate_into::<K>(Realm::anonymous())
    }

    pub fn isolate_into<K: ServiceKey>(&self, realm: Realm) -> Context {
        Context {
            realms: self.realms.push(TypeId::of::<K>(), realm),
            ..self.clone()
        }
    }

    fn slot<K: ServiceKey>(&self) -> (TypeId, Realm) {
        let key = TypeId::of::<K>();
        (key, self.realms.lookup(key))
    }

    /// Mount a provider for seam `K`.
    ///
    /// Two providers for the same seam in the same realm is a config error, not a silent
    /// overwrite: the one being replaced already has consumers holding it.
    pub fn provide<K: ServiceKey>(&self, api: Arc<K::Api>) -> Result<Guard, ProvideError> {
        let slot = self.slot::<K>();
        {
            let mut services = self.core.services.write();
            if services.contains_key(&slot) {
                return Err(ProvideError::Duplicate(K::NAME));
            }
            services.insert(
                slot,
                ServiceCell {
                    value: Box::new(api),
                    name: K::NAME,
                },
            );
        }
        self.core.catalog.write().record::<K>();
        self.core.wake(slot.0);

        let core = self.core.clone();
        Ok(Guard::new(move || {
            core.services.write().remove(&slot);
            core.wake(slot.0);
        }))
    }

    pub fn get<K: ServiceKey>(&self) -> Option<Arc<K::Api>> {
        let slot = self.slot::<K>();
        self.core
            .services
            .read()
            .get(&slot)?
            // Never returns the wrong thing: the cell's key already contains
            // `TypeId::of::<K>()`, and `K → K::Api` is one-to-one by the trait itself.
            .value
            .downcast_ref::<Arc<K::Api>>()
            .cloned()
    }

    pub fn require<K: ServiceKey>(&self) -> Result<Arc<K::Api>, ProvideError> {
        self.get::<K>().ok_or(ProvideError::Missing(K::NAME))
    }

    /// Wait until the seam exists. This is the Rust version of `inject` — load order is
    /// expressed as a need for a service, not as a hand-written startup sequence.
    pub async fn wait_for<K: ServiceKey>(&self) -> Arc<K::Api> {
        loop {
            if let Some(api) = self.get::<K>() {
                return api;
            }
            let gate = self.core.gate(TypeId::of::<K>());
            let waiting = gate.notified();
            // Re-check after registering the wait, or a signal slipping in between is
            // missed.
            if let Some(api) = self.get::<K>() {
                return api;
            }
            waiting.await;
        }
    }

    /// Every seam ever declared, including those with no provider yet.
    pub fn service_names(&self) -> Vec<&'static str> {
        let catalog = self.core.catalog.read();
        let mut names: Vec<_> = catalog.names().collect();
        names.sort_unstable();
        names
    }

    /// Currently mounted providers with their realms — the basis for `--dump-config`.
    pub fn mounted(&self) -> Vec<(&'static str, Realm)> {
        let services = self.core.services.read();
        let mut rows: Vec<_> = services
            .iter()
            .map(|((_, realm), cell)| (cell.name, *realm))
            .collect();
        rows.sort_unstable_by_key(|(name, realm)| (*name, realm.0));
        rows
    }

    // --- events --------------------------------------------------------------------

    pub fn on_notify<E: Notify>(
        &self,
        listener: impl Fn(&E::Payload) + Send + Sync + 'static,
    ) -> Guard {
        let item: NotifyFn<E> = Arc::new(listener);
        self.core.attach::<E, NotifyFn<E>>(self.scope, item, false)
    }

    pub fn notify<E: Notify>(&self, payload: &E::Payload) {
        for listener in self.core.snapshot::<E, NotifyFn<E>>(self.scope) {
            listener(payload);
        }
    }

    pub fn on_first<E: First>(
        &self,
        listener: impl for<'a> Fn(&'a E::Payload) -> BoxFuture<'a, Option<E::Out>>
        + Send
        + Sync
        + 'static,
    ) -> Guard {
        let item: FirstFn<E> = Arc::new(listener);
        self.core.attach::<E, FirstFn<E>>(self.scope, item, false)
    }

    /// Run listeners in order, stopping at the first one that answers.
    pub async fn first<E: First>(&self, payload: &E::Payload) -> Option<E::Out> {
        for listener in self.core.snapshot::<E, FirstFn<E>>(self.scope) {
            if let Some(out) = listener(payload).await {
                return Some(out);
            }
        }
        None
    }

    pub fn on_waterfall<E: Waterfall>(&self, middleware: Arc<dyn Middleware<E>>) -> Guard {
        self.core
            .attach::<E, Arc<dyn Middleware<E>>>(self.scope, middleware, false)
    }

    /// Register a layer that runs **before** every existing one. For policy that has to
    /// go first, not for layers to race each other for position.
    pub fn on_waterfall_first<E: Waterfall>(&self, middleware: Arc<dyn Middleware<E>>) -> Guard {
        self.core
            .attach::<E, Arc<dyn Middleware<E>>>(self.scope, middleware, true)
    }

    pub async fn waterfall<E, F>(&self, req: &mut E::Req, tail: F) -> E::Out
    where
        E: Waterfall,
        F: for<'r> Fn(&'r mut E::Req) -> BoxFuture<'r, E::Out> + Send + Sync,
    {
        let chain = self.core.snapshot::<E, Arc<dyn Middleware<E>>>(self.scope);
        Next::<E> {
            rest: &chain,
            tail: &tail,
        }
        .run(req)
        .await
    }

    pub fn keep<G: Send + 'static>(&self, guard: G) {
        self.effects.keep(guard);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProvideError {
    #[error("service `{0}` is already registered in this realm")]
    Duplicate(&'static str),
    #[error("missing service `{0}`")]
    Missing(&'static str),
}
