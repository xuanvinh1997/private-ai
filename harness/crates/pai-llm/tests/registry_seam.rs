//! Registry provider và khai báo seam.
//!
//! Hai thứ nhỏ nhưng hay hỏng im lặng: cache adapter đánh khoá **theo chữ ký**, và seam
//! cắm được vào cây plugin của `pai-core`.

use std::sync::Arc;

use futures::stream::BoxStream;
use pai_core::{Context, ServiceKey};
use pai_llm::error::LlmError;
use pai_llm::message::ChatRequest;
use pai_llm::registry::{
    AdapterRegistry, ProviderConfig, ProviderKind, active_config, no_provider,
};
use pai_llm::seam::{Llm, LlmAdapter};
use pai_llm::stream::StreamChunk;
use pai_llm::{Capabilities, LlmErrorCode, openai_base_url};

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .build()
        .expect("client dựng được")
}

fn ollama(id: &str, url: &str) -> ProviderConfig {
    ProviderConfig::new(id, "Ollama cục bộ", ProviderKind::Ollama, url)
}

/// Cache khoá theo chữ ký, không theo id.
///
/// Người dùng sửa URL mà giữ nguyên id là trường hợp thật: nếu cache khoá theo id thì mọi
/// request tiếp theo vẫn bay tới máy chủ cũ và **không có gì báo động**.
#[test]
fn sua_url_thi_adapter_cu_bi_vut() {
    let registry = AdapterRegistry::new(client());
    let first = registry
        .adapter(&ollama("local", "http://127.0.0.1:11434"))
        .expect("dựng được");
    let again = registry
        .adapter(&ollama("local", "http://127.0.0.1:11434"))
        .expect("dựng được");
    assert!(Arc::ptr_eq(&first, &again), "cùng chữ ký thì dùng lại");
    assert_eq!(registry.cached(), 1);

    let moved = registry
        .adapter(&ollama("local", "http://192.168.1.9:11434"))
        .expect("dựng được");
    assert!(!Arc::ptr_eq(&first, &moved), "đổi URL là đổi adapter");
    assert_eq!(registry.cached(), 1, "bản cũ của cùng provider bị vứt");
}

/// Khoá API cũng nằm trong chữ ký: dán một khoá mới vào là phải có client mới.
#[test]
fn doi_khoa_api_cung_lam_moi_adapter() {
    let registry = AdapterRegistry::new(client());
    let base = ProviderConfig::new(
        "cloud",
        "Cloud",
        ProviderKind::OpenAiCompatible,
        "https://x.test",
    );
    let first = registry
        .adapter(&base.clone().with_api_key("cu"))
        .expect("dựng được");
    let second = registry
        .adapter(&base.with_api_key("moi"))
        .expect("dựng được");
    assert!(!Arc::ptr_eq(&first, &second));
}

#[test]
fn provider_khac_nhau_khong_dap_len_nhau() {
    let registry = AdapterRegistry::new(client());
    registry
        .adapter(&ollama("a", "http://127.0.0.1:11434"))
        .expect("dựng được");
    registry
        .adapter(&ProviderConfig::new(
            "b",
            "Cloud",
            ProviderKind::OpenAiCompatible,
            "https://x.test",
        ))
        .expect("dựng được");
    assert_eq!(registry.cached(), 2);
}

/// Provider từ xa trả lời "không áp dụng" bằng câu chữ, không bằng một `None` câm lặng.
#[test]
fn provider_tu_xa_la_chi_doc() {
    let registry = AdapterRegistry::new(client());
    let config = ProviderConfig::new(
        "cloud",
        "OpenAI",
        ProviderKind::OpenAiCompatible,
        "https://x.test",
    );
    let Err(err) = registry.admin(&config) else {
        panic!("phải từ chối")
    };
    assert_eq!(err.code, LlmErrorCode::ProviderReadOnly);
    assert!(err.message.contains("OpenAI"));
    assert!(
        registry
            .admin(&ollama("local", "http://127.0.0.1:11434"))
            .is_ok()
    );
}

