//! Provider registry and seam declaration.
//! Two small things that fail silently: the adapter cache keyed by signature, and the
//! seam plugging into `pai-core`'s plugin tree.

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

/// The cache is keyed by signature, not id: editing a URL while keeping the id must not keep routing to the old server.
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

/// The API key is part of the signature too: pasting a new key must produce a new client.
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

/// A remote provider answers "not applicable" in words, not with a silent `None`.
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

/// The three `active_config` fallbacks, kept from the Python version.
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
    // The pinned one is disabled: fall through to the first enabled one.
    assert_eq!(
        active_config(&configs, "tat").map(|c| c.id.as_str()),
        Some("bat")
    );
    // All disabled: still return the first, because "nothing configured" would be wrong.
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
    // `vllm` starts with `v` but the rest is not a number - it must not be read as a version.
    assert_eq!(
        openai_base_url("https://x.test/vllm").as_deref(),
        Ok("https://x.test/vllm/v1")
    );
    assert!(openai_base_url("   ").is_err());
}

/// The signature must never print the API key into a log.
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

/// The seam plugs into the plugin tree and is retrieved by marker type, not by string.
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

    // Registration is a reversible effect: dropping the guard empties the seam again.
    drop(guard);
    assert!(ctx.get::<Llm>().is_none());
}
