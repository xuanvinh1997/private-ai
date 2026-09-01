//! Cây plugin dựng lên đúng như danh sách nói.
//!
//! Bài này rẻ nhưng bắt được đúng loại lỗi mà kiến trúc plugin hay có: một seam không ai
//! cắm provider, hoặc hai plugin cùng đòi một seam. Cả hai đều không hiện ra lúc biên
//! dịch, và đều làm ứng dụng chết lúc mở.

use pai_app_lib::harness::{Config, boot};
use pai_tools::{ToolName, Tools};
use tempfile::TempDir;

fn config(dir: &TempDir) -> Config {
    Config {
        data_dir: dir.path().join("du-lieu"),
        workspace: dir.path().to_path_buf(),
        ollama_url: "http://127.0.0.1:11434".into(),
        model: "mo-hinh-thu".into(),
        context_window: 32_768,
    }
}

#[tokio::test]
async fn cay_plugin_dung_len_va_moi_seam_deu_co_nguoi_cam() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    let mounted: Vec<String> = harness
        .ctx
        .mounted()
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect();
    for seam in [
        "tools",
        "fs",
        "shell",
        "sandbox",
        "system-prompt",
        "tools/spill",
    ] {
        assert!(
            mounted.contains(&seam.to_string()),
            "thiếu provider cho `{seam}`: {mounted:?}"
        );
    }
}

#[tokio::test]
async fn bo_tool_co_du_moi_thu_da_cam() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");

    let names: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|schema| schema.name.as_str().to_string())
        .collect();

    for tool in [
        "read",
        "write",
        "edit",
        "glob",
        "grep",
        "bash",
        "job_output",
        "job_kill",
        "job_list",
        "todo_write",
        "symbol_search",
        "outline",
        "task",
        "terminal_open",
        "terminal_read",
    ] {
        assert!(
            names.contains(&tool.to_string()),
            "thiếu tool `{tool}`: {names:?}"
        );
    }
}

#[tokio::test]
async fn tham_so_mo_hinh_thay_khong_bao_gio_chua_dau_cham() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");

    // Tên trên dây của OpenAI không cho dấu chấm. Một tool lọt qua với dấu chấm sẽ hỏng
    // ở lần gọi đầu tiên, trên máy người dùng, chứ không ở đây.
    for schema in registry.schemas(None) {
        let wire = ToolName::from(schema.name.as_str()).wire();
        assert!(!wire.contains('.'), "tên `{wire}` còn dấu chấm");
    }
}

#[tokio::test]
async fn phien_moi_ghi_duoc_va_doc_lai_duoc() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    let created = harness
        .sessions
        .create(pai_session::NewSession::in_dir(
            dir.path().display().to_string(),
        ))
        .await
        .expect("tạo phiên");
    let id = created.id().to_string();
    drop(created);

    let listed = harness.sessions.list(Some(10)).await.expect("liệt kê");
    assert!(listed.iter().any(|header| header.id == id));
    harness.sessions.open(&id).await.expect("mở lại được");
}

#[tokio::test]
async fn lop_va_cua_nguoi_dung_tat_duoc_mot_plugin_cua_lop_nen() {
    let dir = TempDir::new().expect("thư mục tạm");
    let data = dir.path().join("du-lieu");
    std::fs::create_dir_all(&data).expect("tạo kho dữ liệu");
    // Đây là toàn bộ điểm của cấu hình theo lớp: đổi cây mà không sửa một dòng mã nào.
    std::fs::write(
        data.join("patch.yaml"),
        "patches:\n  - op: disable\n    id: shell\n",
    )
    .expect("ghi bản vá");

    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");
    let names: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|schema| schema.name.as_str().to_string())
        .collect();

    assert!(
        !names.contains(&"bash".to_string()),
        "tắt `shell` mà `bash` vẫn còn: {names:?}"
    );
    assert!(
        names.contains(&"read".to_string()),
        "chỉ `shell` bị tắt, không phải cả cây"
    );
    // Tắt chứ không xoá: hàng vẫn nhìn thấy được, nên không ai đi tìm xem nó biến đi đâu.
    assert!(harness.plugins.dump().contains("shell: shell [tắt]"));
}

