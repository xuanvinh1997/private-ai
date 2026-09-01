//! Những bất biến mà chỉ mục sai thì mô hình đi đọc nhầm chỗ.
//!
//! Mỗi bài ở đây khoá một câu đã viết trong tài liệu crate. Nếu một bài đỏ thì hoặc mã
//! sai, hoặc câu trong tài liệu đã hết đúng — không có khả năng thứ ba.

use std::path::Path;
use std::sync::Arc;

use pai_core::{Context, Plugin};
use pai_fs::FileRoots;
use pai_index::IndexPlugin;
use pai_index::index::{Index, SymbolIndex};
use pai_index::tools::outline::Outline;
use pai_index::tools::symbol_search::SymbolSearch;
use pai_index::{CodeIndex, Extractor, Symbol, SymbolKind};
use pai_tools::{Invocation, Tool, ToolMeta, ToolName, Tools, ToolsPlugin};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

const RUST_SOURCE: &str = r#"
pub const NGUONG: usize = 12;

pub struct KhoLuu {
    pub ten: String,
}

pub trait DocDuoc {
    fn doc(&self) -> String;
}

impl KhoLuu {
    pub fn mo(ten: &str) -> KhoLuu {
        KhoLuu { ten: ten.to_string() }
    }

    fn dong(&mut self) {}
}

pub fn tu_do() -> usize {
    NGUONG
}
"#;

const TS_SOURCE: &str = r#"
export interface HopDong {
  ma: string;
}

export type Nhan = "moi" | "cu";

export const GIOI_HAN = 20;

export class SoDangKy {
  private hang: HopDong[] = [];

  them(muc: HopDong): void {
    this.hang.push(muc);
  }
}

export const dungSo = (ma: string): HopDong => ({ ma });

function noiBo(): void {}
"#;

const PY_SOURCE: &str = r#"
NGUONG = 12


class KhoLuu:
    def mo(self, ten):
        return ten

    def dong(self):
        pass


def tu_do():
    return NGUONG
"#;

fn bench(files: &[(&str, &str)]) -> (TempDir, CodeIndex) {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải gốc");
    for (name, body) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("dựng thư mục");
        }
        std::fs::write(&path, body).expect("ghi tệp");
    }
    let roots = FileRoots::new([root], []);
    let index = CodeIndex::in_memory(roots).expect("mở chỉ mục");
    (dir, index)
}

fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("không thấy `{name}` trong {symbols:#?}"))
}

fn call(name: &str, args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from(name), "c1", map)
}

/// Grammar và core lệch ABI thì hỏng **lúc chạy**, không lúc biên dịch. Bài này là chỗ
/// duy nhất phát hiện ra điều đó trước khi người dùng làm.
#[test]
fn truy_van_cua_moi_ngon_ngu_deu_bien_dich_duoc() {
    Extractor::new().expect("mọi truy vấn trong bảng ngôn ngữ phải biên dịch được");
}

#[tokio::test]
async fn trich_dung_ky_hieu_tu_mot_tep_rust() {
    let (dir, index) = bench(&[("kho.rs", RUST_SOURCE)]);
    index.sync().await.expect("quét được");

    let path = dir.path().canonicalize().unwrap().join("kho.rs");
    let symbols = index
        .outline(&path)
        .await
        .expect("tra được")
        .expect("tệp phải nằm trong chỉ mục");

    assert_eq!(find(&symbols, "KhoLuu").kind, SymbolKind::Type);
    assert_eq!(find(&symbols, "DocDuoc").kind, SymbolKind::Trait);
    assert_eq!(find(&symbols, "NGUONG").kind, SymbolKind::Constant);
    assert_eq!(find(&symbols, "tu_do").kind, SymbolKind::Function);

    // Khoảng dòng phải bao trọn khối, không chỉ dòng khai báo.
    let mo = find(&symbols, "mo");
    assert!(mo.end_line > mo.start_line, "{mo:#?}");
    assert!(mo.signature.contains("pub fn mo"), "{mo:#?}");

    // `impl KhoLuu` là scope chứ không phải ký hiệu: `KhoLuu` chỉ được kể **một lần**,
    // nếu không chỉ mục nói dối về số lượng kiểu trong repo.
    assert_eq!(
        symbols
            .iter()
            .filter(|symbol| symbol.name == "KhoLuu")
            .count(),
        1,
        "{symbols:#?}"
    );

    // `doc` khai trong trait, `dong` khai trong impl — cả hai đều phải có cha.
    assert_eq!(find(&symbols, "doc").parent.as_deref(), Some("DocDuoc"));
    assert_eq!(find(&symbols, "dong").parent.as_deref(), Some("KhoLuu"));
    assert_eq!(find(&symbols, "tu_do").parent, None);
}

