//! Ngữ cảnh: kho service, xe buýt sự kiện, và quyền sở hữu hiệu ứng.
//!
//! `Context` là một handle nhẹ — clone nó chỉ tăng vài `Arc`. Mọi hàm đăng ký nhận
//! `&self` chứ không `&mut self`, vì plugin nào cũng cần bắt một bản `Context` vào
//! listener bất đồng bộ, mà `&mut` thì không cho làm thế.

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

/// Một đăng ký đơn lẻ trong một chuỗi listener.
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

/// Phần dùng chung của cả ứng dụng. Một bản duy nhất, mọi `Context` cùng trỏ vào.
#[derive(Default)]
pub struct Core {
    services: RwLock<HashMap<(TypeId, Realm), ServiceCell>>,
    catalog: RwLock<ServiceCatalog>,
    /// Chuỗi listener, xoá kiểu một lần theo `TypeId` của sự kiện.
    chains: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    /// Đánh thức ai đang chờ một seam xuất hiện.
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

    /// Đọc một chuỗi listener, lọc theo phạm vi, trả về bản chụp.
    ///
    /// Chụp lại chứ không giữ khoá: một plugin gỡ tải giữa chừng không được làm hỏng
    /// chuỗi đang chạy. Cordis cắt thẳng vào mảng dùng chung; chỗ này Rust an toàn hơn.
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
                .expect("một TypeId sự kiện chỉ ứng với đúng một kiểu listener");
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

/// Cửa vào của một plugin.
#[derive(Clone)]
pub struct Context {
    core: Arc<Core>,
    realms: RealmChain,
    scope: Option<ScopeKey>,
    effects: Arc<EffectScope>,
}

impl Context {
    /// Gốc của cây. Gọi đúng một lần, lúc khởi động.
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

    /// Cây phạm vi dùng chung.
    ///
    /// Lộ ra ngoài vì những thứ giữ phạm vi trong dữ liệu của mình — sổ đăng ký tool là
    /// ví dụ — phải trả lời được câu hỏi cha–con mà không có sẵn một `Context` đúng
    /// nhánh trong tay. Không lộ ra thì chúng buộc phải khớp đúng, và đó là một quyết
    /// định về quyền bị ép bởi khả năng truy cập chứ không phải bởi chủ ý.
    pub fn scopes(&self) -> Arc<ScopeTree> {
        self.core.scopes.clone()
    }

    /// Ngữ cảnh cho một plugin: hiệu ứng của nó thuộc về nó, gỡ tải là dọn sạch.
    pub fn plugin(&self, label: &'static str) -> Context {
        Context {
            effects: self.effects.child(label),
            ..self.clone()
        }
    }

    /// Ngữ cảnh cho một agent con. Listener gắn ở đây không chạm tới agent khác.
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

    /// Đẩy một seam sang cõi mới. Provider đăng ký sau đó không lọt ra ngoài.
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

    /// Cắm một provider cho seam `K`.
    ///
    /// Hai provider cho cùng một seam trong cùng một cõi là lỗi cấu hình, không phải
    /// chuyện im lặng ghi đè: cái bị thay thế đã có consumer đang cầm rồi.
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
            // Không bao giờ trả nhầm: khoá của ô đã chứa `TypeId::of::<K>()`, mà quan hệ
            // `K → K::Api` là một-một do chính trait quy định.
            .value
            .downcast_ref::<Arc<K::Api>>()
            .cloned()
    }

    pub fn require<K: ServiceKey>(&self) -> Result<Arc<K::Api>, ProvideError> {
        self.get::<K>().ok_or(ProvideError::Missing(K::NAME))
    }

    /// Chờ tới khi seam có mặt. Đây là bản Rust của `inject` — thứ tự nạp được diễn đạt
    /// bằng nhu cầu về service, không bằng một trình tự khởi động viết tay.
    pub async fn wait_for<K: ServiceKey>(&self) -> Arc<K::Api> {
        loop {
            if let Some(api) = self.get::<K>() {
                return api;
            }
            let gate = self.core.gate(TypeId::of::<K>());
            let waiting = gate.notified();
            // Kiểm lại sau khi đã đăng ký chờ, nếu không sẽ bỏ lỡ tín hiệu chen vào giữa.
            if let Some(api) = self.get::<K>() {
                return api;
            }
            waiting.await;
        }
    }

    /// Mọi seam đã từng được khai báo, kể cả cái chưa có provider nào.
    pub fn service_names(&self) -> Vec<&'static str> {
        let catalog = self.core.catalog.read();
        let mut names: Vec<_> = catalog.names().collect();
        names.sort_unstable();
        names
    }

    /// Provider đang cắm, kèm cõi của chúng — nền cho `--dump-config`.
    pub fn mounted(&self) -> Vec<(&'static str, Realm)> {
        let services = self.core.services.read();
        let mut rows: Vec<_> = services
            .iter()
            .map(|((_, realm), cell)| (cell.name, *realm))
            .collect();
        rows.sort_unstable_by_key(|(name, realm)| (*name, realm.0));
        rows
    }

    // --- sự kiện -------------------------------------------------------------------

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

    /// Chạy listener theo thứ tự, dừng ở cái đầu tiên trả lời.
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

    /// Đăng ký một tầng chạy **trước** mọi tầng đã có. Dành cho chính sách phải đi đầu,
    /// không phải để giành thứ tự với nhau.
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
    #[error("service `{0}` đã được đăng ký trong cõi này")]
    Duplicate(&'static str),
    #[error("thiếu service `{0}`")]
    Missing(&'static str),
}
