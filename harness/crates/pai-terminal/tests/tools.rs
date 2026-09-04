//! The six tools: exact names, exact parameter shapes, exactly one approval gate.
//! Names and shapes are locked because they are the interface to the model, not an implementation
//! detail -- renaming `sessionId` would buy a class of wrong calls with nothing to signal it.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_terminal::TerminalPlugin;
use pai_tools::{Approval, ApprovalRequest, Approver, ToolPipeline, Tools, ToolsPlugin};
use parking_lot::Mutex;
use serde_json::{Value, json};

/// Records the prompt and answers with a canned reply, enough to tell "asked and denied" from "never asked".
struct Recorder {
    answer: bool,
    asked: Mutex<Vec<String>>,
}

#[async_trait]
impl Approver for Recorder {
    async fn approve(&self, request: &ApprovalRequest) -> bool {
        self.asked
            .lock()
            .push(format!("{}: {}", request.name, request.reason));
        self.answer
    }
}

async fn harness(approver: Option<Arc<Recorder>>) -> (Context, ToolPipeline) {
    let root = Context::root();
    ToolsPlugin
        .apply(&root.plugin("tools"))
        .await
        .expect("cắm được tools");
    TerminalPlugin::new(PathBuf::from("/tmp"))
        .apply(&root.plugin("terminal"))
        .await
        .expect("cắm được terminal");
    if let Some(approver) = approver {
        let api: Arc<dyn Approver> = approver;
        root.provide::<Approval>(api).expect("cắm được").leak();
    }
    let registry = root.require::<Tools>().expect("sổ tool có mặt");
    let pipeline = ToolPipeline::new(&root, registry);
    (root, pipeline)
}

fn schema_of(ctx: &Context, name: &str) -> Value {
    let registry = ctx.require::<Tools>().expect("sổ tool có mặt");
    let schema = registry
        .schemas(None)
        .into_iter()
        .find(|schema| schema.name.as_str() == name)
        .unwrap_or_else(|| panic!("không có tool `{name}`"));
    schema.parameters
}

fn required(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn has_property(schema: &Value, field: &str) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|props| props.contains_key(field))
}

#[tokio::test]
async fn sau_tool_giu_dung_ten_va_hinh_dang_cua_dsh() {
    let (ctx, _pipeline) = harness(None).await;

    let open = schema_of(&ctx, "terminal_open");
    assert_eq!(required(&open), vec!["type"]);
    for field in ["type", "name", "cwd"] {
        assert!(
            has_property(&open, field),
            "thiếu `{field}` ở terminal_open"
        );
    }

    assert!(required(&schema_of(&ctx, "terminal_list")).is_empty());

    let read = schema_of(&ctx, "terminal_read");
    assert_eq!(required(&read), vec!["sessionId"]);
    for field in ["sessionId", "offset", "count"] {
        assert!(
            has_property(&read, field),
            "thiếu `{field}` ở terminal_read"
        );
    }

    let send = schema_of(&ctx, "terminal_send");
    assert_eq!(required(&send), vec!["sessionId", "text"]);
    for field in ["sessionId", "text", "submit", "run_in_background"] {
        assert!(
            has_property(&send, field),
            "thiếu `{field}` ở terminal_send"
        );
    }

    let signal = schema_of(&ctx, "terminal_signal");
    assert_eq!(required(&signal), vec!["sessionId", "signal"]);
    // The signal set is a closed enum in the schema itself, not a free string validated in the tool body.
    let allowed = signal["properties"]["signal"]["enum"]
        .as_array()
        .expect("signal phải là enum trong schema");
    assert_eq!(
        allowed,
        &vec![
            json!("SIGINT"),
            json!("SIGTERM"),
            json!("SIGKILL"),
            json!("SIGTSTP"),
            json!("SIGHUP"),
        ]
    );

    assert_eq!(
        required(&schema_of(&ctx, "terminal_close")),
        vec!["sessionId"]
    );
}

#[tokio::test]
async fn tao_phien_phai_hoi_va_khong_ai_tra_loi_thi_khong_chay() {
    // No approver plugged in: fail closed, exactly like `bash`.
    let (_ctx, pipeline) = harness(None).await;
    let outcome = pipeline
        .execute("call-1", "terminal_open", json!({ "type": "shell" }))
        .await;
    assert!(outcome.is_error, "{}", outcome.content);
    assert!(
        outcome.content.contains("không cho phép"),
        "{}",
        outcome.content
    );
}

#[tokio::test]
async fn cau_hoi_noi_dung_muc_rui_ro_va_noi_rang_phien_o_lai() {
    let recorder = Arc::new(Recorder {
        answer: false,
        asked: Mutex::new(Vec::new()),
    });
    let (_ctx, pipeline) = harness(Some(recorder.clone())).await;
    let outcome = pipeline
        .execute("call-2", "terminal_open", json!({ "type": "shell" }))
        .await;
    assert!(outcome.is_error);

    let asked = recorder.asked.lock().clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    // With no sandbox provider present the prompt must say so outright; this is the line the user reads before clicking.
    assert!(asked[0].contains("Không có vòng giam nào"), "{asked:?}");
    assert!(asked[0].contains("ở lại"), "{asked:?}");
}

#[tokio::test]
async fn nam_tool_con_lai_khong_hoi_lai_sau_khi_phien_da_duoc_duyet() {
    let recorder = Arc::new(Recorder {
        answer: true,
        asked: Mutex::new(Vec::new()),
    });
    let (_ctx, pipeline) = harness(Some(recorder.clone())).await;

    let opened = pipeline
        .execute("call-3", "terminal_open", json!({ "type": "shell" }))
        .await;
    assert!(!opened.is_error, "{}", opened.content);
    let id = opened.structured.as_ref().expect("có structured")["id"]
        .as_str()
        .expect("có id")
        .to_string();

    let sent = pipeline
        .execute(
            "call-4",
            "terminal_send",
            json!({ "sessionId": id, "text": "echo xin-chao" }),
        )
        .await;
    assert!(!sent.is_error, "{}", sent.content);
    assert!(sent.content.contains("xin-chao"), "{}", sent.content);

    let closed = pipeline
        .execute("call-5", "terminal_close", json!({ "sessionId": id }))
        .await;
    assert!(!closed.is_error, "{}", closed.content);

    // Exactly one prompt for all three calls: approval buys the shell, not each keystroke into it.
    assert_eq!(recorder.asked.lock().len(), 1);
}

#[tokio::test]
async fn id_cua_chu_khac_tra_ve_dung_cau_nhu_id_khong_ton_tai() {
    let recorder = Arc::new(Recorder {
        answer: true,
        asked: Mutex::new(Vec::new()),
    });
    let (_ctx, pipeline) = harness(Some(recorder)).await;
    let bia_dat = pipeline
        .execute(
            "call-6",
            "terminal_read",
            json!({ "sessionId": "khong-co-that" }),
        )
        .await;
    assert!(bia_dat.is_error);
    assert!(
        bia_dat.content.contains("không có phiên terminal"),
        "{}",
        bia_dat.content
    );
}
