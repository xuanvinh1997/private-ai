//! The config store and the built-in catalogue.
//! Two things nobody sees until they break: the file must survive a half-written save, and
//! every catalogue entry must produce a config that actually runs.

use std::collections::BTreeMap;
use std::fs;

use pai_mcp::catalog::{self, CATALOG};
use pai_mcp::{ConfigError, McpStore, McpTransport, ServerConfig};

fn store(dir: &std::path::Path) -> McpStore {
    McpStore::open(dir.join("mcp.json"))
}

/// Both the pasted and the native shapes parse, and a write-then-read round trip loses nothing.
#[test]
fn doc_duoc_ca_hai_hinh_dang_va_ghi_lai_tron_ven() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join("mcp.json");

    // The Claude Desktop / codex shape, plus a native block in the same file.
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

    // Written as `mcpServers`, and read back identical.
    store.save(configs[0].clone()).expect("lưu lại được");
    let text = fs::read_to_string(&path).expect("đọc tệp đã ghi");
    assert!(text.contains("\"mcpServers\""));
    assert!(!text.contains("\"servers\""));
    assert_eq!(store.list().expect("đọc lại"), configs);

    // And every management action goes through that same round trip.
    store.set_enabled("docs", false).expect("tắt được");
    assert!(!store.list().expect("đọc lại")[0].enabled);
    assert!(store.remove("xa").expect("xoá được"));
    assert!(!store.remove("xa").expect("xoá lần hai không phải lỗi"));
    assert_eq!(store.list().expect("đọc lại").len(), 3);
}

/// After a save the directory holds only the store file; no temp file is left behind.
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

/// The file holds tokens, so only its owner may read it.
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

/// Bad config is stopped at the door, not at dial time.
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

/// A missing required variable must be named in the error.
#[test]
fn instantiate_noi_ro_bien_nao_con_thieu() {
    let entry = catalog::find("github").expect("danh mục có github");
    let err = catalog::instantiate(entry, &BTreeMap::new())
        .expect_err("thiếu token thì không dựng được cấu hình");

    assert!(matches!(err, ConfigError::MissingValue(_, _)));
    assert!(
        err.to_string().contains("Authorization"),
        "lỗi phải nêu tên biến còn trống: {err}"
    );

    // Filled in it passes, and the value lands in the right place.
    let values = BTreeMap::from([("Authorization".to_string(), "ghp_gia".to_string())]);
    let config = catalog::instantiate(entry, &values).expect("điền đủ thì dựng được");
    // GitHub is a remote entry: the token becomes a request header, since there is no child process here.
    let McpTransport::Http { url, headers } = &config.transport else {
        panic!("mục github phải ra một server http");
    };
    assert!(url.starts_with("https://"), "endpoint phải là https: {url}");
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some("ghp_gia")
    );
}

/// A remote entry must require nothing locally: that is the whole reason to pick it over the local build.
#[test]
fn muc_tu_xa_khong_can_gi_tren_may() {
    for entry in CATALOG.iter().filter(|entry| entry.url.is_some()) {
        assert!(
            entry.requires.is_empty(),
            "mục từ xa `{}` không được đòi hỏi gì trên máy",
            entry.id
        );
        assert!(
            entry.command.is_empty() && entry.args.is_empty(),
            "mục từ xa `{}` không dựng tiến trình con nào",
            entry.id
        );
    }
}

/// Every catalogue entry yields a valid config, so a bad `id` or an unmatched `${...}` fails here, not on a user's machine.
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

        match &config.transport {
            McpTransport::Stdio { command, args, .. } => {
                assert!(!command.trim().is_empty());
                for arg in args {
                    assert!(
                        !arg.contains("${"),
                        "mục `{}` còn chỗ trống chưa điền trong đối số `{arg}`",
                        entry.id
                    );
                }
            }
            McpTransport::Http { url, .. } => {
                assert!(
                    !url.contains("${"),
                    "mục `{}` còn chỗ trống chưa điền trong endpoint `{url}`",
                    entry.id
                );
                assert!(
                    url.starts_with("https://"),
                    "mục `{}` phải quay số qua https, không phải `{url}`",
                    entry.id
                );
            }
        }
        assert!(
            !entry.summary.is_empty() && !entry.homepage.is_empty(),
            "mục `{}` phải nói được nó làm gì và tra ở đâu",
            entry.id
        );
    }
}

/// An empty optional variable removes its whole argument; a bare leftover flag would make the server refuse to start.
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