#[tokio::test]
async fn trich_dung_ky_hieu_tu_mot_tep_typescript() {
    let (dir, index) = bench(&[("so.ts", TS_SOURCE)]);
    index.sync().await.expect("quét được");

    let path = dir.path().canonicalize().unwrap().join("so.ts");
    let symbols = index
        .outline(&path)
        .await
        .unwrap()
        .expect("có trong chỉ mục");

    assert_eq!(find(&symbols, "HopDong").kind, SymbolKind::Trait);
    assert_eq!(find(&symbols, "Nhan").kind, SymbolKind::Type);
    assert_eq!(find(&symbols, "SoDangKy").kind, SymbolKind::Type);
    assert_eq!(find(&symbols, "GIOI_HAN").kind, SymbolKind::Constant);

    // Phương thức của class mang tên class làm cha.
    assert_eq!(find(&symbols, "them").kind, SymbolKind::Function);
    assert_eq!(find(&symbols, "them").parent.as_deref(), Some("SoDangKy"));

    // `export const f = () => {}` khớp cả mẫu hàm lẫn mẫu hằng; thang hạng phải chọn hàm.
    assert_eq!(find(&symbols, "dungSo").kind, SymbolKind::Function);

    // Một hàm không export vẫn là một khai báo người ta đi tìm.
    assert_eq!(find(&symbols, "noiBo").kind, SymbolKind::Function);

    // `private hang: HopDong[]` là trường của class, không phải hằng của module.
    assert!(
        !symbols.iter().any(|symbol| symbol.name == "hang"),
        "{symbols:#?}"
    );
}

/// Bất biến trung tâm của cả crate: quét lại một cây không đổi **không** parse lại gì.
#[tokio::test]
async fn tep_khong_doi_thi_khong_parse_lai() {
    let (dir, index) = bench(&[("a.rs", RUST_SOURCE), ("b.ts", TS_SOURCE)]);
    let root = dir.path().canonicalize().unwrap();

    let first = index.sync().await.expect("quét lần đầu");
    assert_eq!(first.parsed, 2);
    assert_eq!(index.parse_count(), 2);

    let second = index.sync().await.expect("quét lần hai");
    assert_eq!(second.scanned, 2, "vẫn phải *nhìn thấy* đủ hai tệp");
    assert_eq!(second.parsed, 0, "nhưng không được parse lại tệp nào");
    assert_eq!(index.parse_count(), 2);

    // Sửa một tệp thì đúng một tệp được parse lại, không phải cả hai.
    std::fs::write(
        root.join("a.rs"),
        format!("{RUST_SOURCE}\npub fn them_moi() {{}}\n"),
    )
    .unwrap();
    let third = index.sync().await.expect("quét lần ba");
    assert_eq!(third.parsed, 1);
    assert_eq!(index.parse_count(), 3);

    let hits = index.search("them_moi", None, 10).await.unwrap();
    assert_eq!(hits.len(), 1, "{hits:#?}");
}

#[tokio::test]
async fn tep_bi_xoa_thi_ky_hieu_cua_no_bien_mat() {
    let (dir, index) = bench(&[("kho.rs", RUST_SOURCE)]);
    let root = dir.path().canonicalize().unwrap();
    index.sync().await.unwrap();
    assert!(!index.search("KhoLuu", None, 10).await.unwrap().is_empty());

    std::fs::remove_file(root.join("kho.rs")).unwrap();
    let report = index.sync().await.expect("quét sau khi xoá");
    assert_eq!(report.forgotten, 1);

    assert!(
        index.search("KhoLuu", None, 10).await.unwrap().is_empty(),
        "ký hiệu của một tệp đã xoá vẫn còn trong chỉ mục"
    );
    // Và hàng FTS cũng phải đi theo, không chỉ hàng `symbols`.
    assert_eq!(index.symbol_count().unwrap(), 0);
    assert!(index.outline(&root.join("kho.rs")).await.unwrap().is_none());
}

#[tokio::test]
async fn symbol_search_tim_duoc_ky_hieu_long_nhau_kem_dung_cha() {
    let (_dir, index) = bench(&[("kho.rs", RUST_SOURCE), ("so.ts", TS_SOURCE)]);
    index.sync().await.unwrap();

    let hits = index.search("dong", None, 10).await.unwrap();
    let dong = find(&hits, "dong");
    assert_eq!(dong.parent.as_deref(), Some("KhoLuu"));
    assert_eq!(dong.qualified(), "KhoLuu::dong");
    assert!(dong.path.ends_with("kho.rs"), "{dong:#?}");
    assert!(dong.start_line > 1);

    // Lọc theo loại phải cắt đúng, không cắt nhầm.
    let types = index
        .search("KhoLuu", Some(SymbolKind::Type), 10)
        .await
        .unwrap();
    assert_eq!(types.len(), 1);
    let traits = index
        .search("KhoLuu", Some(SymbolKind::Trait), 10)
        .await
        .unwrap();
    assert!(traits.is_empty());

    // Hỏi bằng nửa sau của một tên là chuyện người ta làm suốt, và FTS5 một mình không
    // trả lời được — lượt `LIKE` tồn tại vì chỗ này.
    let giua = index.search("Dang", None, 10).await.unwrap();
    assert!(
        giua.iter().any(|symbol| symbol.name == "SoDangKy"),
        "{giua:#?}"
    );
}

