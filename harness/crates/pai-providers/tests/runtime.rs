//! Đường đổi provider, chạy từ đầu đến cuối.
//!
//! Không có mạng: adapter được **dựng** chứ không được gọi, và cái bài này hỏi là "sau khi
//! ghim một provider khác thì `Driver` có thực sự cầm adapter mới không" — đúng cái bước
//! mà một đường đổi provider viết rời rạc hay quên.

use std::sync::Arc;

use pai_agent::{Driver, SystemPrompt};
use pai_core::Context;
use pai_llm::{AdapterRegistry, ProviderKind};
use pai_providers::{ProviderInput, ProviderRuntime, ProviderStore, Role, SqliteProviderStore};
use pai_tools::{ToolPipeline, ToolRegistry};

fn runtime() -> (ProviderRuntime, Arc<Driver>, Arc<SqliteProviderStore>) {
    let ctx = Context::root();
    let tools = ToolRegistry::new(&ctx);
    let pipeline = Arc::new(ToolPipeline::new(&ctx, tools));
    let http = reqwest::Client::new();
    let registry = Arc::new(AdapterRegistry::new(http.clone()));
    let store = Arc::new(SqliteProviderStore::open_in_memory().expect("mở kho"));
    let driver = Arc::new(Driver::new(
        ctx,
        registry
            .adapter(&pai_providers::PRESETS[0].config())
            .expect("adapter khởi điểm"),
        pipeline,
        SystemPrompt::new(),
        "chua-chon",
    ));
    let runtime = ProviderRuntime::new(store.clone(), registry, driver.clone(), http);
    (runtime, driver, store)
}

#[tokio::test]
async fn ghim_mot_provider_thi_driver_cam_adapter_cua_no() {
    let (runtime, driver, _store) = runtime();

    let ollama = runtime
        .save(ProviderInput::create(
            "Ollama nhà",
            ProviderKind::Ollama,
            "http://localhost:11434",
        ))
        .await
        .expect("lưu Ollama");
    let openai = runtime
        .save(
            ProviderInput::create(
                "OpenAI",
                ProviderKind::OpenAiCompatible,
                "https://api.openai.com/v1",
            )
            .with_api_key("sk-thu")
            .with_model("gpt-4o-mini"),
        )
        .await
        .expect("lưu OpenAI");

    // Cái đầu tiên được ghim ngay lúc lưu, và mô hình rơi về mặc định của mục danh mục
    // cùng địa chỉ vì người dùng chưa chọn gì.
    assert_eq!(driver.llm().id(), ollama.id());
    assert_eq!(driver.model(), "qwen3:8b");

    runtime
        .activate(openai.id(), None)
        .await
        .expect("ghim OpenAI");
    assert_eq!(driver.llm().id(), openai.id());
    assert_eq!(driver.model(), "gpt-4o-mini");

    // Xoá cái đang hoạt động: người kế nhiệm phải đã nằm trong `Driver` khi lời gọi trả về.
    runtime.remove(openai.id()).await.expect("xoá OpenAI");
    assert_eq!(driver.llm().id(), ollama.id());
    assert_eq!(
        runtime
            .active()
            .expect("đọc")
            .map(|row| row.id().to_string()),
        Some(ollama.id().to_string())
    );
}

#[tokio::test]
async fn sua_dia_chi_thi_adapter_cu_bi_vut_di() {
    let (runtime, driver, store) = runtime();

    let saved = runtime
        .save(ProviderInput::create(
            "Máy chủ nhà",
            ProviderKind::OpenAiCompatible,
            "http://localhost:8080/v1",
        ))
        .await
        .expect("lưu");
    let dau_tien = driver.llm();

    // Cùng id, khác URL: cache của `pai-llm` đánh khoá theo chữ ký chứ không theo id, nên
    // adapter phải là một cái khác — nếu không thì mọi request tiếp theo vẫn bay tới máy
    // chủ cũ mà không có gì báo động.
    runtime
        .save(
            ProviderInput::create(
                "Máy chủ nhà",
                ProviderKind::OpenAiCompatible,
                "http://localhost:9090/v1",
            )
            .with_id(saved.id()),
        )
        .await
        .expect("đổi địa chỉ");

    assert!(!Arc::ptr_eq(&dau_tien, &driver.llm()));
    assert_eq!(
        store
            .active(Role::Chat)
            .expect("đọc")
            .expect("còn một cái")
            .config
            .base_url,
        "http://localhost:9090/v1"
    );
}