#[tokio::test]
async fn ban_va_hong_thi_dung_khoi_dong_chu_khong_chay_tiep_voi_cay_mac_dinh() {
    let dir = TempDir::new().expect("thư mục tạm");
    let data = dir.path().join("du-lieu");
    std::fs::create_dir_all(&data).expect("tạo kho dữ liệu");
    std::fs::write(data.join("patch.yaml"), "patches: [ khong-phai-yaml").expect("ghi bản vá");

    let Err(err) = boot(config(&dir)).await else {
        panic!("bản vá hỏng mà vẫn khởi động được");
    };
    // Chạy tiếp với cây mặc định trông y hệt chạy đúng, và người dùng sẽ đi tìm cả buổi
    // xem vì sao bản vá của họ không có tác dụng.
    assert!(err.to_string().contains("patch.yaml"), "{err}");
}

#[tokio::test]
async fn khong_phoi_gi_ra_ngoai_khi_chua_ai_bat() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    // Mở một cổng, kể cả loopback, là một hành động hướng ra ngoài. Nó phải là thứ người
    // dùng bật, không phải thứ họ phát hiện ra là đang chạy — nên tệp token cũng không
    // được sinh ra chừng nào chưa ai bật.
    let token = pai_mcp::token_path(&dir.path().join("du-lieu"));
    assert!(
        !token.exists(),
        "sinh mcp-token trong khi chưa ai bật phơi ra ngoài"
    );
    assert!(harness.plugins.dump().contains("mcp: mcp"));
}

#[tokio::test]
async fn hook_tu_ban_va_chan_duoc_mot_tool() {
    let dir = TempDir::new().expect("thư mục tạm");
    let data = dir.path().join("du-lieu");
    std::fs::create_dir_all(&data).expect("tạo kho dữ liệu");
    // Toàn bộ chính sách này là bốn dòng YAML và một lệnh shell — không có mã nào được
    // biên dịch lại, và không có tính năng nào của ứng dụng biết nó tồn tại.
    std::fs::write(
        data.join("patch.yaml"),
        r#"patches:
  - op: replace
    id: hooks
    config:
      hooks:
        - command: "echo '{\"decision\":\"deny\",\"reason\":\"máy này không chạy lệnh\"}'"
          tools: ["bash"]
"#,
    )
    .expect("ghi bản vá");

    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");
    let pipeline = pai_tools::ToolPipeline::new(&harness.ctx, registry);

    let outcome = pipeline
        .execute("c1", "bash", serde_json::json!({ "command": "ls" }))
        .await;
    assert!(outcome.is_error, "hook nói không mà tool vẫn chạy");
    assert!(
        outcome.content.contains("máy này không chạy lệnh"),
        "{}",
        outcome.content
    );

    // Và chỉ `bash` bị chặn: hook khai `tools: [bash]`.
    let read = pipeline
        .execute(
            "c2",
            "read",
            serde_json::json!({ "file_path": "/khong-co" }),
        )
        .await;
    assert!(!read.content.contains("máy này không chạy lệnh"));
}

