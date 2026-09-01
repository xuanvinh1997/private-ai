//! Kho cấu hình và danh mục dựng sẵn.
//!
//! Những bài ở đây khoá hai thứ mà người dùng không bao giờ thấy cho tới lúc chúng hỏng:
//! tệp cấu hình phải sống sót qua một lần ghi bị cắt, và một mục danh mục phải dựng ra
//! được một cấu hình chạy được. Cả hai đều hỏng một cách im lặng nếu không có bài kiểm.

use std::collections::BTreeMap;
use std::fs;

use pai_mcp::catalog::{self, CATALOG};
use pai_mcp::{ConfigError, McpStore, McpTransport, ServerConfig};

fn store(dir: &std::path::Path) -> McpStore {
    McpStore::open(dir.join("mcp.json"))
}

/// Cấu hình dán từ tài liệu bên thứ ba và cấu hình dạng gốc đều đọc được, và ghi ra rồi
/// đọc lại thì không mất gì.
#[test]
fn doc_duoc_ca_hai_hinh_dang_va_ghi_lai_tron_ven() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join("mcp.json");

    // Hình dạng của Claude Desktop / codex, cộng một khối dạng gốc trong cùng một tệp.
    fs::write(
        &path,
        r#"{
  "mcpServers": {
    "docs": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-fetch"],
      "env": { "TOKEN": "bí-mật" },
      "description": "một khoá lạ mà ta phải bỏ qua chứ không từ chối cả tệp"
    },
    "tat": { "command": "x", "disabled": true },
    "xa": { "url": "https://vi.du/mcp", "headers": { "Authorization": "Bearer abc" } }
  },
  "servers": [
    { "name": "goc", "transport": "stdio", "command": "goc-cmd", "max_retries": 1 }
  ]
}"#,
    )
    .expect("ghi tệp mẫu");

    let store = store(dir.path());
    let configs = store.list().expect("đọc được kho");
    let names: Vec<&str> = configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["docs", "goc", "tat", "xa"]);

    let docs = &configs[0];
    assert!(docs.enabled);
    let McpTransport::Stdio {
        command, args, env, ..
    } = &docs.transport
    else {
        panic!("`docs` phải là stdio");
    };
    assert_eq!(command, "npx");
    assert_eq!(args.len(), 2);
    assert_eq!(env.get("TOKEN").map(String::as_str), Some("bí-mật"));

    assert!(!configs[2].enabled, "`disabled: true` phải đọc thành tắt");
    assert!(matches!(configs[3].transport, McpTransport::Http { .. }));
    assert_eq!(
        configs[1].max_retries, 1,
        "hình dạng gốc giữ nguyên tham số"
    );

    // Ghi ra là dạng `mcpServers`, và đọc lại thì y hệt.
    store.save(configs[0].clone()).expect("lưu lại được");
    let text = fs::read_to_string(&path).expect("đọc tệp đã ghi");
    assert!(text.contains("\"mcpServers\""));
    assert!(!text.contains("\"servers\""));
    assert_eq!(store.list().expect("đọc lại"), configs);

    // Và mọi thao tác quản lý đều đi qua cùng một vòng đó.
    store.set_enabled("docs", false).expect("tắt được");
    assert!(!store.list().expect("đọc lại")[0].enabled);
    assert!(store.remove("xa").expect("xoá được"));
    assert!(!store.remove("xa").expect("xoá lần hai không phải lỗi"));
    assert_eq!(store.list().expect("đọc lại").len(), 3);
}

/// Sau khi lưu, thư mục chỉ còn đúng tệp kho: không có tệp tạm nào bị bỏ lại.
#[test]
fn ghi_nguyen_tu_khong_bo_lai_tep_tam() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let store = store(dir.path());
    for i in 0..3 {
        store
            .save(ServerConfig::stdio(format!("s{i}"), "cmd"))
            .expect("lưu được");
    }
    let left: Vec<String> = fs::read_dir(dir.path())
        .expect("đọc thư mục")
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().to_string()))
        .collect();
    assert_eq!(
        left,
        vec!["mcp.json".to_string()],
        "còn sót tệp tạm: {left:?}"
    );
}

/// Tệp chứa token, nên chỉ chủ nhân của nó đọc được.
#[cfg(unix)]
#[test]
fn tep_kho_chi_chu_no_doc_duoc() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("thư mục tạm");
    let store = store(dir.path());
    store
        .save(ServerConfig::stdio("co-token", "cmd"))
        .expect("lưu được");

    let mode = fs::metadata(dir.path().join("mcp.json"))
        .expect("đọc metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "kho MCP phải là 0600, đang là {mode:o}");
}

