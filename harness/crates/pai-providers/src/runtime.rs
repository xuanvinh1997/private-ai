//! Một đường duy nhất để đổi nhà cung cấp.
//!
//! Đổi provider chạm vào ba thứ: hàng trên đĩa, cache adapter, và cái `Driver` đang cầm
//! adapter. Nếu mỗi chỗ gọi tự làm ba bước đó thì sớm muộn có một chỗ quên bước thứ ba,
//! và triệu chứng là "đã bấm đổi rồi mà vẫn chạy máy chủ cũ" — một lỗi không để lại dấu
//! vết nào trong log. Nên [`ProviderRuntime`] không phải một tiện ích: nó là chỗ duy nhất
//! biết cả ba, cùng tinh thần với `Harness::open_project`.

use std::sync::Arc;

use pai_agent::Driver;
use pai_llm::{AdapterRegistry, ProviderConfig};

use crate::error::{ProviderError, Result};
use crate::presets;
use crate::probe::{EmbeddingProbeResult, ProbeResult, probe, probe_embedding};
use crate::store::{ProviderInput, ProviderStore, Role, StoredProvider};

pub struct ProviderRuntime {
    store: Arc<dyn ProviderStore>,
    registry: Arc<AdapterRegistry>,
    driver: Arc<Driver>,
    http: reqwest::Client,
}

impl ProviderRuntime {
    pub fn new(
        store: Arc<dyn ProviderStore>,
        registry: Arc<AdapterRegistry>,
        driver: Arc<Driver>,
        http: reqwest::Client,
    ) -> ProviderRuntime {
        ProviderRuntime {
            store,
            registry,
            driver,
            http,
        }
    }

    pub fn store(&self) -> &dyn ProviderStore {
        self.store.as_ref()
    }

    pub fn registry(&self) -> &AdapterRegistry {
        &self.registry
    }

    pub fn list(&self) -> Result<Vec<StoredProvider>> {
        self.store.list()
    }

    /// Provider đang giữ vai hội thoại.
    pub fn active(&self) -> Result<Option<StoredProvider>> {
        self.store.active(Role::Chat)
    }

    /// Provider đang giữ vai nhúng. `None` là trạng thái thường gặp và hợp lệ: chưa ai
    /// được hỏi câu "tài liệu của tôi được nhúng ở đâu", nên chưa ai trả lời.
    pub fn embedding(&self) -> Result<Option<StoredProvider>> {
        self.store.active(Role::Embedding)
    }

    /// Bộ nhúng đang có hiệu lực, hoặc `None` kèm lý do đọc được ở
    /// [`crate::embed::embedding_reason`].
    ///
    /// Một lời gọi thay vì hai, vì chỗ dùng nó — `app/` lúc áp lại provider — không có
    /// việc gì khác để làm với hàng `StoredProvider` ở giữa.
    pub fn embedder(&self) -> Result<Option<Arc<dyn pai_rag::Embedder>>> {
        Ok(self
            .embedding()?
            .as_ref()
            .and_then(crate::embed::embedder_for))
    }

    /// Trao vai nhúng, kèm mô hình nhúng nếu người dùng vừa chọn.
    ///
    /// Không đụng tới `Driver`: bộ nhúng không nằm trong đường chạy một lượt hội thoại.
    /// Nhưng vẫn đi qua [`ProviderRuntime::resync`] như mọi lối sửa khác — một đường áp
    /// lại duy nhất là cả lý do runtime này tồn tại, và một ngoại lệ "chỗ này thì không
    /// cần" là chỗ để quên mất một bước khi luật đổi.
    pub async fn set_embedding(&self, id: &str, model: Option<&str>) -> Result<StoredProvider> {
        let active = self.store.activate(Role::Embedding, id, model)?;
        self.resync().await;
        Ok(active)
    }

