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

/// Bộ skill đi kèm bản cài đặt thật sự tới được prompt.
///
/// `builtin_skills()` dò bốn chỗ theo đường dẫn, và cả bốn đều là loại thứ chỉ sai khi
/// đóng gói hoặc khi đổi bố cục thư mục — nghĩa là không bao giờ sai lúc viết mã, và luôn
/// sai lúc phát hành. Bài này neo nó lại: chín skill trong `harness/skills/` phải xuất
/// hiện trong prompt mà mô hình đọc, chứ không chỉ nằm trên đĩa.
#[tokio::test]
async fn skill_dung_san_di_toi_duoc_prompt() {
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
        "so-do-luong",
        "so-do-tuan-tu",
        "so-do-lop",
        "so-do-thuc-the",
        "so-do-trang-thai",
        "so-do-kien-truc",
        "so-do-tu-duy",
        "duong-thoi-gian",
        "so-do-hanh-trinh",
    ] {
        assert!(text.contains(ten), "prompt thiếu skill `{ten}`");
    }
    harness.shutdown().await;
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
    let truoc = harness
        .embedder
        .current()
        .map(|item| item.id().to_string())
        .expect("hàng gieo phải có bộ nhúng");
    assert_eq!(truoc, "nomic-embed-text");

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

    // …còn bộ nhúng thì **không**. Tài liệu vẫn được nhúng tại chỗ.
    let sau = harness
        .embedder
        .current()
        .map(|item| item.id().to_string())
        .expect("bộ nhúng không được biến mất");
    assert_eq!(sau, truoc, "đổi provider hội thoại đã kéo bộ nhúng đi theo");

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

/// Mở một thư mục làm dự án tài liệu thì **thấy ngay tệp đang nằm trong đó**.
///
/// Đây là bài khoá lại đúng câu hỏi người dùng đã hỏi: *"tại sao chọn folder nhưng phần
/// mềm không thấy file nào"*. Trước đây thư mục ấy không bao giờ được quét — thư viện chỉ
/// chứa tệp thêm tay, và bản sao của chúng nằm trong một thư mục ẩn.
///
/// Khẳng định đi qua **seam `Docs` thật** sau khi cây plugin đã cắm, chứ không gọi thẳng
/// `pai-rag`: chỗ hỏng lần trước không nằm trong `pai-rag` mà nằm ở chỗ nối — nó không hề
/// biết thư mục người dùng chọn là thư mục nào.
#[tokio::test]
async fn mo_thu_muc_tai_lieu_thi_thay_ngay_tep_trong_do() {
    use futures::StreamExt;
    use pai_project::ProjectKind;
    use pai_rag::Docs;

    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    // Một thư mục có sẵn tài liệu, y như thư mục người dùng đã chỉ vào.
    let thu_vien = TempDir::new().expect("thư mục tạm");
    let goc = thu_vien.path().canonicalize().expect("phân giải");
    std::fs::write(
        goc.join("ghi-chu.md"),
        "# Ghi chú\n\nNội dung thử nghiệm.\n",
    )
    .expect("ghi");
    std::fs::write(goc.join("bang.csv"), "ten,tuoi\nan,30\n").expect("ghi");
    std::fs::write(goc.join("anh.png"), [0x89, 0x50, 0x4e, 0x47]).expect("ghi");

    harness
        .create_project(&goc, ProjectKind::Docs, None)
        .expect("ghi nhận được dự án tài liệu");
    harness.open_project(&goc).await.expect("mở được");

    let library = harness
        .ctx
        .get::<Docs>()
        .expect("dự án tài liệu phải có thư viện");

    // Trước khi quét, thư viện trống — và `Library::open` cố ý không tự quét, vì một lần
    // quét đồng bộ lúc cắm plugin là đóng băng cửa sổ không có thanh tiến trình nào.
    assert_eq!(library.documents().expect("đọc được").len(), 0);

    let mut stream = library.sync();
    while stream.next().await.is_some() {}
    drop(stream);

    let docs = library.documents().expect("đọc được");
    let ten: Vec<String> = docs
        .iter()
        .map(|doc| {
            doc.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    assert!(ten.iter().any(|n| n == "ghi-chu.md"), "{ten:?}");
    assert!(ten.iter().any(|n| n == "bang.csv"), "{ten:?}");
    assert!(!ten.iter().any(|n| n == "anh.png"), "nạp cả ảnh: {ten:?}");

    // Và tệp gốc **vẫn nằm nguyên chỗ cũ**, không bị chép đi đâu.
    assert!(goc.join("ghi-chu.md").is_file());
    let stats = library.stats().expect("đọc được");
    assert_eq!(stats.root, goc, "thư viện đang soi vào nhầm thư mục");

    harness.shutdown().await;
}
