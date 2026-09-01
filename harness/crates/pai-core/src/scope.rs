//! Phạm vi theo agent.
//!
//! Một đăng ký không gắn phạm vi thì mọi agent đều thấy. Một đăng ký có gắn thì chỉ
//! agent đó — và con cháu của nó — mới thấy. Sự kiện chảy **lên** cây: một listener gắn
//! ở agent cha nhận được sự kiện phát ra từ agent con, không có chiều ngược lại.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

/// Danh tính một phạm vi. Thường là một agent hoặc một phiên.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ScopeKey(u64);

/// Quan hệ cha–con giữa các phạm vi.
#[derive(Default)]
pub struct ScopeTree {
    parents: RwLock<HashMap<ScopeKey, Option<ScopeKey>>>,
}

impl ScopeTree {
    pub fn create(&self, parent: Option<ScopeKey>) -> ScopeKey {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let key = ScopeKey(NEXT.fetch_add(1, Ordering::Relaxed));
        self.parents.write().insert(key, parent);
        key
    }

    pub fn remove(&self, key: ScopeKey) {
        self.parents.write().remove(&key);
    }

    pub fn parent(&self, key: ScopeKey) -> Option<ScopeKey> {
        self.parents.read().get(&key).copied().flatten()
    }

    /// Listener có `tag` có nhận được sự kiện phát ra tại `dispatch` không?
    ///
    /// Không tag thì nhận tất. Có tag thì nhận khi tag chính là nơi phát, hoặc là tổ
    /// tiên của nơi phát.
    pub fn admits(&self, dispatch: Option<ScopeKey>, tag: Option<ScopeKey>) -> bool {
        let Some(tag) = tag else { return true };
        let mut cursor = dispatch;
        while let Some(key) = cursor {
            if key == tag {
                return true;
            }
            cursor = self.parent(key);
        }
        false
    }
}
