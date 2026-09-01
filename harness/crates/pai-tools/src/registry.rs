//! Sổ đăng ký tool, có phạm vi.
//!
//! Bất biến trung tâm của cả crate nằm ở đây, và nó là **lọc hai tầng**:
//!
//! > Danh sách quảng cáo chỉ là gợi ý. Quyền phải được kiểm lại lúc gọi, **sau khi đã
//! > giải mã tên**, vì một mô hình đoán ra `documents__delete` đi thẳng vào hàm gọi mà
//! > không bao giờ đi qua hàm liệt kê.
//!
//! Bản Python làm đúng chỗ này (`adapter.py:141-152`) và đó là lý do nó được chép sang
//! nguyên vẹn. Cách sắp xếp ở đây cố tình khiến chỗ sai khó viết ra: hàm liệt kê và hàm
//! tra cứu gọi **cùng một** [`ToolRegistry::permits`], nên không có đường nào để hai
//! tầng lọc trôi ra khỏi nhau qua thời gian.

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

/// Một hạn chế đặt lên một phạm vi.
///
/// `allow = None` nghĩa là "không giới hạn danh sách trắng", **không** phải "cấm hết".
/// Đó là mặc định của một agent bình thường; danh sách trắng là thứ một agent bị siết
/// phải nói ra tường minh.
#[derive(Clone, Debug, Default)]
pub struct ToolRestriction {
    pub allow: Option<HashSet<ToolName>>,
    pub deny: HashSet<ToolName>,
}

impl ToolRestriction {
    /// Chỉ những tên này, không gì khác.
    pub fn allow_only<I: IntoIterator<Item = N>, N: Into<ToolName>>(names: I) -> ToolRestriction {
        ToolRestriction {
            allow: Some(names.into_iter().map(Into::into).collect()),
            deny: HashSet::new(),
        }
    }

    /// Mọi thứ trừ những tên này.
    pub fn deny_only<I: IntoIterator<Item = N>, N: Into<ToolName>>(names: I) -> ToolRestriction {
        ToolRestriction {
            allow: None,
            deny: names.into_iter().map(Into::into).collect(),
        }
    }

    /// `deny` thắng `allow`. Một cái tên nằm ở cả hai bên là một mâu thuẫn cấu hình, và
    /// cách giải quyết an toàn duy nhất là nghiêng về phía từ chối.
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

/// Kết quả tra cứu một cái tên đến từ mô hình.
pub enum Resolution {
    Found(Arc<dyn Tool>, ToolName),
    /// Có tool này, nhưng phạm vi hiện tại không được dùng.
    Denied(ToolName),
    /// Không có tool nào tên như vậy trong phạm vi này.
    Unknown(ToolName),
}

struct ToolEntry {
    id: u64,
    /// `None` là đăng ký toàn cục.
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

/// Mọi tool mà ứng dụng với tới được, cộng chính sách gắn theo phạm vi.
#[derive(Default)]
pub struct ToolRegistry {
    /// Cây phạm vi của lõi. Sổ đăng ký giữ phạm vi trong dữ liệu của mình nên không có
    /// sẵn một `Context` đúng nhánh lúc tra cứu; nó hỏi cây trực tiếp.
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

    // --- đăng ký -----------------------------------------------------------------------

    /// Đăng ký toàn cục: mọi phạm vi đều thấy, trừ khi bị hạn chế che đi.
    pub fn register(self: &Arc<Self>, tool: Arc<dyn Tool>) -> Guard {
        self.install(None, tool)
    }

    /// Đăng ký trong một phạm vi. **Che** đăng ký toàn cục cùng tên.
    ///
    /// Đây là cách một agent con thay một tool bằng bản của riêng nó — một `bash` bị giam
    /// trong sandbox chẳng hạn — mà không đụng tới agent khác.
    pub fn register_in(self: &Arc<Self>, scope: ScopeKey, tool: Arc<dyn Tool>) -> Guard {
        self.install(Some(scope), tool)
    }

