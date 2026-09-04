//! Cây plugin dựng lên đúng như danh sách nói.
//!
//! Bài này rẻ nhưng bắt được đúng loại lỗi mà kiến trúc plugin hay có: một seam không ai
//! cắm provider, hoặc hai plugin cùng đòi một seam. Cả hai đều không hiện ra lúc biên
//! dịch, và đều làm ứng dụng chết lúc mở.

use pai_app_lib::harness::{Config, boot};
use pai_session::SessionScope;
use pai_tools::{ToolName, Tools};
use tempfile::TempDir;

fn config(dir: &TempDir) -> Config {
    Config {
        data_dir: dir.path().join("du-lieu"),
        // Bộ skill dựng sẵn cố tình để trống: bài test nói về cây plugin, và một thư mục
        // skill thật trên máy chạy test sẽ làm kết quả đổi theo máy.
        builtin_skills: None,
        // Mô hình nhúng cũng vậy: bài test không nói về thư viện tài liệu, và một tên mô
        // hình thật ở đây sẽ khiến kết quả phụ thuộc vào máy chủ có đang chạy hay không.
        embed_model: None,
        workspace: Some(dir.path().to_path_buf()),
        ollama_url: "http://127.0.0.1:11434".into(),
        model: "mo-hinh-thu".into(),
        context_window: 32_768,
    }
}

/// Bỏ tiền tố đường dẫn dài của Windows, đúng như `rag_config` làm khi ghi tệp.
///
/// `TempDir::canonicalize` trả về dạng verbatim, còn cấu hình ghi ra thì cố ý cắt nó đi
/// để người dùng đọc được đường dẫn của chính mình. Bài kiểm chứng phải so cùng một dạng.
fn khong_verbatim(path: &std::path::Path) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => std::path::PathBuf::from(rest),
        None => path.to_path_buf(),
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

    let listed = harness
        .sessions
        .list(SessionScope::All, Some(10))
        .await
        .expect("liệt kê");
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
        workspace: Some(workspace.clone()),
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
    let listed = harness
        .sessions
        .list(SessionScope::All, Some(10))
        .await
        .expect("liệt kê");
    assert!(
        listed
            .iter()
            .any(|h| h.title.as_deref() == Some("đặt tên rồi"))
    );

    harness.sessions.delete(&id).await.expect("xoá được");
    let after = harness
        .sessions
        .list(SessionScope::All, Some(10))
        .await
        .expect("liệt kê");
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
    assert!(
        harness
            .forget_project(&current.expect("có dự án đang mở").id)
            .is_err()
    );
}

#[tokio::test]
async fn danh_sach_phien_chi_co_phien_cua_du_an_dang_mo() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let first = harness.workspace().expect("có dự án").display().to_string();

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
            .list(SessionScope::All, Some(10))
            .await
            .expect("liệt kê")
            .len(),
        2
    );
    let mine: Vec<_> = harness
        .sessions
        .list(SessionScope::All, Some(10))
        .await
        .expect("liệt kê")
        .into_iter()
        .filter(|h| h.cwd.as_deref() == Some(other_root.display().to_string().as_str()))
        .collect();
    assert_eq!(mine.len(), 1);
}

