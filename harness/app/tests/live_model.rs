//! A real turn, with a real model, on a real server. It needs a running model server, so it is `#[ignore]`d:
//! a suite that goes red for environmental reasons is a suite nobody trusts. Run it deliberately:
//!
//! ```text
//! # Ollama (mặc định)
//! PAI_LIVE_MODEL=qwen3:8b cargo test -p pai-app --test live_model -- --ignored --nocapture
//!
//! # LM Studio
//! PAI_LIVE_KIND=lmstudio PAI_LIVE_MODEL=qwen3-8b \
//!   cargo test -p pai-app --test live_model -- --ignored --nocapture
//!
//! # Bất cứ máy chủ OpenAI-compatible nào — kể cả cổng `/v1` của chính Ollama
//! PAI_LIVE_KIND=openai PAI_LIVE_BASE_URL=http://127.0.0.1:11434/v1 PAI_LIVE_MODEL=qwen3:8b \
//!   cargo test -p pai-app --test live_model -- --ignored --nocapture
//! ```
//!
//! It checks what nothing else can: that our tool schemas actually make a model call tools, that the arguments
//! parse, and that results feed back into the next step. All three provider kinds are covered because they
//! differ exactly where breakage happens -- whole tool calls versus streamed `arguments` fragments.

use std::sync::Arc;

use pai_agent::TurnSink;
use pai_app_lib::harness::{Config, boot};
use pai_llm::ProviderKind;
use pai_providers::ProviderInput;
use parking_lot::Mutex;
use tempfile::TempDir;

/// The server this run targets, read from the environment.
struct Duoi {
    kind: ProviderKind,
    base_url: String,
    model: String,
    api_key: String,
}

impl Duoi {
    fn from_env() -> Duoi {
        let kind = std::env::var("PAI_LIVE_KIND")
            .ok()
            .and_then(|value| ProviderKind::parse(&value))
            .unwrap_or(ProviderKind::Ollama);
        // Default to each server's stock address: whoever runs this just started it with its default command.
        let base_url = std::env::var("PAI_LIVE_BASE_URL").unwrap_or_else(|_| {
            match kind {
                ProviderKind::Ollama => "http://127.0.0.1:11434",
                ProviderKind::LmStudio => "http://127.0.0.1:1234",
                ProviderKind::OpenAiCompatible => "http://127.0.0.1:8080/v1",
            }
            .to_string()
        });
        Duoi {
            kind,
            base_url,
            // No default model name anywhere: a guessed name yields a 404 the test would blame on the tool schema.
            model: std::env::var("PAI_LIVE_MODEL")
                .expect("đặt PAI_LIVE_MODEL thành tên mô hình đang có trên máy chủ"),
            api_key: std::env::var("PAI_LIVE_API_KEY").unwrap_or_default(),
        }
    }

    fn ten(&self) -> String {
        format!("{} ({})", self.model, self.kind.as_str())
    }
}

/// Records what the loop reports, for the test to read back.
#[derive(Default)]
struct Recorder {
    tools: Mutex<Vec<String>>,
    text: Mutex<String>,
}

impl TurnSink for Recorder {
    fn token(&self, text: &str) {
        self.text.lock().push_str(text);
    }
    fn tool_start(&self, _call_id: &str, name: &str, arguments: &str) {
        self.tools.lock().push(format!("{name} {arguments}"));
    }
}

/// Build the tree, then point it at the server the environment names, going through the provider store rather
/// than injecting an adapter, because that is the path the UI takes. The trailing `apply_provider` is not
/// redundant: without it the shared `ActiveLlm` keeps pointing at the startup server, silently.
async fn dung_cay(dir: &TempDir, duoi: &Duoi) -> (Arc<pai_app_lib::harness::Harness>, std::path::PathBuf) {
    let workspace = dir.path().canonicalize().expect("phân giải");
    let config = Config {
        data_dir: workspace.join("du-lieu"),
        workspace: Some(workspace.clone()),
        ..Config::from_env()
    };
    let harness = Arc::new(boot(config).await.expect("dựng được cây"));

    let saved = harness
        .providers
        .save(
            ProviderInput::create("máy chủ của bài test", duoi.kind, duoi.base_url.clone())
                .with_model(duoi.model.clone())
                .with_api_key(duoi.api_key.clone()),
        )
        .await
        .expect("lưu được hàng provider");
    harness
        .providers
        .activate(saved.id(), Some(&duoi.model))
        .await
        .expect("trao được vai hội thoại");
    harness
        .apply_provider()
        .await
        .expect("đẩy được provider ra mọi chỗ cầm con trỏ chia sẻ");

    (harness, workspace)
}