#[tokio::test]
async fn meta_that_cua_tool_khop_dung_hop_dong_giao_dien() {
    let dir = TempDir::new().expect("thư mục tạm");
    let workspace = dir.path().canonicalize().expect("phân giải");
    std::fs::write(workspace.join("a.rs"), "fn chao() {}\nfn tam_biet() {}\n").expect("ghi tệp");

    let harness = boot(Config {
        workspace: workspace.clone(),
        ..config(&dir)
    })
    .await
    .expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");
    let pipeline = pai_tools::ToolPipeline::new(&harness.ctx, registry);

    // Hai hợp đồng này khớp nhau **bằng tay**: `pai-fs` dựng JSON, `protocol::ToolMeta`
    // đọc nó, và `ui/src/lib/protocol.ts` đọc lại lần nữa. Không có bài này thì một lần
    // đổi tên trường ở một đầu chỉ hiện ra dưới dạng "thẻ diff trống", trên máy người dùng.
    for (tool, args) in [
        (
            "read",
            serde_json::json!({ "file_path": workspace.join("a.rs") }),
        ),
        ("grep", serde_json::json!({ "pattern": "fn " })),
        ("glob", serde_json::json!({ "pattern": "*.rs" })),
        (
            "write",
            serde_json::json!({ "file_path": workspace.join("b.rs"), "content": "fn moi() {}\n" }),
        ),
    ] {
        let outcome = pipeline.execute("c1", tool, args).await;
        assert!(!outcome.is_error, "`{tool}` hỏng: {}", outcome.content);
        assert!(!outcome.meta.is_empty(), "`{tool}` không phát meta nào");

        let parsed: pai_app_lib::protocol::ToolMeta =
            serde_json::from_value(serde_json::Value::Object(outcome.meta.clone()))
                .unwrap_or_else(|err| panic!("meta của `{tool}` không khớp hợp đồng: {err}"));
        let round_trip = serde_json::to_value(&parsed).expect("serialize lại được");
        assert!(
            round_trip.as_object().is_some_and(|map| !map.is_empty()),
            "meta của `{tool}` rỗng sau khi đi qua hợp đồng"
        );
    }
}

#[tokio::test]
async fn thao_cay_khong_hoang_loan_va_goi_hai_lan_khong_sao() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    // Gọi hai lần vì đường thoát của Tauri không đảm bảo gọi đúng một lần, và một
    // `dispose` thứ hai làm hỏng thứ gì đó là loại lỗi chỉ xuất hiện lúc tắt máy.
    harness.shutdown().await;
    harness.shutdown().await;

    // Sau khi tháo, seam phải trống — nếu còn thì có provider nào đó đã bị rò.
    assert!(
        harness.ctx.require::<Tools>().is_err(),
        "sổ đăng ký sống sót qua lần tháo"
    );
}

#[tokio::test]
async fn ban_ghi_cua_phien_cu_nap_lai_duoc_va_giu_dung_gio() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    let mut session = harness
        .sessions
        .create(pai_session::NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");
    let id = session.id().to_string();
    session
        .append_surface(pai_session::SessionEvent::UserMessage(
            pai_session::Message::user("sửa bộ nạp cấu hình"),
        ))
        .await
        .expect("ghi được");
    session.flush().await.expect("đẩy xuống đĩa");
    drop(session);

    // Không có lệnh này thì bấm vào một phiên cũ chỉ ra màn hình trống — danh sách phiên
    // trông có việc nhưng không dẫn tới đâu.
    let nodes = pai_app_lib::load_session_for_test(&harness, &id)
        .await
        .expect("nạp lại được");
    assert_eq!(nodes.len(), 1);
    let rendered = serde_json::to_value(&nodes[0]).expect("serialize được");
    assert_eq!(rendered["kind"], "user");
    assert_eq!(rendered["text"], "sửa bộ nạp cấu hình");
    // Giờ lấy từ sổ, không phải lúc node hiện lên màn hình.
    assert!(rendered["created_at"].as_i64().unwrap_or_default() > 0);
}

#[tokio::test]
async fn xoa_phien_thi_no_bien_khoi_danh_sach() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let session = harness
        .sessions
        .create(pai_session::NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");
    let id = session.id().to_string();
    drop(session);

    harness
        .sessions
        .rename(&id, "đặt tên rồi")
        .await
        .expect("đổi tên được");
    let listed = harness.sessions.list(Some(10)).await.expect("liệt kê");
    assert!(
        listed
            .iter()
            .any(|h| h.title.as_deref() == Some("đặt tên rồi"))
    );

    harness.sessions.delete(&id).await.expect("xoá được");
    let after = harness.sessions.list(Some(10)).await.expect("liệt kê");
    assert!(!after.iter().any(|h| h.id == id));
}

