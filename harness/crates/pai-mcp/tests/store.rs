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
        err.to_string().contains("Authorization"),
        "lỗi phải nêu tên biến còn trống: {err}"
    );

    // Điền vào thì qua, và giá trị đi đúng chỗ.
    let values = BTreeMap::from([("Authorization".to_string(), "ghp_gia".to_string())]);
    let config = catalog::instantiate(entry, &values).expect("điền đủ thì dựng được");
    // GitHub là mục **chạy từ xa**: token đi vào header của lời gọi, không vào môi trường
    // của một tiến trình con — ở đây không có tiến trình con nào.
    let McpTransport::Http { url, headers } = &config.transport else {
        panic!("mục github phải ra một server http");
    };
    assert!(url.starts_with("https://"), "endpoint phải là https: {url}");
    assert_eq!(headers.get("Authorization").map(String::as_str), Some("ghp_gia"));
}

/// Mục chạy từ xa không được đòi hỏi gì trên máy này.
///
/// Đây chính là lý do người ta chọn nó thay cho bản chạy tại chỗ của cùng một dịch vụ, nên
/// một mục từ xa lỡ khai `requires` là một mục đang bắt người dùng cài thứ nó không dùng.
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

/// Cú bấm trong ứng dụng thắng hàng cấu hình, không phải ngược lại.
///
/// Hàng `mcp` trong tệp vá là thứ bản cài đặt mồi sẵn; kho là thứ người dùng vừa bấm ba
/// giây trước. Cho hàng cấu hình thắng nghĩa là cú bấm "tắt" im lặng không có tác dụng —
/// loại lỗi người dùng không báo cáo được, vì họ tưởng mình bấm hụt.
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

/// Một mục dán thiếu chỗ đi tới chỉ mất **một mục**, không kéo theo cả tệp.
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

/// Có cả `url` lẫn `command` thì `url` thắng: một mục dán chồng lên nhau vẫn phải đi tới
/// một chỗ xác định, và địa chỉ mạng cụ thể hơn một cái lệnh còn sót lại.
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

/// `enabled` tường minh thắng `disabled`, và thiếu cả hai thì mặc định là bật.
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

/// Tệp hỏng thì nói ra, và **không** bị ghi đè mất.
///
/// Trả về danh sách rỗng ở đây là kiểu hỏng tệ nhất: giao diện vẽ ra "chưa có server nào",
/// người dùng bấm thêm một cái, và lần lưu đó dựng lại tệp từ con số không — mọi server
/// cùng token của họ biến mất vì một dấu phẩy thừa.
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

/// Bật/tắt một cái tên không có trong kho là lỗi, không phải một thao tác im lặng trôi qua.
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

/// Nhiều luồng cùng lưu thì không ai bị nuốt mất.
///
/// Mỗi lần lưu là một chu trình đọc → sửa → ghi. Không có khoá thì cái ghi sau dựng lại
/// từ ảnh chụp cũ và xoá mất cái ghi trước: người dùng bấm thêm bốn server rồi thấy còn một.
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