    fn install(self: &Arc<Self>, scope: Option<ScopeKey>, tool: Arc<dyn Tool>) -> Guard {
        let name = tool.schema().name;

        // Một cái tên chứa sẵn `__` phá mất tính khả nghịch của phép chiếu sang wire: hai
        // tool khác nhau va vào cùng một tên mô hình gõ được, và bộ lọc quyền hết là bộ
        // lọc. Từ chối đăng ký là fail-closed — tool không tồn tại, thay vì tồn tại dưới
        // một cái tên mà chính sách không kiểm được.
        if !name.round_trips() {
            tracing::error!(
                tool = %name,
                "từ chối đăng ký: tên chứa `__` nên không mã hoá sang wire một cách khả nghịch"
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

    /// Siết một phạm vi. Nhiều hạn chế trên cùng phạm vi thì **giao nhau**.
    ///
    /// Phạm vi là bắt buộc, và đó là một quyết định chứ không phải một thiếu sót của API:
    /// một hạn chế toàn cục bịt mắt mọi agent cùng lúc, kể cả cái đang chạy việc mà người
    /// dùng vừa bấm nút. Siết thì siết đúng agent cần siết.
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

    /// Ghim một tham số cho cả phạm vi.
    ///
    /// Nửa đầu: tham số biến mất khỏi schema. Nửa sau: nó bị **ghi đè** lúc gọi. Ghi đè
    /// chứ không phải điền mặc định — một mô hình tự gửi kèm id của mình không được phép
    /// với sang workspace khác chỉ vì nó đoán trúng một id.
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

    /// Cắm một canh gác đơn điệu.
    ///
    /// Toàn cục được phép ở đây, khác với [`Self::restrict`], vì một canh gác **chỉ từ
    /// chối được**: thêm nó vào chỉ có thể thu hẹp, không bao giờ mở rộng. Xem
    /// [`ToolGuard`].
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

    // --- tra cứu -----------------------------------------------------------------------

    /// Phạm vi `at` có nhìn thấy đăng ký gắn ở `tag` không.
    ///
    /// Đi ngược cây: một agent con thấy tool mà cha nó đăng ký. Đây là ngữ nghĩa đúng vì
    /// công cụ để **thu hẹp** quyền của agent con là `ToolRestriction`, thứ nói ra ý
    /// định ngay tại chỗ đọc. Bắt phạm vi kiêm luôn việc thu hẹp thì một agent con mất
    /// tool mà không có dòng nào nói rằng nó đã bị lấy đi.
    fn admits(&self, at: Option<ScopeKey>, tag: Option<ScopeKey>) -> bool {
        self.scopes.admits(at, tag)
    }

    fn find(&self, scope: Option<ScopeKey>, name: &ToolName) -> Option<Arc<dyn Tool>> {
        let tools = self.tools.read();
        // Đăng ký có phạm vi che đăng ký toàn cục; trong cùng một tầng, cái mới nhất thắng.
        let scoped = tools
            .iter()
            .rfind(|e| e.scope.is_some() && self.admits(scope, e.scope) && &e.name == name);
        let global = || tools.iter().rfind(|e| e.scope.is_none() && &e.name == name);
        scoped.or_else(global).map(|entry| entry.tool.clone())
    }

    /// Phạm vi này có được dùng tool này không. **Một hàm duy nhất cho cả hai tầng lọc.**
    pub fn permits(&self, scope: Option<ScopeKey>, name: &ToolName) -> bool {
        let Some(scope) = scope else {
            // Không có phạm vi thì không có hạn chế nào áp được — đây là đường của host,
            // không phải đường của agent.
            return true;
        };
        self.restrictions
            .read()
            .iter()
            .filter(|entry| entry.scope == scope)
            // Giao nhau: một cái tên phải qua được **mọi** hạn chế đang đặt trên phạm vi.
            .all(|entry| entry.restriction.permits(name))
    }

    /// Tầng lọc thứ hai. Nhận cái tên **mô hình gửi** và trả về tool đã được duyệt.
    ///
    /// Thứ tự thử: khớp đúng trước, rồi mới giải mã `__`. Vì đăng ký đã từ chối mọi tên
    /// chứa `__`, hai đường không bao giờ trỏ vào hai tool khác nhau.
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

    /// Tầng lọc thứ nhất: cái gì được quảng cáo cho phạm vi này.
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

    /// Schema đúng như mô hình sẽ thấy: đã lọc quyền, đã chèn cảnh báo, đã giấu tham số ghim.
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

    /// Tên các tham số bị ghim trong phạm vi này.
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

    /// Ghi đè tham số ghim lên tham số mô hình gửi.
    pub fn apply_pins(&self, scope: Option<ScopeKey>, arguments: &mut Map<String, Value>) {
        let Some(scope) = scope else { return };
        for entry in self.pins.read().iter().filter(|entry| entry.scope == scope) {
            arguments.insert(entry.field.clone(), entry.value.clone());
        }
    }

    /// Canh gác áp dụng cho phạm vi này, theo thứ tự đăng ký.
    ///
    /// Thứ tự chỉ ảnh hưởng tới **lý do** nào được báo trước, không ảnh hưởng tới việc có
    /// bị từ chối hay không — xem [`ToolGuard`].
    pub fn guards(&self, scope: Option<ScopeKey>) -> Vec<Arc<dyn ToolGuard>> {
        self.guards
            .read()
            .iter()
            .filter(|entry| self.admits(scope, entry.scope))
            .map(|entry| entry.guard.clone())
            .collect()
    }
}