/// Cấu hình hỏng bị chặn ở cửa vào, không phải lúc nối.
#[test]
fn save_tu_choi_ten_khong_hop_le() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let store = store(dir.path());

    let err = store
        .save(ServerConfig::stdio("a__b", "cmd"))
        .expect_err("`__` phá phép chiếu tên sang dạng wire");
    assert!(
        err.to_string().contains("__"),
        "lỗi phải nói ra vì sao: {err}"
    );

    assert!(
        store.save(ServerConfig::stdio("a.b", "cmd")).is_err(),
        "dấu chấm trong tên server làm hai server đẻ ra cùng một danh tính tool"
    );

    assert!(
        store.list().expect("đọc kho").is_empty(),
        "cấu hình bị từ chối không được để lại dấu vết nào"
    );
}

/// Thiếu một biến bắt buộc thì lỗi phải gọi đúng tên nó ra.
#[test]
fn instantiate_noi_ro_bien_nao_con_thieu() {
    let entry = catalog::find("github").expect("danh mục có github");
    let err = catalog::instantiate(entry, &BTreeMap::new())
        .expect_err("thiếu token thì không dựng được cấu hình");

    assert!(matches!(err, ConfigError::MissingValue(_, _)));
    assert!(
        err.to_string().contains("GITHUB_PERSONAL_ACCESS_TOKEN"),
        "lỗi phải nêu tên biến còn trống: {err}"
    );

    // Điền vào thì qua, và giá trị đi đúng chỗ.
    let values = BTreeMap::from([(
        "GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
        "ghp_gia".to_string(),
    )]);
    let config = catalog::instantiate(entry, &values).expect("điền đủ thì dựng được");
    let McpTransport::Stdio { env, .. } = &config.transport else {
        panic!("mục danh mục phải ra một server stdio");
    };
    assert_eq!(
        env.get("GITHUB_PERSONAL_ACCESS_TOKEN").map(String::as_str),
        Some("ghp_gia")
    );
}

/// Mọi mục trong danh mục đều dựng ra được một cấu hình hợp lệ.
///
/// Bài này canh chính cái bảng: thêm một mục có `id` sai luật đặt tên, hay một `${...}`
/// không có biến tương ứng, sẽ hỏng ở đây chứ không hỏng trên máy người dùng.
#[test]
fn moi_muc_danh_muc_dung_duoc() {
    for entry in CATALOG {
        let values: BTreeMap<String, String> = entry
            .env
            .iter()
            .filter(|var| var.required)
            .map(|var| (var.key.to_string(), format!("gia-tri-cua-{}", var.key)))
            .collect();

        let config = catalog::instantiate(entry, &values)
            .unwrap_or_else(|err| panic!("mục `{}` không dựng được: {err}", entry.id));
        config
            .validate()
            .unwrap_or_else(|err| panic!("mục `{}` dựng ra cấu hình sai: {err}", entry.id));
        assert_eq!(config.name, entry.id);

        let McpTransport::Stdio { command, args, .. } = &config.transport else {
            panic!("mục `{}` phải ra một server stdio", entry.id);
        };
        assert!(!command.trim().is_empty());
        for arg in args {
            assert!(
                !arg.contains("${"),
                "mục `{}` còn chỗ trống chưa điền trong đối số `{arg}`",
                entry.id
            );
        }
        assert!(
            !entry.summary.is_empty() && !entry.homepage.is_empty(),
            "mục `{}` phải nói được nó làm gì và tra ở đâu",
            entry.id
        );
    }
}

/// Biến không bắt buộc mà bỏ trống thì đối số mang nó biến mất cả cụm.
///
/// Nếu chỉ bỏ phần giá trị thì cái cờ trơ lại trên dòng lệnh, và server từ chối khởi động
/// vì một tham số thiếu — một kiểu hỏng mà người dùng không có manh mối nào để lần ra.
#[test]
fn bien_khong_bat_buoc_bo_trong_thi_bo_ca_doi_so() {
    let entry = catalog::find("git").expect("danh mục có git");
    let config = catalog::instantiate(entry, &BTreeMap::new()).expect("git không bắt buộc gì");
    let McpTransport::Stdio { args, env, .. } = &config.transport else {
        panic!("git phải là stdio");
    };
    assert_eq!(args, &["mcp-server-git".to_string()]);
    assert!(env.is_empty());

    let values = BTreeMap::from([("GIT_REPOSITORY".to_string(), "/kho/cua/toi".to_string())]);
    let config = catalog::instantiate(entry, &values).expect("điền vào thì dựng được");
    let McpTransport::Stdio { args, env, .. } = &config.transport else {
        panic!("git phải là stdio");
    };
    assert_eq!(args[1], "--repository=/kho/cua/toi");
    assert!(
        env.is_empty(),
        "giá trị đã lên dòng lệnh thì không nhân đôi vào môi trường"
    );
}