    /// Lưu một biểu mẫu rồi đồng bộ lại đường chạy.
    ///
    /// Cả khi hàng vừa sửa **không** phải cái đang hoạt động: sửa xong vẫn phải đi qua
    /// [`ProviderRuntime::apply_active`], vì luật chọn có ba tầng dự phòng và một cú tắt
    /// `enabled` đủ để đổi người thắng.
    pub async fn save(&self, input: ProviderInput) -> Result<StoredProvider> {
        let saved = self.store.save(input)?;
        self.resync().await;
        Ok(saved)
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        self.store.remove(id)?;
        self.resync().await;
        Ok(())
    }

    /// Trao vai hội thoại.
    pub async fn activate(&self, id: &str, model: Option<&str>) -> Result<StoredProvider> {
        let active = self.store.activate(Role::Chat, id, model)?;
        // Đây là lần đổi mà người dùng thực sự yêu cầu, nên lỗi phải nổi lên tới họ chứ
        // không chìm vào một dòng log như ở `resync`.
        self.apply_active().await?;
        Ok(active)
    }

    /// Dựng adapter từ provider đang giữ **vai hội thoại** và đẩy nó vào [`Driver`].
    ///
    /// Chỉ vai hội thoại: `Driver` chạy một lượt nói chuyện, và provider giữ vai nhúng có
    /// thể là một máy chủ hoàn toàn khác — thường thì đúng là thế.
    ///
    /// `async` vì đây là điểm hẹn cho mọi việc cần mạng khi đổi provider — hâm nóng kết
    /// nối, hỏi năng lực mô hình — và đổi chữ ký một hàm đã có mười chỗ gọi thì đắt hơn
    /// nhiều so với giữ sẵn một `await` không tốn gì.
    pub async fn apply_active(&self) -> Result<()> {
        let Some(active) = self.store.active(Role::Chat)? else {
            return Err(ProviderError::Llm(pai_llm::registry::no_provider()));
        };
        let adapter = self.registry.adapter(&active.config)?;
        self.driver.set_llm(adapter);
        if let Some(model) = model_for(&active) {
            self.driver.set_model(model);
        }
        tracing::info!(
            provider = %active.config.name,
            on_device = active.config.on_device(),
            model = %self.driver.model(),
            "đã đổi nhà cung cấp"
        );
        Ok(())
    }

    /// Thử một cấu hình chưa lưu.
    pub async fn probe(&self, config: &ProviderConfig) -> ProbeResult {
        probe(config, &self.http).await
    }

    /// Thử **nhúng thật một câu** bằng một mô hình, trên một cấu hình có thể chưa lưu.
    pub async fn probe_embedding(
        &self,
        config: &ProviderConfig,
        model: &str,
    ) -> EmbeddingProbeResult {
        probe_embedding(config, model).await
    }

    /// Đồng bộ sau một thay đổi mà bản thân nó đã thành công.
    ///
    /// Nuốt lỗi thành một dòng log, cố ý: xoá provider cuối cùng là một thao tác hợp lệ và
    /// nó *phải* thành công, dù sau đó chẳng còn gì để dựng adapter. Báo lỗi ở đây làm
    /// người dùng tưởng cú xoá không ăn.
    async fn resync(&self) {
        if let Err(err) = self.apply_active().await {
            tracing::warn!("không áp dụng được nhà cung cấp đang hoạt động: {err}");
        }
    }
}

/// Mô hình nào cho provider này: cái người dùng đã chọn, nếu không thì mặc định của mục
/// danh mục cùng địa chỉ.
///
/// `None` nghĩa là **giữ nguyên tên mô hình đang dùng**. Đó là lựa chọn ít tệ nhất: đặt
/// một tên bịa ra thì lượt sau hỏng chắc chắn, còn giữ tên cũ thì vẫn có cơ hội đúng —
/// nhiều máy chủ tự vận hành nhận bất cứ tên nào cũng trả về mô hình duy nhất nó đang nạp.
fn model_for(provider: &StoredProvider) -> Option<String> {
    provider.model.clone().or_else(|| {
        presets::matching(&provider.config.base_url)
            .and_then(|preset| preset.default_model)
            .map(str::to_string)
    })
}