/// Dự án tài liệu **không được** có tool sửa tệp hay chạy lệnh.
///
/// Đây là khẳng định bảo mật của cả tính năng "dự án hai loại". Một thư viện tài liệu là
/// một chồng tệp do người khác gửi tới; nếu `bash` và `edit` vẫn còn đó thì cái duy nhất
/// ngăn cách một câu trong một tệp PDF với một lệnh chạy trên máy người dùng là việc mô
/// hình có nghe theo hay không — và đó không phải một ranh giới.
///
/// Bài test khẳng định bằng **danh sách tool thật** sau khi đổi dự án, chứ không bằng việc
/// đọc lại hằng số cấu hình: hằng số nói ý định, danh sách nói cái thật sự đã cắm.
#[tokio::test]
async fn du_an_tai_lieu_khong_co_tool_sua_tep_hay_chay_lenh() {
    use pai_project::ProjectKind;

    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");

    let ten = |registry: &pai_tools::ToolRegistry| -> Vec<String> {
        registry
            .schemas(None)
            .into_iter()
            .map(|s| s.name.as_str().to_string())
            .collect()
    };

    // Dự án mã nguồn khởi động: bộ tool đầy đủ.
    let truoc = ten(&registry);
    assert!(
        truoc.iter().any(|n| n == "bash") && truoc.iter().any(|n| n == "read"),
        "dự án mã nguồn thiếu tool cơ bản: {truoc:?}"
    );

    // Một thư mục khác, ghi nhận là dự án tài liệu **trước khi** mở nó.
    let thu_vien = TempDir::new().expect("thư mục tạm");
    let goc = thu_vien.path().canonicalize().expect("phân giải");
    harness
        .create_project(&goc, ProjectKind::Docs, None)
        .expect("ghi nhận được dự án tài liệu");
    harness.open_project(&goc).await.expect("mở được");

    let sau = ten(&registry);
    for cam in [
        "bash",
        "read",
        "edit",
        "write",
        "symbol_search",
        "grep",
        "glob",
    ] {
        assert!(
            !sau.iter().any(|n| n == cam),
            "dự án tài liệu vẫn còn tool `{cam}`: {sau:?}"
        );
    }

    // Và nó **có** thứ của riêng nó: ba tool tài liệu. Khẳng định cả hai chiều, vì
    // "không có tool nguy hiểm" mà cũng không có tool nào dùng được thì là một dự án chết
    // chứ không phải một dự án an toàn.
    for can in ["docs.search", "docs.read", "docs.list"] {
        assert!(
            sau.iter().any(|n| n == can),
            "dự án tài liệu thiếu tool `{can}`: {sau:?}"
        );
    }

    // Và loại được giữ lại: mở lại lần nữa không âm thầm biến nó thành dự án mã nguồn.
    harness.open_project(&goc).await.expect("mở lại được");
    let lai = ten(&registry);
    assert!(
        !lai.iter().any(|n| n == "bash"),
        "mở lại làm dự án tài liệu mọc lại `bash`: {lai:?}"
    );
}

/// Every bundled skill actually reaches the prompt.
///
/// `builtin_skills()` probes four paths, and all four are the kind of thing that only
/// breaks when packaging changes or the directory layout moves — meaning never while
/// writing code, and always at release. This test anchors it: every skill in
/// `harness/skills/` must show up in the prompt the model reads, not merely sit on disk.
#[tokio::test]
async fn bundled_skills_reach_the_prompt() {
    use pai_agent::Prompt;

    let dir = TempDir::new().expect("thư mục tạm");
    let skills = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../skills");
    assert!(skills.is_dir(), "không tìm thấy {}", skills.display());

    let harness = boot(Config {
        builtin_skills: Some(skills),
        ..config(&dir)
    })
    .await
    .expect("dựng được cây");

    let prompt = harness.ctx.require::<Prompt>().expect("có prompt");
    let text = prompt.assemble();
    for ten in [
        "flowchart",
        "sequence-diagram",
        "class-diagram",
        "er-diagram",
        "state-diagram",
        "architecture-diagram",
        "mindmap",
        "timeline",
        "user-journey",
        "summarize-document",
        "synthesize-sources",
    ] {
        assert!(text.contains(ten), "prompt thiếu skill `{ten}`");
    }
    harness.shutdown().await;
}