/// A click in the app beats a config row: otherwise a "disable" click silently does nothing.
#[test]
fn kho_cua_nguoi_dung_thang_hang_cau_hinh() {
    let mut tat = ServerConfig::stdio("github", "docker");
    tat.enabled = false;

    let gop = pai_mcp::merge(
        vec![
            ServerConfig::stdio("github", "docker"),
            ServerConfig::stdio("chi-co-o-hang", "npx"),
        ],
        vec![tat, ServerConfig::stdio("chi-co-trong-kho", "npx")],
    );

    let ten: Vec<&str> = gop.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(ten, ["chi-co-o-hang", "chi-co-trong-kho", "github"]);
    assert!(
        !gop.iter().find(|c| c.name == "github").expect("có").enabled,
        "kho tắt mà hàng cấu hình bật lại được thì cú bấm tắt vô nghĩa"
    );
}

/// A pasted entry with nowhere to go costs one entry, not the whole file.
#[test]
fn mot_muc_khong_noi_duoc_di_toi_dau_bi_bo_rieng_no() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    fs::write(
        dir.path().join("mcp.json"),
        r#"{"mcpServers": {
             "cut": { "args": ["-y", "x"] },
             "lanh": { "command": "npx" }
           }}"#,
    )
    .expect("ghi tệp mẫu");

    let configs = store(dir.path())
        .list()
        .expect("mục hỏng không làm hỏng cả tệp");
    let ten: Vec<&str> = configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(ten, ["lanh"]);
}

/// With both `url` and `command`, `url` wins: an address is more specific than a leftover command.
#[test]
fn co_ca_url_lan_command_thi_url_thang() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    fs::write(
        dir.path().join("mcp.json"),
        r#"{"mcpServers": { "hai-duong": {
             "command": "npx", "url": "https://vi.du/mcp"
           }}}"#,
    )
    .expect("ghi tệp mẫu");

    let configs = store(dir.path()).list().expect("đọc được");
    let McpTransport::Http { url, .. } = &configs[0].transport else {
        panic!("phải chọn đường HTTP");
    };
    assert_eq!(url, "https://vi.du/mcp");
}

/// An explicit `enabled` beats `disabled`, and neither means on.
#[test]
fn hai_cach_noi_nguoc_nhau_ve_bat_tat_deu_doc_duoc() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    fs::write(
        dir.path().join("mcp.json"),
        r#"{"mcpServers": {
             "a-tat-kieu-khac": { "command": "x", "disabled": true },
             "b-bat-tuong-minh": { "command": "x", "enabled": true, "disabled": true },
             "c-khong-noi-gi":   { "command": "x" }
           }}"#,
    )
    .expect("ghi tệp mẫu");

    let bat: Vec<bool> = store(dir.path())
        .list()
        .expect("đọc được")
        .iter()
        .map(|c| c.enabled)
        .collect();
    assert_eq!(bat, [false, true, true]);
}

/// A malformed file is reported, never overwritten: returning an empty list would let the next save erase everything.
#[test]
fn json_hong_thi_bao_loi_chu_khong_am_tham_lam_moi_tep() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join("mcp.json");
    let hong = r#"{"mcpServers": {"a": {"command": "x",}}}"#;
    fs::write(&path, hong).expect("ghi tệp hỏng");

    let store = store(dir.path());
    assert!(store.list().is_err(), "tệp hỏng phải nói ra");
    assert!(
        store.save(ServerConfig::stdio("moi", "npx")).is_err(),
        "lưu đè lên một tệp không đọc được là xoá cấu hình của người dùng"
    );
    assert_eq!(
        fs::read_to_string(&path).expect("đọc lại"),
        hong,
        "tệp phải nguyên vẹn để người dùng còn sửa tay được"
    );
}

/// Toggling a name that is not in the store is an error, not a silent no-op.
#[test]
fn set_enabled_ten_khong_co_thi_noi_ra() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let store = store(dir.path());
    store
        .save(ServerConfig::stdio("co-that", "npx"))
        .expect("lưu được");

    assert!(store.set_enabled("khong-co", false).is_err());
    assert!(
        store.list().expect("đọc lại")[0].enabled,
        "một lời gọi hỏng không được đụng tới hàng khác"
    );
}

/// Concurrent saves lose nothing; each save is read-modify-write, and without a lock the later one swallows the earlier.
#[test]
fn nhieu_luong_cung_luu_thi_khong_nuot_mat_ai() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let store = std::sync::Arc::new(store(dir.path()));

    std::thread::scope(|scope| {
        for i in 0..8 {
            let store = store.clone();
            scope.spawn(move || {
                store
                    .save(ServerConfig::stdio(format!("s{i}"), "cmd"))
                    .expect("lưu được");
            });
        }
    });

    let ten: Vec<String> = store
        .list()
        .expect("đọc lại")
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(ten.len(), 8, "mất server sau khi lưu song song: {ten:?}");
}