/// Ba tầng dự phòng của `active_config`, giữ nguyên từ bản Python.
#[test]
fn provider_dang_chon_co_ba_tang_du_phong() {
    let mut tat = ollama("tat", "http://a");
    tat.enabled = false;
    let bat = ollama("bat", "http://b");
    let khac = ollama("khac", "http://c");

    let configs = vec![tat.clone(), bat.clone(), khac.clone()];
    assert_eq!(
        active_config(&configs, "khac").map(|c| c.id.as_str()),
        Some("khac")
    );
    // Cái được ghim đang tắt: rơi xuống cái đầu tiên còn bật.
    assert_eq!(
        active_config(&configs, "tat").map(|c| c.id.as_str()),
        Some("bat")
    );
    // Tất cả đều tắt: vẫn trả cái đầu tiên, vì "chưa cấu hình gì" là một câu sai.
    let all_off = vec![tat.clone()];
    assert_eq!(
        active_config(&all_off, "").map(|c| c.id.as_str()),
        Some("tat")
    );
    assert!(active_config(&[], "bat").is_none());
    assert_eq!(no_provider().code, LlmErrorCode::NoProviderConfigured);
}

#[test]
fn nhan_ra_provider_chay_tren_may_nay() {
    assert!(ollama("a", "http://localhost:11434").on_device());
    assert!(ollama("a", "http://127.0.0.1:11434").on_device());
    assert!(ollama("a", "http://[::1]:11434").on_device());
    assert!(!ollama("a", "https://api.openai.com").on_device());
    assert!(!ollama("a", "http://192.168.1.9:11434").on_device());
}

#[test]
fn base_url_chap_nhan_ca_goc_api_lan_host_tran() {
    assert_eq!(
        openai_base_url("http://localhost:8080").as_deref(),
        Ok("http://localhost:8080/v1")
    );
    assert_eq!(
        openai_base_url("http://localhost:8080/v1/").as_deref(),
        Ok("http://localhost:8080/v1")
    );
    assert_eq!(
        openai_base_url("https://x.test/v2").as_deref(),
        Ok("https://x.test/v2")
    );
    // `vllm` bắt đầu bằng `v` nhưng phần sau không phải số — không được nhầm là phiên bản.
    assert_eq!(
        openai_base_url("https://x.test/vllm").as_deref(),
        Ok("https://x.test/vllm/v1")
    );
    assert!(openai_base_url("   ").is_err());
}

/// Chữ ký không được in khoá API ra log.
#[test]
fn chu_ky_khong_ro_ri_khoa() {
    let config = ProviderConfig::new(
        "cloud",
        "Cloud",
        ProviderKind::OpenAiCompatible,
        "https://x.test",
    )
    .with_api_key("sk-that-su-bi-mat");
    let printed = format!("{:?}", config.signature());
    assert!(!printed.contains("sk-that-su-bi-mat"), "{printed}");
    assert!(printed.contains("<đã đặt>"));
}

// --- seam ----------------------------------------------------------------------------

struct Cam;

#[async_trait::async_trait]
impl LlmAdapter for Cam {
    fn id(&self) -> &str {
        "cam"
    }

    fn stream(&self, _req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        futures::stream::empty().boxed()
    }

    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError> {
        Ok(Capabilities::infer(model))
    }
}

use futures::StreamExt;

/// Seam cắm vào cây plugin và lấy ra được qua marker type, không qua chuỗi.
#[tokio::test]
async fn seam_cam_duoc_vao_cay_plugin() {
    let ctx = Context::root();
    assert!(ctx.get::<Llm>().is_none());

    let guard = ctx.provide::<Llm>(Arc::new(Cam)).expect("chưa ai cắm");
    let adapter = ctx.require::<Llm>().expect("đã có");
    assert_eq!(adapter.id(), "cam");
    assert_eq!(Llm::NAME, "llm");
    assert!(
        adapter
            .capabilities("llava:7b")
            .await
            .expect("đoán được")
            .vision
    );

    // Đăng ký là hiệu ứng gỡ lại được: thả guard là seam trống trở lại.
    drop(guard);
    assert!(ctx.get::<Llm>().is_none());
}