/// A Vietnamese question still selects the skills, all of which are named in English.
///
/// This is the one thing an English `name` can quietly break. Selection scores `name`,
/// `title`, `keywords` and `description` against the user's text with diacritics folded
/// away — so `so-do-tuan-tu` used to *be* the phrase a Vietnamese user types, worth three
/// points on its own. English names give that up: `sequence-diagram` matches nothing a
/// Vietnamese question contains, and the same was already true of the two English-bodied
/// skills. The Vietnamese `title` and `keywords` are what carry every one of them, and
/// nothing else does — which is exactly what this test pins down.
#[test]
fn vietnamese_questions_still_select_the_english_skills() {
    use pai_agent::SkillRegistry;

    let registry = SkillRegistry::new();
    registry.scan(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../skills"));

    for (question, expected) in [
        ("tóm tắt tài liệu này giúp tôi", "summarize-document"),
        ("tom tat ho toi cai bao cao", "summarize-document"),
        ("tài liệu này nói gì", "summarize-document"),
        ("tổng hợp nhiều nguồn giúp tôi", "synthesize-sources"),
        ("so sánh tài liệu v1 với v3", "synthesize-sources"),
        ("hai bản này khác nhau chỗ nào", "synthesize-sources"),
        // Chín gói sơ đồ, hỏi bằng đúng cụm người dùng gõ — kể cả khi họ gõ không dấu.
        ("vẽ sơ đồ luồng cho quy trình duyệt", "flowchart"),
        ("ve so do tuan tu giua hai ben", "sequence-diagram"),
        ("vẽ sơ đồ lớp cho mấy kiểu này", "class-diagram"),
        ("vẽ sơ đồ thực thể của cơ sở dữ liệu", "er-diagram"),
        ("vẽ sơ đồ trạng thái của đơn hàng", "state-diagram"),
        ("vẽ sơ đồ kiến trúc hệ thống", "architecture-diagram"),
        ("vẽ sơ đồ tư duy từ tài liệu này", "mindmap"),
        ("vẽ đường thời gian các đợt phát hành", "timeline"),
        ("vẽ sơ đồ hành trình người dùng", "user-journey"),
    ] {
        let chosen = registry.select(question);
        assert!(
            chosen.iter().any(|name| name == expected),
            "`{question}` phải chọn được `{expected}`, chọn ra: {chosen:?}"
        );
    }
}

/// Đổi nhà cung cấp **hội thoại** không kéo bộ nhúng đi theo.
///
/// Đây là toàn bộ lý do hai vai được tách ra. Trước khi tách, chọn một provider từ xa để
/// trò chuyện cũng lặng lẽ gửi mọi tài liệu người dùng vừa nạp sang đúng chỗ đó để nhúng —
/// không có gì trên màn hình nói ra, và người dùng chỉ phát hiện khi đọc log mạng.
///
/// Bài này khẳng định bằng **bộ nhúng đang cắm thật**, không bằng hàng trong kho: kho đúng
/// mà đường truyền tới `ActiveEmbedder` sai thì lỗi vẫn còn nguyên.
#[tokio::test]
async fn doi_nha_cung_cap_hoi_thoai_khong_keo_bo_nhung_di_theo() {
    use pai_llm::ProviderKind;
    use pai_providers::ProviderInput;

    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    // Hàng gieo là Ollama trên máy này, giữ cả hai vai.
    //
    // Đọc **tệp cấu hình mà thư viện native đọc**, không suy ra từ provider store: tệp
    // này là thứ duy nhất quyết định
    // tài liệu được gửi tới đâu. Kiểm nó là kiểm cả bất biến lẫn đường ống chở nó.
    let doc_cau_hinh = || -> serde_json::Value {
        let raw = std::fs::read_to_string(harness.rag_config.path())
            .expect("app phải ghi cấu hình RAG ngay khi áp provider");
        serde_json::from_str(&raw).expect("cấu hình RAG phải là JSON hợp lệ")
    };
    let truoc = doc_cau_hinh()["embedding"]["model"]
        .as_str()
        .expect("vai nhúng phải có mô hình")
        .to_string();
    assert_eq!(truoc, "qwen3-embedding:4b");

    // Thêm một provider từ xa và giao cho nó **vai hội thoại**.
    let xa = harness
        .providers
        .save(
            ProviderInput::create(
                "Một máy chủ từ xa",
                ProviderKind::OpenAiCompatible,
                "https://vi-du.test/v1",
            )
            .with_api_key("sk-thu")
            .with_model("mo-hinh-xa"),
        )
        .await
        .expect("lưu được");
    harness
        .providers
        .activate(xa.id(), Some("mo-hinh-xa"))
        .await
        .expect("giao được vai hội thoại");
    harness.apply_provider().await.expect("áp được");

    // Vai hội thoại đã đổi…
    assert_eq!(
        harness
            .providers
            .active()
            .expect("đọc được")
            .map(|item| item.id().to_string()),
        Some(xa.id().to_string())
    );

    // …còn vai nhúng thì **không**. Tài liệu vẫn được nhúng tại chỗ.
    let cau_hinh = doc_cau_hinh();
    let sau = cau_hinh["embedding"]["model"].as_str().unwrap_or_default();
    assert_eq!(
        sau, truoc,
        "đổi provider hội thoại đã kéo vai nhúng đi theo"
    );
    // Và vai hội thoại thì phải đổi thật — nếu không thì bài này xanh vì tệp không bao
    // giờ được ghi lại, chứ không phải vì bất biến đúng.
    assert_eq!(
        cau_hinh["chat"]["base_url"].as_str().unwrap_or_default(),
        xa.config.base_url,
        "vai hội thoại chưa được ghi lại vào cấu hình RAG"
    );

    harness.shutdown().await;
}

/// Không có dự án nào thì ứng dụng **vẫn dựng lên được và vẫn trò chuyện được**.
///
/// Đây là trạng thái lần đầu mở ứng dụng. Trước đây nó không tồn tại: `boot` lấy thư mục
/// hiện hành làm dự án, nên mở từ Finder cho một "dự án" tên `/` mà người dùng chưa bao
/// giờ chọn — cùng với `fs`, `shell` và `index` cắm vào gốc đĩa.
///
/// Bài này khẳng định cả hai nửa. Nửa an toàn: không tool nào chạm đĩa. Nửa còn lại quan
/// trọng ngang thế: hội thoại vẫn có tool để chạy, vì một ứng dụng "an toàn" mà không làm
/// được gì thì người dùng chỉ kết luận là nó hỏng.
#[tokio::test]
async fn khong_co_du_an_thi_van_tro_chuyen_duoc() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(Config {
        workspace: None,
        ..config(&dir)
    })
    .await
    .expect("dựng được cây khi không có dự án");

    assert!(
        harness.current_project().is_none(),
        "không được tự nhận dự án"
    );
    assert!(harness.workspace().is_none());

    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");
    let names: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|s| s.name.as_str().to_string())
        .collect();

    for cam in [
        "bash",
        "read",
        "edit",
        "write",
        "grep",
        "glob",
        "symbol_search",
        "docs.search",
    ] {
        assert!(
            !names.iter().any(|n| n == cam),
            "không có dự án mà vẫn cắm `{cam}`: {names:?}"
        );
    }
    assert!(
        names.iter().any(|n| n == "todo_write"),
        "hội thoại phải còn tool để chạy: {names:?}"
    );

    // Và một phiên mới ghi được, không kèm thư mục nào.
    let session = harness
        .sessions
        .create(pai_session::NewSession::default())
        .await
        .expect("tạo được phiên không thuộc dự án nào");
    assert!(session.header().cwd.is_none());

    harness.shutdown().await;
}

