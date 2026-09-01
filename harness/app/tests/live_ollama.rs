//! Một lượt thật, với một mô hình thật.
//!
//! Bài này **cần một máy chủ Ollama đang chạy**, nên nó mang `#[ignore]`: một bộ test đỏ
//! vì môi trường là một bộ test không ai còn tin, và người ta sẽ bỏ qua cả những bài đỏ
//! thật. Chạy nó có chủ ý:
//!
//! ```text
//! PAI_MODEL=qwen3.8:27b-mlx cargo test -p pai-app --test live_ollama -- --ignored --nocapture
//! ```
//!
//! Nó kiểm đúng thứ mà mọi bài khác không kiểm được: rằng schema tool ta phát ra thật sự
//! khiến một mô hình gọi tool, rằng tham số nó sinh ra parse được, và rằng kết quả quay
//! lại được vào lượt sau. Ba thứ đó chỉ hỏng khi gặp một mô hình thật.

use std::sync::Arc;

use pai_agent::TurnSink;
use pai_app_lib::harness::{Config, boot};
use parking_lot::Mutex;
use tempfile::TempDir;

/// Ghi lại những gì vòng lặp kể ra, để bài kiểm chứng đọc lại.
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

#[tokio::test]
#[ignore = "cần một máy chủ Ollama đang chạy"]
async fn mo_hinh_that_goi_duoc_tool_va_doc_duoc_ket_qua() {
    let dir = TempDir::new().expect("thư mục tạm");
    let workspace = dir.path().canonicalize().expect("phân giải");
    std::fs::write(
        workspace.join("bi-mat.txt"),
        "Mật khẩu của kho là: ca-heo-mau-tim\n",
    )
    .expect("ghi tệp");

    let config = Config {
        data_dir: workspace.join("du-lieu"),
        workspace: workspace.clone(),
        ..Config::from_env()
    };
    let model = config.model.clone();
    let harness = boot(config).await.expect("dựng được cây");

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
        "mô hình: {model}\nlý do dừng: {reason:?}\ntool đã gọi: {tools:?}\ntrả lời:\n{answer}"
    );

    assert!(
        tools.iter().any(|call| call.starts_with("read")),
        "mô hình không gọi `read`; schema ta phát ra chưa đủ để nó hiểu: {tools:?}"
    );
    assert!(
        answer.contains("ca-heo-mau-tim"),
        "kết quả tool không quay lại được vào lượt sau; mô hình trả lời:\n{answer}"
    );

    // Và mọi thứ mô hình thấy đều nằm trong sổ — đó là bất biến trung tâm.
    let history = session.derive_messages();
    assert!(history.iter().any(|m| m.role == pai_session::Role::Tool));
}
