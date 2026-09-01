//! Cây plugin dựng từ cấu hình theo lớp.
//!
//! Cả kiến trúc này dựa trên câu "mọi thứ đều thay được từ cấu hình". Chừng nào danh sách
//! plugin còn nằm trong mã thì câu đó chỉ là ý định. Tệp này biến nó thành sự thật.
//!
//! Ba khái niệm, mượn nguyên từ dsh:
//!
//! - **Hàng (row)** — một plugin cùng cấu hình của nó, mang một `id` bền vững.
//! - **Lớp (layer)** — một tập thao tác lên danh sách hàng: chèn, thay, hoặc tắt.
//! - **Áp lớp** — các lớp áp lần lượt lên một danh sách rỗng, theo đúng thứ tự đã cho.
//!
//! Hai quyết định khác dsh, cả hai đều vì đây là ứng dụng desktop chứ không phải CLI cho
//! lập trình viên:
//!
//! **Không có biểu thức trong cấu hình.** dsh cho phép `!!js` trong tệp cấu hình, tức là
//! thực thi mã tuỳ ý từ một tệp. Chấp nhận được cho một công cụ dòng lệnh; không chấp
//! nhận được cho một ứng dụng mà người dùng cài từ một tệp `.dmg`.
//!
//! **Tắt chứ không xoá.** Một lớp trên chỉ **tắt** được hàng của lớp dưới. Xoá hẳn thì
//! một hàng vắng mặt trong lớp trên sẽ lặng lẽ sống lại vào ngày ai đó đổi thứ tự lớp,
//! còn `disabled` thì luôn nhìn thấy trong `dump`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Một plugin trong cây, cùng cấu hình của nó.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// Danh tính bền vững. Lớp trên nhắm vào hàng bằng `id`, nên đổi `id` là mất mọi bản
    /// vá đang trỏ vào nó.
    pub id: String,
    /// Tên plugin, khớp với thứ đã đăng ký trong [`PluginCatalog`].
    pub plugin: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub disabled: bool,
}

/// Một thao tác lên danh sách hàng.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Patch {
    /// Thêm một hàng mới. Trùng `id` với hàng đã có là lỗi cấu hình, không phải ghi đè
    /// im lặng — người viết lớp gần như chắc chắn đang định `replace`.
    Insert(Row),
    /// Thay **toàn bộ** cấu hình của một hàng.
    ///
    /// Thay cả khối chứ không trộn vào: trộn thì không có cách nào xoá một trường, và
    /// người viết bản vá phải đoán xem trường nào của lớp dưới còn sót lại.
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

/// Một lớp cấu hình, kèm nơi nó đến từ.
#[derive(Clone, Debug, Deserialize)]
pub struct Layer {
    /// Đường dẫn tệp, tên bundle, hay `"--patch"`. Chỉ dùng để nói cho con người.
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

    /// Một lớp toàn hàng mới. Dạng thường gặp của lớp nền.
    pub fn base(origin: impl Into<String>, rows: Vec<Row>) -> Layer {
        Layer::new(origin, rows.into_iter().map(Patch::Insert).collect())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{origin}: đã có hàng `{id}`; dùng `replace` nếu muốn thay cấu hình của nó")]
    Duplicate { origin: String, id: String },
    #[error("{origin}: không có hàng nào tên `{id}` để {op}")]
    Unknown {
        origin: String,
        id: String,
        op: &'static str,
    },
}

/// Kết quả áp lớp: danh sách hàng, kèm dấu vết ai đã đụng vào hàng nào.
#[derive(Clone, Debug, Default)]
pub struct Composed {
    pub rows: Vec<Row>,
    /// `id` → nơi tạo ra nó, rồi tới những nơi đã sửa nó, theo thứ tự.
    pub provenance: HashMap<String, Vec<String>>,
}

impl Composed {
    /// Những hàng thật sự sẽ được cắm.
    pub fn active(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|row| !row.disabled)
    }

    /// Bản in cho con người — tương đương `--dump-config` của dsh.
    ///
    /// Có mặt vì cùng một lý do: trong một kiến trúc mà mọi thứ đều thay được từ cấu
    /// hình, câu hỏi đầu tiên khi có gì đó sai luôn là "bản đang chạy gồm những gì".
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

/// Áp các lớp lên một danh sách rỗng, theo đúng thứ tự đã cho.
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
                    let row = find(&mut out.rows, id, &layer.origin, "thay")?;
                    row.config = config.clone();
                    note(&mut out.provenance, id, &layer.origin);
                }
                Patch::Disable { id } => {
                    find(&mut out.rows, id, &layer.origin, "tắt")?.disabled = true;
                    note(&mut out.provenance, id, &layer.origin);
                }
                Patch::Enable { id } => {
                    find(&mut out.rows, id, &layer.origin, "bật")?.disabled = false;
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

/// Tên plugin → cách dựng nó từ cấu hình.
///
/// Đây là chỗ duy nhất một chuỗi trong tệp cấu hình biến thành mã. Danh sách đóng, khai
/// báo trong lúc khởi động: một tệp cấu hình không gọi được thứ chưa ai đăng ký ở đây.
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
            anyhow::anyhow!("hàng `{}`: không biết plugin `{}`", row.id, row.plugin)
        })?;
        builder(&row.config)
            .map_err(|err| anyhow::anyhow!("hàng `{}` ({}): {err}", row.id, row.plugin))
    }

    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.builders.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}
