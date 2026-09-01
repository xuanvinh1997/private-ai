//! Seam: danh tính của một khả năng, tách khỏi bản cài đặt của nó.
//!
//! Cordis đánh địa chỉ service bằng chuỗi (`ctx.tools`). Ở Rust làm vậy là đổi lỗi
//! biên dịch lấy lỗi lúc chạy mà chẳng được gì, nên khoá là một **marker type** không
//! khởi tạo được, còn giá trị là một **trait object**. Consumer vẫn chỉ biết giao diện —
//! đổi provider không đụng call site — mà gõ sai tên thì không biên dịch được.
//!
//! Chuỗi `NAME` vẫn tồn tại, nhưng chỉ để nói với con người và với tệp cấu hình:
//! `--dump-config`, `isolate:`, thông báo lỗi.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Một khả năng thay thế được.
///
/// `Self` là nhãn thuần tuý. Khai báo nó bằng `enum` rỗng để không ai lỡ tay dựng một
/// giá trị của nó.
pub trait ServiceKey: 'static {
    /// Giao diện mà consumer nhìn thấy. Phải object-safe.
    type Api: ?Sized + Send + Sync + 'static;
    /// Tên cho cấu hình, log và thông báo lỗi.
    const NAME: &'static str;
}

/// Một cõi. Hai provider của cùng một seam sống cạnh nhau được khi ở hai cõi khác nhau.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Realm(pub(crate) u64);

impl Realm {
    pub const ROOT: Realm = Realm(0);

    /// Một cõi mới, không tên — tương đương `ctx.isolate('shell')`.
    pub fn anonymous() -> Realm {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Realm(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Bản đồ seam → cõi, dạng danh sách liên kết.
///
/// Kế thừa là O(1) và `Clone` chỉ là một lần tăng `Arc`; tra cứu O(độ sâu), mà độ sâu
/// thực tế dưới năm. Đây là bản Rust của prototype chain mà Cordis dùng.
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

/// Ô chứa một provider.
///
/// `value` thực chất giữ `Arc<K::Api>`. Một `Arc<dyn Trait>` là con trỏ béo nhưng vẫn
/// `Sized + 'static`, nên nó nhét vừa `Box<dyn Any>` và lấy lại nguyên vẹn được — đó là
/// mấu chốt khiến cách này chạy được.
pub(crate) struct ServiceCell {
    pub(crate) value: Box<dyn std::any::Any + Send + Sync>,
    pub(crate) name: &'static str,
}

/// Bảng tra tên → seam, để cấu hình và `--dump-config` nói được bằng chuỗi.
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