/// Đóng dự án đưa ứng dụng về đúng trạng thái không-có-dự-án, không phải một trạng thái
/// thứ ba nào khác.
#[tokio::test]
async fn dong_du_an_thi_tool_cham_dia_bien_mat() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");

    let ten = || -> Vec<String> {
        registry
            .schemas(None)
            .into_iter()
            .map(|s| s.name.as_str().to_string())
            .collect()
    };
    assert!(ten().iter().any(|n| n == "bash"), "{:?}", ten());

    harness.close_project().await;

    assert!(harness.current_project().is_none());
    let sau = ten();
    for cam in ["bash", "read", "edit", "write"] {
        assert!(
            !sau.iter().any(|n| n == cam),
            "đóng dự án rồi vẫn còn `{cam}`: {sau:?}"
        );
    }
    assert!(sau.iter().any(|n| n == "todo_write"), "{sau:?}");

    harness.shutdown().await;
}

/// Dự án tài liệu được cắm `rag`, và thư viện soi vào **đúng thư mục người dùng chọn**.
///
/// Phần quét thư mục, rút chữ và nhúng nằm trong `pai-rag` và có bài kiểm chứng riêng.
///
/// Cái còn lại, và cũng là **chỗ từng hỏng thật**, là chỗ nối: cây plugin có cắm `rag`
/// cho đúng loại dự án không, seam `Docs` có được cấp không, ba tool đọc có vào sổ đăng
/// ký với đúng siêu dữ liệu không, và thư mục truyền xuống có đúng thư mục người dùng
/// chọn không. Lần hỏng trước không nằm trong `pai-rag` mà nằm ở chỗ nối — nó không hề
/// biết thư mục người dùng chọn là thư mục nào.
#[tokio::test]
async fn du_an_tai_lieu_duoc_cam_rag_va_soi_dung_thu_muc() {
    use pai_project::ProjectKind;
    use pai_rag::Docs;

    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    let thu_vien = TempDir::new().expect("thư mục tạm");
    let goc = thu_vien.path().canonicalize().expect("phân giải");
    std::fs::write(
        goc.join("ghi-chu.md"),
        "# Ghi chú\n\nNội dung thử nghiệm.\n",
    )
    .expect("ghi");

    harness
        .create_project(&goc, ProjectKind::Docs, None)
        .expect("ghi nhận được dự án tài liệu");
    harness.open_project(&goc).await.expect("mở được");

    assert!(
        harness.ctx.get::<Docs>().is_some(),
        "dự án tài liệu phải có seam thư viện"
    );

    // Ba tool đọc, và **chỉ** ba. Service phơi thêm bốn tool quản lý — sync, ingest,
    // reprocess, remove — mà mô hình không được chạm tới: một tài liệu không đáng tin có
    // thể bảo nó nạp thêm tệp hoặc xoá sạch thư viện.
    let ten: Vec<String> = harness
        .ctx
        .require::<pai_tools::Tools>()
        .expect("sổ tool")
        .visible(None)
        .iter()
        .map(|tool| tool.schema().name.as_str().to_string())
        .filter(|name| name.starts_with("docs."))
        .collect();
    for mong_doi in ["docs.search", "docs.read", "docs.list"] {
        assert!(
            ten.iter().any(|name| name == mong_doi),
            "thiếu {mong_doi}: {ten:?}"
        );
    }
    for cam in ["docs.sync", "docs.ingest", "docs.reprocess", "docs.remove"] {
        assert!(
            !ten.iter().any(|name| name == cam),
            "`{cam}` không được lọt vào tầm với của mô hình: {ten:?}"
        );
    }

    // Cấu hình ghi ra cho service phải trỏ đúng thư mục người dùng chọn.
    let raw = std::fs::read_to_string(harness.rag_config.path()).expect("đọc cấu hình RAG");
    let cau_hinh: serde_json::Value = serde_json::from_str(&raw).expect("JSON hợp lệ");
    let root = cau_hinh["projects"][0]["root"]
        .as_str()
        .expect("phải khai thư mục dự án");
    assert_eq!(
        std::path::Path::new(root),
        khong_verbatim(&goc),
        "thư viện đang soi vào nhầm thư mục"
    );

    // Và tệp gốc **vẫn nằm nguyên chỗ cũ**, không bị chép đi đâu.
    assert!(goc.join("ghi-chu.md").is_file());

    harness.shutdown().await;
}