#[tokio::test]
async fn tep_cu_phap_hong_khong_lam_hong_ca_lan_quet() {
    let (_dir, index) = bench(&[
        ("hong.rs", "pub fn (( { không phải Rust ]]] impl"),
        ("hong.ts", "class {{{ ) => interface"),
        ("lanh.rs", RUST_SOURCE),
    ]);

    let report = index
        .sync()
        .await
        .expect("một tệp hỏng không được làm gãy lần quét");
    assert_eq!(report.scanned, 3);

    // Và tệp lành vẫn phải vào chỉ mục đầy đủ.
    let hits = index.search("KhoLuu", None, 10).await.unwrap();
    assert_eq!(
        hits[0].name, "KhoLuu",
        "khớp tên phải đứng trước khớp chữ ký: {hits:#?}"
    );
    assert!(
        hits.iter().all(|symbol| symbol.path.ends_with("lanh.rs")),
        "{hits:#?}"
    );
    assert_eq!(
        hits.iter()
            .filter(|symbol| symbol.kind == SymbolKind::Type)
            .count(),
        1,
        "{hits:#?}"
    );
}

#[tokio::test]
async fn gitignore_duoc_ton_trong_ke_ca_khi_chua_git_init() {
    let (_dir, index) = bench(&[
        (".gitignore", "sinh/\n"),
        ("sinh/may.rs", RUST_SOURCE),
        ("that.rs", "pub fn that() {}"),
    ]);
    index.sync().await.unwrap();

    assert_eq!(
        index.search("KhoLuu", None, 10).await.unwrap().len(),
        0,
        "thư mục bị .gitignore loại trừ vẫn lọt vào chỉ mục"
    );
    assert_eq!(index.search("that", None, 10).await.unwrap().len(), 1);
}

/// Luật "chuẩn hoá trước, kiểm tra sau" áp cho đường dẫn của `outline` y hệt như cho
/// `read`: một chỉ mục kể được cấu trúc của tệp ngoài gốc là một đường vòng quanh gốc.
#[tokio::test]
async fn outline_khong_ra_khoi_goc_va_phat_dung_hinh_dang_meta() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("kho.rs"), RUST_SOURCE).unwrap();
    let ngoai = TempDir::new().unwrap();
    let ngoai_file = ngoai.path().join("ngoai.rs");
    std::fs::write(&ngoai_file, RUST_SOURCE).unwrap();

    let roots = FileRoots::new([root.clone()], []);
    let index: Arc<dyn SymbolIndex> = Arc::new(CodeIndex::in_memory(roots.clone()).unwrap());
    let outline = Outline::new(index.clone(), roots);

    let escape = outline
        .execute(&call(
            "outline",
            json!({ "file_path": ngoai_file.display().to_string() }),
        ))
        .await;
    assert!(escape.is_err(), "tệp ngoài gốc không được xem cấu trúc");

    let ok = outline
        .execute(&call(
            "outline",
            json!({ "file_path": root.join("kho.rs").display().to_string() }),
        ))
        .await
        .expect("tệp trong gốc thì được");
    assert!(ok.content.contains("KhoLuu"), "{}", ok.content);
    // Thụt đầu dòng là cách duy nhất người đọc thấy `dong` nằm trong `KhoLuu`.
    assert!(ok.content.contains("\n  "), "{}", ok.content);

    let meta = ok.meta.get("search").expect("phải có meta.search");
    assert_eq!(meta["shape"], "matches");
    assert_eq!(meta["truncated"], false);
    assert!(meta["groups"][0]["matches"][0]["line"].is_number());
    assert!(meta["groups"][0]["matches"][0]["text"].is_string());
}

/// Tên ký hiệu do người dùng đặt, nên chúng là dữ liệu chứ không phải chỉ dẫn.
#[test]
fn ca_hai_tool_deu_chi_doc_va_khong_dang_tin() {
    let roots = FileRoots::new([std::env::temp_dir()], []);
    let index: Arc<dyn SymbolIndex> = Arc::new(CodeIndex::in_memory(roots.clone()).unwrap());
    for meta in [
        SymbolSearch::new(index.clone()).meta(),
        Outline::new(index, roots).meta(),
    ] {
        assert_eq!(
            meta,
            ToolMeta::read_only().untrusted().concurrency_safe(true)
        );
        assert!(!meta.mutating);
        assert!(meta.returns_untrusted_content);
        assert!(!meta.leaves_device);
    }
}

