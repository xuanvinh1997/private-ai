//! Danh mục dựng sẵn và phép thử một cấu hình.
//!
//! Không bài nào gọi ra Internet: bài thử dùng một cổng loopback vừa đóng, còn bài danh
//! mục chỉ **dựng** adapter chứ không nói chuyện với nó.

use pai_llm::{AdapterRegistry, ProviderConfig, ProviderKind};
use pai_providers::{PRESETS, probe};

#[test]
fn moi_muc_dung_duoc_thanh_adapter() {
    let registry = AdapterRegistry::new(reqwest::Client::new());
    for preset in PRESETS {
        let config = preset.config();
        assert!(
            !config.base_url.is_empty(),
            "{} thiếu địa chỉ máy chủ",
            preset.name
        );
        registry
            .adapter(&config)
            .unwrap_or_else(|err| panic!("{} không dựng được adapter: {err}", preset.name));
        assert!(!preset.hint.is_empty(), "{} thiếu lời nhắc", preset.name);
        // `on_device` của mục phải khớp với cái mà tầng mô hình tự suy ra từ URL. Hai
        // nguồn sự thật cho cùng một câu "dữ liệu có rời máy này không" là chỗ giao diện
        // nói một đằng còn thực tế một nẻo.
        assert_eq!(
            preset.on_device,
            config.on_device(),
            "{} nói on_device={} nhưng URL nói ngược lại",
            preset.name,
            preset.on_device
        );
    }
}

#[test]
fn danh_muc_co_du_cac_muc_bat_buoc() {
    for can in [
        "openai",
        "anthropic",
        "ollama",
        "lmstudio",
        "llamacpp",
        "vllm",
        "openrouter",
        "deepseek",
        "groq",
        "xai",
    ] {
        assert!(
            PRESETS.iter().any(|preset| preset.id == can),
            "thiếu mục `{can}`"
        );
    }
}

/// Một cổng loopback vừa được nhả ra: chắc chắn không có ai nghe, và chắc chắn không ra
/// khỏi máy này.
fn cong_dong() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("mượn cổng");
    let port = listener.local_addr().expect("địa chỉ").port();
    drop(listener);
    port
}

#[tokio::test]
async fn khong_noi_duoc_thi_dung_do_cho_cai_khoa() {
    let port = cong_dong();
    let config = ProviderConfig::new(
        "thu",
        "Máy chủ chưa bật",
        ProviderKind::OpenAiCompatible,
        format!("http://127.0.0.1:{port}/v1"),
    )
    .with_api_key("khoa-hoan-toan-hop-le");

    let result = probe(&config, &reqwest::Client::new()).await;
    assert!(!result.ok, "không được coi là nối được: {result:?}");
    assert!(
        result.message.contains("Không kết nối được"),
        "phải thuộc nhóm không nối được: {}",
        result.message
    );
    // Nhóm sai khoá là một hành động khác hẳn của người dùng. Nhắc tới khoá ở đây là đẩy
    // họ đi sửa nhầm chỗ.
    assert!(
        !result.message.to_lowercase().contains("khoá"),
        "không được đổ cho khoá: {}",
        result.message
    );
    assert!(result.models.is_empty());
}

#[tokio::test]
async fn ollama_chua_bat_cung_thuoc_nhom_khong_noi_duoc() {
    let port = cong_dong();
    let config = ProviderConfig::new(
        "thu",
        "Ollama chưa bật",
        ProviderKind::Ollama,
        format!("http://127.0.0.1:{port}"),
    );

    let result = probe(&config, &reqwest::Client::new()).await;
    assert!(!result.ok);
    assert!(
        result.message.contains("Không kết nối được"),
        "{}",
        result.message
    );
    assert!(result.message.contains("/api/tags"), "{}", result.message);
}