/// Đổi **từ** dự án tài liệu **sang** dự án mã nguồn thì tool đọc/sửa/chạy quay lại.
///
/// Bộ test đang có chỉ đi một chiều — mã nguồn sang tài liệu — và chiều đó xanh. Chiều
/// ngược lại chưa ai đi qua, mà đó mới là chiều người dùng gặp: họ mở một thư viện tài
/// liệu, rồi chuyển sang repo của mình và thấy trợ lý nói nó không có tool nào để liệt kê
/// tệp.
#[tokio::test]
async fn doi_tu_du_an_tai_lieu_sang_ma_nguon_thi_tool_quay_lai() {
    use pai_project::ProjectKind;

    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");
    let ten = || -> Vec<String> {
        registry
            .schemas(None)
            .into_iter()
            .map(|s| s.name.as_str().to_string())
            .collect()
    };

    let thu_vien = TempDir::new().expect("thư mục tạm");
    let goc_docs = thu_vien.path().canonicalize().expect("phân giải");
    harness
        .create_project(&goc_docs, ProjectKind::Docs, None)
        .expect("ghi nhận");
    harness.open_project(&goc_docs).await.expect("mở được");
    assert!(!ten().iter().any(|n| n == "read"), "{:?}", ten());
    // Kệ đính kèm là chuyện của dự án mã nguồn: ở đây tệp người dùng gửi đã nằm sẵn trong
    // thư viện, nên mount thứ hai không có lý do tồn tại.
    for cam in ["attachment.search", "attachment.read", "attachment.list"] {
        assert!(
            !ten().iter().any(|n| n == cam),
            "dự án tài liệu không nên có `{cam}`: {:?}",
            ten()
        );
    }

    // Rồi sang một dự án mã nguồn.
    let repo = TempDir::new().expect("thư mục tạm");
    let goc_code = repo.path().canonicalize().expect("phân giải");
    std::fs::write(goc_code.join("a.rs"), "fn main() {}\n").expect("ghi");
    harness
        .create_project(&goc_code, ProjectKind::Code, None)
        .expect("ghi nhận");
    harness.open_project(&goc_code).await.expect("mở được");

    let sau = ten();
    for can in [
        "read",
        "write",
        "edit",
        "glob",
        "grep",
        "bash",
        "symbol_search",
    ] {
        assert!(
            sau.iter().any(|n| n == can),
            "sang dự án mã nguồn mà thiếu `{can}`: {sau:?}"
        );
    }
    // Và tool của thư viện tài liệu **đi hẳn**, không nằm lại chồng lên.
    for cam in ["docs.search", "docs.read", "docs.list"] {
        assert!(
            !sau.iter().any(|n| n == cam),
            "tool tài liệu còn nằm lại trong dự án mã nguồn: {sau:?}"
        );
    }
    // Đổi lại, dự án mã nguồn có kệ đính kèm: cùng ba tool ấy trên thư mục tệp đính kèm,
    // mang tên riêng, vì `read` không mở nổi một tệp PDF hay DOCX.
    for can in ["attachment.search", "attachment.read", "attachment.list"] {
        assert!(
            sau.iter().any(|n| n == can),
            "dự án mã nguồn thiếu `{can}`: {sau:?}"
        );
    }

    // Đọc được một tệp thật, không chỉ có tên tool trong danh sách.
    let pipeline = pai_tools::ToolPipeline::new(&harness.ctx, registry.clone());
    let doc = pipeline
        .execute(
            "c1",
            "read",
            serde_json::json!({ "file_path": goc_code.join("a.rs") }),
        )
        .await;
    assert!(!doc.is_error, "không đọc được: {}", doc.content);
    assert!(doc.content.contains("fn main"));

    harness.shutdown().await;
}