/// Đường dẫn lưu trong chỉ mục phải là đường đã phân giải, y hệt thứ `read` nhận vào —
/// nếu không, mô hình chép số dòng sang một đường dẫn mà `read` từ chối.
#[tokio::test]
async fn duong_dan_tra_ve_dung_bang_duong_ma_read_nhan() {
    let (dir, index) = bench(&[("con/kho.rs", RUST_SOURCE)]);
    index.sync().await.unwrap();
    let roots = FileRoots::new([dir.path().canonicalize().unwrap()], []);

    let hits = index.search("KhoLuu", None, 5).await.unwrap();
    let resolved = roots
        .resolve_read(Path::new(&hits[0].path))
        .expect("đường dẫn trong chỉ mục phải qua được chính bộ lọc của hệ tệp");
    assert_eq!(resolved.display().to_string(), hits[0].path);
}

#[tokio::test]
async fn trich_dung_ky_hieu_tu_mot_tep_python() {
    let (dir, index) = bench(&[("kho.py", PY_SOURCE)]);
    index.sync().await.expect("quét được");

    let path = dir.path().canonicalize().unwrap().join("kho.py");
    let symbols = index
        .outline(&path)
        .await
        .unwrap()
        .expect("có trong chỉ mục");

    assert_eq!(find(&symbols, "KhoLuu").kind, SymbolKind::Type);
    assert_eq!(find(&symbols, "NGUONG").kind, SymbolKind::Constant);
    assert_eq!(find(&symbols, "mo").parent.as_deref(), Some("KhoLuu"));
    assert_eq!(find(&symbols, "tu_do").parent, None);
}

/// Chỉ mục sống qua lần khởi động sau. Không có bài này thì "tăng dần" chỉ đúng trong
/// một phiên, và mở lại ứng dụng là parse lại cả repo — đúng cái giá mà cả crate này
/// sinh ra để khỏi phải trả.
#[tokio::test]
async fn chi_muc_tren_dia_song_qua_lan_mo_lai() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("kho.rs"), RUST_SOURCE).unwrap();
    let kho = TempDir::new().unwrap();
    let db = kho.path().join("con").join("chi-muc.sqlite");
    let roots = FileRoots::new([root], []);

    let first = CodeIndex::open(roots.clone(), &db).expect("mở kho trên đĩa");
    assert_eq!(first.sync().await.unwrap().parsed, 1);
    drop(first);

    let second = CodeIndex::open(roots, &db).expect("mở lại kho cũ");
    let report = second.sync().await.expect("quét lại");
    assert_eq!(report.scanned, 1);
    assert_eq!(
        report.parsed, 0,
        "mở lại không được parse lại tệp không đổi"
    );
    assert_eq!(second.parse_count(), 0);
    assert!(!second.search("KhoLuu", None, 5).await.unwrap().is_empty());
}

/// Đường vào thật của sản phẩm là plugin, không phải `CodeIndex::new`. Bài này giữ cho
/// nó chạy được: dựng cây, cắm chỉ mục, và thấy đúng hai tool xuất hiện trong sổ đăng ký.
#[tokio::test]
async fn plugin_cam_dung_hai_tool_va_mot_provider() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("kho.rs"), RUST_SOURCE).unwrap();
    let kho = TempDir::new().unwrap();

    let ctx = Context::root();
    ToolsPlugin.apply(&ctx.plugin("tools")).await.unwrap();
    IndexPlugin::new([root], [], kho.path().to_path_buf())
        .apply(&ctx.plugin("index"))
        .await
        .expect("cắm được chỉ mục");

    let names: Vec<String> = ctx
        .require::<Tools>()
        .unwrap()
        .schemas(None)
        .into_iter()
        .map(|schema| schema.name.to_string())
        .collect();
    assert!(
        names.iter().any(|name| name == "symbol_search"),
        "{names:?}"
    );
    assert!(names.iter().any(|name| name == "outline"), "{names:?}");

    // Tên tệp chỉ mục được suy từ thư mục làm việc, nên hai workspace không dùng chung
    // một kho — triệu chứng của việc đó là kết quả của một dự án khác.
    let files: Vec<String> = std::fs::read_dir(kho.path())
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().to_string()))
        .filter(|name| name.ends_with(".sqlite"))
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");

    // Và seam phải dùng được từ ngoài, không chỉ từ hai tool bên trong.
    let index = ctx.require::<Index>().unwrap();
    index.sync().await.unwrap();
    assert!(!index.search("KhoLuu", None, 5).await.unwrap().is_empty());
}
