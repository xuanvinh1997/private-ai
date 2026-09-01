//! Ráp prompt hệ thống.
//!
//! Plugin đóng góp từng khối, mỗi khối mang một số thứ tự. **Thứ tự là biên giới tin
//! cậy**, không phải sở thích trình bày: danh tính và chỉ dẫn do người vận hành viết đứng
//! trước, dữ liệu truy hồi đứng sau. Đảo lại là để một đoạn văn bản lấy từ đâu đó nói
//! chuyện với mô hình trước khi luật của chính ta kịp nói.

use std::sync::Arc;

use pai_core::ServiceKey;
use parking_lot::RwLock;

/// Càng nhỏ càng gần đầu prompt.
pub mod order {
    /// Ta là ai, làm gì, không làm gì.
    pub const IDENTITY: i32 = 0;
    /// Quy trình đóng gói sẵn do người vận hành viết. Đáng tin.
    pub const SKILLS: i32 = 100;
    /// Thư mục làm việc, cấu trúc dự án, quy ước của repo.
    pub const WORKSPACE: i32 = 200;
    /// Ghi nhớ cá nhân.
    pub const MEMORY: i32 = 300;
    /// Trích đoạn tài liệu, kết quả web. **Không đáng tin** — luôn đứng cuối.
    pub const RETRIEVED: i32 = 900;
}

/// Khối được tính lại mỗi lần ráp, nên nó là một hàm chứ không phải một chuỗi.
type Render = Arc<dyn Fn() -> Option<String> + Send + Sync>;

struct Section {
    id: u64,
    order: i32,
    text: Render,
}

/// Sổ các khối prompt.
#[derive(Default)]
pub struct SystemPrompt {
    sections: RwLock<Vec<Section>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl SystemPrompt {
    pub fn new() -> Arc<SystemPrompt> {
        Arc::new(SystemPrompt::default())
    }

    /// Đóng góp một khối. Khối được tính **mỗi lần ráp**, không phải một lần lúc đăng ký:
    /// thư mục làm việc đổi giữa chừng thì prompt phải đổi theo.
    pub fn contribute(
        self: &Arc<Self>,
        order: i32,
        text: impl Fn() -> Option<String> + Send + Sync + 'static,
    ) -> pai_core::Guard {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.sections.write().push(Section {
            id,
            order,
            text: Arc::new(text),
        });
        let registry = self.clone();
        pai_core::Guard::new(move || {
            registry.sections.write().retain(|section| section.id != id);
        })
    }

    pub fn assemble(&self) -> String {
        let mut sections: Vec<(i32, u64, Render)> = self
            .sections
            .read()
            .iter()
            .map(|s| (s.order, s.id, s.text.clone()))
            .collect();
        // Thứ tự đăng ký phân giải khi cùng `order`, nên prompt tất định giữa hai lần chạy.
        sections.sort_unstable_by_key(|(order, id, _)| (*order, *id));
        sections
            .into_iter()
            .filter_map(|(_, _, text)| text())
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

pub enum Prompt {}
impl ServiceKey for Prompt {
    type Api = SystemPrompt;
    const NAME: &'static str = "system-prompt";
}
