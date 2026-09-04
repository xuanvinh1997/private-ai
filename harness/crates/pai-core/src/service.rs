//! Seams: the identity of a capability, separated from its implementation.
//! The key is an uninstantiable marker type and the value a trait object, so a misspelled
//! seam fails to compile; `NAME` exists only for config, logs and error messages.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A replaceable capability; declare `Self` as an empty `enum`, it is only a label.
pub trait ServiceKey: 'static {
    /// The interface consumers see. Must be object-safe.
    type Api: ?Sized + Send + Sync + 'static;
    /// The name used in config, logs and error messages.
    const NAME: &'static str;
}

/// A realm. Two providers of the same seam can coexist when they sit in different realms.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Realm(pub(crate) u64);

impl Realm {
    pub const ROOT: Realm = Realm(0);

    /// A fresh, unnamed realm.
    pub fn anonymous() -> Realm {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Realm(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A seam -> realm map as a linked list: O(1) inheritance and clone, O(depth) lookup.
#[derive(Clone, Default)]
pub struct RealmChain(Option<Arc<RealmNode>>);

struct RealmNode {
    key: TypeId,
    realm: Realm,
    parent: RealmChain,
}

impl RealmChain {
    pub fn lookup(&self, key: TypeId) -> Realm {
        let mut cursor = self.0.as_deref();
        while let Some(node) = cursor {
            if node.key == key {
                return node.realm;
            }
            cursor = node.parent.0.as_deref();
        }
        Realm::ROOT
    }

    pub fn push(&self, key: TypeId, realm: Realm) -> RealmChain {
        RealmChain(Some(Arc::new(RealmNode {
            key,
            realm,
            parent: self.clone(),
        })))
    }
}

/// One provider cell: `value` is an `Arc<K::Api>`, which is `Sized`, so `Box<dyn Any>` holds it.
pub(crate) struct ServiceCell {
    pub(crate) value: Box<dyn std::any::Any + Send + Sync>,
    pub(crate) name: &'static str,
}

/// A name -> seam lookup, so config and `--dump-config` can speak in strings.
#[derive(Default)]
pub struct ServiceCatalog {
    by_name: HashMap<&'static str, TypeId>,
}

impl ServiceCatalog {
    pub fn record<K: ServiceKey>(&mut self) {
        self.by_name.insert(K::NAME, TypeId::of::<K>());
    }

    pub fn lookup(&self, name: &str) -> Option<TypeId> {
        self.by_name.get(name).copied()
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.by_name.keys().copied()
    }
}