#[tokio::test]
async fn doi_du_an_cam_lai_tang_plugin_voi_duong_dan_moi() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");
    let pipeline = pai_tools::ToolPipeline::new(&harness.ctx, registry.clone());

    // Dự án thứ hai, với một tệp chỉ nó có.
    let other = TempDir::new().expect("thư mục tạm");
    let other_root = other.path().canonicalize().expect("phân giải");
    std::fs::write(other_root.join("rieng.txt"), "chỉ dự án hai có").expect("ghi tệp");

    // Trước khi đổi: tệp đó nằm ngoài mọi gốc được cấp quyền.
    let before = pipeline
        .execute(
            "c1",
            "read",
            serde_json::json!({ "file_path": other_root.join("rieng.txt") }),
        )
        .await;
    assert!(before.is_error, "đọc được tệp ngoài dự án trước khi đổi");

    harness
        .open_project(&other_root)
        .await
        .expect("đổi được dự án");

    // Sau khi đổi: cùng lời gọi, kết quả khác. Không có bước "cập nhật gốc của fs" nào ở
    // giữa — chỉ có tháo rồi cắm lại.
    let after = pipeline
        .execute(
            "c2",
            "read",
            serde_json::json!({ "file_path": other_root.join("rieng.txt") }),
        )
        .await;
    assert!(
        !after.is_error,
        "không đọc được sau khi đổi dự án: {}",
        after.content
    );
    assert!(after.content.contains("chỉ dự án hai có"));

    // Và tool vẫn đúng bộ: cắm lại không được nhân đôi hay bỏ sót cái nào.
    let names: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|s| s.name.as_str().to_string())
        .collect();
    let reads = names.iter().filter(|name| name.as_str() == "read").count();
    assert_eq!(reads, 1, "cắm lại nhân đôi tool: {names:?}");
    assert!(names.contains(&"bash".to_string()) && names.contains(&"symbol_search".to_string()));
}

#[tokio::test]
async fn hai_loi_vao_cung_thu_muc_van_la_mot_du_an() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let root = dir.path().canonicalize().expect("phân giải");
    std::fs::create_dir_all(root.join("con")).expect("tạo thư mục con");

    harness
        .open_project(&root.join("con").join(".."))
        .await
        .expect("mở được");
    assert_eq!(harness.projects().expect("liệt kê").len(), 1);
}

#[tokio::test]
async fn khong_bo_duoc_du_an_dang_mo() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let current = harness.current_project();
    // Bỏ dự án đang mở khỏi danh sách sẽ để ứng dụng trỏ vào một chỗ không còn ai nhắc tới.
    assert!(harness.forget_project(&current.id).is_err());
}

#[tokio::test]
async fn danh_sach_phien_chi_co_phien_cua_du_an_dang_mo() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let first = harness.workspace().display().to_string();

    harness
        .sessions
        .create(pai_session::NewSession::in_dir(first.clone()))
        .await
        .expect("tạo phiên");

    let other = TempDir::new().expect("thư mục tạm");
    let other_root = other.path().canonicalize().expect("phân giải");
    harness
        .open_project(&other_root)
        .await
        .expect("đổi được dự án");
    harness
        .sessions
        .create(pai_session::NewSession::in_dir(
            other_root.display().to_string(),
        ))
        .await
        .expect("tạo phiên");

    // Kho vẫn giữ cả hai; việc lọc là của lệnh, và nó lọc theo dự án đang mở.
    assert_eq!(
        harness
            .sessions
            .list(Some(10))
            .await
            .expect("liệt kê")
            .len(),
        2
    );
    let mine: Vec<_> = harness
        .sessions
        .list(Some(10))
        .await
        .expect("liệt kê")
        .into_iter()
        .filter(|h| h.cwd.as_deref() == Some(other_root.display().to_string().as_str()))
        .collect();
    assert_eq!(mine.len(), 1);
}