#[tokio::test]
#[ignore = "cần một máy chủ mô hình đang chạy"]
async fn mo_hinh_that_goi_duoc_tool_va_doc_duoc_ket_qua() {
    let duoi = Duoi::from_env();
    let dir = TempDir::new().expect("thư mục tạm");
    let (harness, workspace) = dung_cay(&dir, &duoi).await;
    std::fs::write(
        workspace.join("bi-mat.txt"),
        "Mật khẩu của kho là: ca-heo-mau-tim\n",
    )
    .expect("ghi tệp");

    let mut session = harness
        .sessions
        .create(pai_session::NewSession::in_dir(
            workspace.display().to_string(),
        ))
        .await
        .expect("tạo phiên");

    let recorder = Arc::new(Recorder::default());
    let reason = harness
        .driver
        .run_turn(
            &mut session,
            1,
            vec![pai_session::Message::user(format!(
                "Đọc tệp {}/bi-mat.txt rồi cho tôi biết mật khẩu của kho là gì. \
                 Dùng công cụ, đừng đoán.",
                workspace.display()
            ))],
            tokio_util::sync::CancellationToken::new(),
            recorder.as_ref(),
        )
        .await
        .expect("lượt chạy được");

    let tools = recorder.tools.lock().clone();
    let answer = recorder.text.lock().clone();
    eprintln!(
        "mô hình: {}\nlý do dừng: {reason:?}\ntool đã gọi: {tools:?}\ntrả lời:\n{answer}",
        duoi.ten()
    );

    assert!(
        tools.iter().any(|call| call.starts_with("read")),
        "mô hình không gọi `read`; schema ta phát ra chưa đủ để nó hiểu: {tools:?}"
    );
    assert!(
        answer.contains("ca-heo-mau-tim"),
        "kết quả tool không quay lại được vào lượt sau; mô hình trả lời:\n{answer}"
    );

    // And everything the model saw is in the log -- the central invariant.
    let history = session.derive_messages();
    assert!(history.iter().any(|m| m.role == pai_session::Role::Tool));
}

/// A local server's model catalogue, read for real; separate because it fails differently: no model runs here,
/// only an admin endpoint's JSON shape, which changes silently between someone else's releases.
#[tokio::test]
#[ignore = "cần một máy chủ mô hình đang chạy"]
async fn may_chu_tai_cho_khai_duoc_kho_mo_hinh() {
    let duoi = Duoi::from_env();
    if duoi.kind == ProviderKind::OpenAiCompatible {
        // The OpenAI protocol has no model lifecycle and `admin()` returns `None` by design; say so and move on.
        eprintln!("bỏ qua: {} không có nửa quản trị mô hình", duoi.ten());
        return;
    }

    let dir = TempDir::new().expect("thư mục tạm");
    let (harness, _) = dung_cay(&dir, &duoi).await;

    let models = harness.models().await;
    assert!(
        !models.is_empty(),
        "{} không khai mô hình nào; kho rỗng hay hình dạng phản hồi đã đổi?",
        duoi.ten()
    );
    let chon = models
        .iter()
        .find(|choice| choice.id == duoi.model)
        .unwrap_or_else(|| {
            panic!(
                "kho không có `{}`; máy chủ khai: {:?}",
                duoi.model,
                models.iter().map(|m| &m.id).collect::<Vec<_>>()
            )
        });
    eprintln!(
        "{}: chat={} tools={} embedding={} ngữ cảnh={:?}",
        duoi.ten(),
        chon.chat,
        chon.tools,
        chon.embedding,
        chon.context_window
    );
    assert!(
        chon.chat,
        "máy chủ không coi `{}` là mô hình trò chuyện",
        duoi.model
    );
}

/// The embedding role, really embedding a sentence on the running server; skipped without
/// `PAI_LIVE_EMBED_MODEL`, since that model is a separate download. It matters because this path fails
/// silently -- a 200 with an empty body, or a chat model picked by mistake, both yield a garbage library.
#[tokio::test]
#[ignore = "cần một máy chủ mô hình đang chạy"]
async fn may_chu_that_nhung_duoc_mot_cau() {
    let Ok(embed_model) = std::env::var("PAI_LIVE_EMBED_MODEL") else {
        eprintln!("bỏ qua: chưa đặt PAI_LIVE_EMBED_MODEL");
        return;
    };
    let duoi = Duoi::from_env();
    let dir = TempDir::new().expect("thư mục tạm");
    let (harness, _) = dung_cay(&dir, &duoi).await;

    let config = harness
        .providers
        .active()
        .expect("đọc được hàng đang hoạt động")
        .expect("có hàng đang hoạt động")
        .config;
    let result = harness
        .providers
        .probe_embedding(&config, &embed_model)
        .await;

    eprintln!("{}: {}", duoi.ten(), result.message);
    assert!(result.ok, "{}", result.message);
    assert!(
        result.dimensions.is_some_and(|dims| dims > 0),
        "nhúng xong mà không đo được số chiều: {}",
        result.message
    );
}