/// Dự án **mã nguồn** không được có tên trong tệp cấu hình của RAG native.
///
/// Bộ tool đã lọc đúng từ lâu — `rag` chỉ nạp cho dự án tài liệu — nên nhìn từ trong ứng
/// dụng thì mọi thứ ổn. Nhưng tệp cấu hình lại được ghi cho **mọi loại** dự án: mở một
/// repo là ghi luôn đường dẫn repo ấy vào `projects` và `active_project`. Nếu `rag` bị
/// cắm nhầm, lần `docs.sync` kế tiếp sẽ quét cả cây mã nguồn và cắt đoạn như văn xuôi.
///
/// Bài này khoá cả hai chiều: mở dự án mã nguồn thì tệp trống, và đổi từ tài liệu sang
/// mã nguồn thì tên thư viện cũ **đi hẳn**, không nằm lại.
#[tokio::test]
async fn du_an_ma_nguon_khong_lot_vao_cau_hinh_rag() {
    use pai_project::ProjectKind;

    let dir = TempDir::new().expect("thư mục tạm");
    let mut dau = config(&dir);
    dau.workspace = None;
    let harness = boot(dau).await.expect("dựng được cây");

    let cau_hinh = || -> serde_json::Value {
        let raw = std::fs::read_to_string(harness.rag_config.path()).expect("đọc cấu hình RAG");
        serde_json::from_str(&raw).expect("cấu hình RAG phải là JSON hợp lệ")
    };

    let repo = TempDir::new().expect("thư mục tạm");
    let goc_code = repo.path().canonicalize().expect("phân giải");
    std::fs::write(goc_code.join("a.rs"), "fn main() {}\n").expect("ghi");
    harness
        .create_project(&goc_code, ProjectKind::Code, None)
        .expect("ghi nhận");
    harness.open_project(&goc_code).await.expect("mở được");

    let sau = cau_hinh();
    assert_eq!(
        sau["projects"].as_array().map(Vec::len),
        Some(0),
        "cây mã nguồn bị khai làm thư viện tài liệu: {sau}"
    );
    assert!(
        sau["active_project"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "`docs.sync` không tham số sẽ quét đúng cây mã nguồn này: {sau}"
    );

    // Mở một thư viện thật thì tệp lại có tên nó — nếu không, bài trên xanh vì lý do sai.
    let thu_vien = TempDir::new().expect("thư mục tạm");
    let goc_docs = thu_vien.path().canonicalize().expect("phân giải");
    std::fs::write(goc_docs.join("ghi-chu.md"), "# Ghi chú\n").expect("ghi");
    harness
        .create_project(&goc_docs, ProjectKind::Docs, None)
        .expect("ghi nhận");
    harness.open_project(&goc_docs).await.expect("mở được");
    let giua = cau_hinh();
    assert_eq!(
        std::path::Path::new(giua["projects"][0]["root"].as_str().unwrap_or_default()),
        khong_verbatim(&goc_docs),
        "mở thư viện mà cấu hình không biết: {giua}"
    );

    // Rồi đổi chính thư viện ấy sang mã nguồn: tên cũ phải biến mất, không được nằm lại.
    let id = harness.current_project().expect("có dự án").id;
    harness
        .set_project_kind(&id, ProjectKind::Code)
        .await
        .expect("đổi được loại");
    let cuoi = cau_hinh();
    assert!(
        cuoi["active_project"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "đổi sang mã nguồn mà thư viện cũ còn nằm lại trong cấu hình: {cuoi}"
    );

    harness.shutdown().await;
}

/// Đổi loại một dự án **đang mở** thì bộ tool đổi theo ngay.
///
/// Loại được đặt một lần lúc ghi nhận và `open_project` cố ý giữ nguyên nó — nên không có
/// đường đổi loại thì một thư mục vào nhầm loại là ngõ cụt vĩnh viễn. Bài này khoá cả hai
/// nửa: hàng trong kho đổi, **và** bộ tool đang chạy đổi theo mà không cần khởi động lại.
#[tokio::test]
async fn doi_loai_du_an_dang_mo_thi_bo_tool_doi_theo_ngay() {
    use pai_project::ProjectKind;

    let dir = TempDir::new().expect("thư mục tạm");
    let repo = TempDir::new().expect("thư mục tạm");
    let goc = repo.path().canonicalize().expect("phân giải");
    let harness = boot(Config {
        workspace: Some(goc.clone()),
        ..config(&dir)
    })
    .await
    .expect("dựng được cây");

    let registry = harness.ctx.require::<Tools>().expect("có sổ đăng ký");
    let ten = || -> Vec<String> {
        registry
            .schemas(None)
            .into_iter()
            .map(|s| s.name.as_str().to_string())
            .collect()
    };
    assert!(ten().iter().any(|n| n == "bash"), "{:?}", ten());

    let id = harness.current_project().expect("có dự án").id;
    harness
        .set_project_kind(&id, ProjectKind::Docs)
        .await
        .expect("đổi được loại");

    let sau = ten();
    assert!(
        !sau.iter().any(|n| n == "bash"),
        "đổi sang tài liệu mà còn `bash`: {sau:?}"
    );
    assert!(sau.iter().any(|n| n == "docs.search"), "{sau:?}");

    // Và đổi ngược lại cũng phải chạy — đây mới là chiều người dùng cần khi họ lỡ ghi
    // nhận một repo thành thư viện tài liệu.
    harness
        .set_project_kind(&id, ProjectKind::Code)
        .await
        .expect("đổi lại được");
    let lai = ten();
    for can in ["read", "grep", "bash"] {
        assert!(
            lai.iter().any(|n| n == can),
            "đổi về mã nguồn mà thiếu `{can}`: {lai:?}"
        );
    }

    harness.shutdown().await;
}

/// Màn hình Quyền đọc được **mức giam thật**, không phải một chỗ trống.
///
/// `describe_harness` chỉ nói hàng `sandbox` có cắm hay không — nó in cây cấu hình, không
/// in `Enforcement`. Nên trước lệnh này, màn hình quyền chỉ nói được "vòng giam đang được
/// cắm", mà câu đó đúng với cả một vòng giam thủng lẫn một vòng giam không tồn tại. Đó là
/// đúng thứ mà `pai-sandbox` được thiết kế để không bao giờ nói.
#[tokio::test]
async fn man_hinh_quyen_doc_duoc_muc_giam_that() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    let sandbox = harness
        .ctx
        .get::<pai_sandbox::Sandbox>()
        .expect("vòng giam phải được cắm");
    let muc = sandbox.enforcement();

    // Mức nào cũng được — cái phải đúng là **nhãn có nghĩa** và lý do đi kèm khi thủng.
    assert!(
        ["full", "partial", "none"].contains(&muc.label()),
        "nhãn lạ: {}",
        muc.label()
    );
    if !muc.is_full() {
        assert!(
            muc.reason().is_some_and(|r| !r.trim().is_empty()),
            "vòng giam không kín mà không nói vì sao"
        );
    }

    // Và thư mục ghi được luôn chứa chính dự án đang mở — nếu không thì mọi lần ghi đều
    // bị chặn và triệu chứng sẽ giống hệt "tool hỏng".
    let goc = harness.workspace().expect("có dự án");
    let roots = pai_sandbox::writable_roots(&pai_sandbox::Policy::workspace_write(goc.clone()));
    assert!(
        roots.iter().any(|dir| goc.starts_with(dir) || dir == &goc),
        "thư mục dự án không nằm trong vùng ghi được: {roots:?}"
    );

    harness.shutdown().await;
}

/// `boot` khôi phục dự án gần nhất, và cấu hình ghi ra cho service **phải** nói tên nó.
///
/// Bài này khoá một lỗi đã xảy ra thật. `boot` dựng tầng plugin của dự án được khôi phục
/// ngay trong chính nó chứ không đi qua `open_project`, nên nó là chỗ **duy nhất** ghi
/// được cấu hình cho dự án ấy. Bản đầu truyền `None` xuống, và tệp ghi ra khai
/// `projects: []`: giao diện hiện một dự án đang mở, còn mọi lời gọi tới service trả về
/// "chưa có dự án nào đang mở".
///
/// Kiểu hỏng này không lộ ra trong bài kiểm `open_project` — ở đó mọi thứ đúng — mà chỉ
/// lộ khi mở lại ứng dụng, tức là ở lần chạy thứ hai của người dùng.
#[tokio::test]
async fn khoi_dong_lai_thi_cau_hinh_rag_van_biet_du_an_nao_dang_mo() {
    use pai_project::ProjectKind;

    let dir = TempDir::new().expect("thư mục tạm");
    let thu_vien = TempDir::new().expect("thư mục tạm");
    let goc = thu_vien.path().canonicalize().expect("phân giải");
    std::fs::write(goc.join("ghi-chu.md"), "# Ghi chú\n").expect("ghi");

    // Lần chạy thứ nhất: ghi nhận dự án rồi mở nó, y như người dùng làm.
    let mut dau = config(&dir);
    dau.workspace = None;
    let harness = boot(dau).await.expect("dựng được cây");
    harness
        .create_project(&goc, ProjectKind::Docs, None)
        .expect("ghi nhận dự án");
    harness.open_project(&goc).await.expect("mở được");
    harness.shutdown().await;

    // Lần chạy thứ hai: cùng thư mục dữ liệu, `boot` tự khôi phục dự án gần nhất.
    let mut lai = config(&dir);
    lai.workspace = Some(goc.clone());
    let harness = boot(lai).await.expect("dựng lại được cây");
    assert!(
        harness.current_project().is_some(),
        "boot phải khôi phục dự án gần nhất"
    );

    let raw = std::fs::read_to_string(harness.rag_config.path())
        .expect("boot phải ghi cấu hình RAG, kể cả khi không ai gọi open_project");
    let cau_hinh: serde_json::Value = serde_json::from_str(&raw).expect("JSON hợp lệ");
    assert_eq!(
        std::path::Path::new(cau_hinh["projects"][0]["root"].as_str().unwrap_or_default()),
        khong_verbatim(&goc),
        "cấu hình ghi lúc khởi động không biết dự án nào đang mở: {raw}"
    );
    assert!(
        !cau_hinh["active_project"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "thiếu `active_project` thì service từ chối mọi lời gọi: {raw}"
    );

    harness.shutdown().await;
}
