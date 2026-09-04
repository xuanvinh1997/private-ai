//! Invariants whose failure sends the model to the wrong file.
//! Each test locks down one sentence of the crate docs: a red test means either the code
//! is wrong or that sentence is no longer true, and there is no third option.

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

/// A grammar/core ABI mismatch fails at runtime, not compile time; this test is the only thing that catches it first.
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

    // The line range must span the whole block, not just the declaration line.
    let mo = find(&symbols, "mo");
    assert!(mo.end_line > mo.start_line, "{mo:#?}");
    assert!(mo.signature.contains("pub fn mo"), "{mo:#?}");

    // `impl KhoLuu` is a scope, not a symbol: `KhoLuu` is counted once, or the index misreports the type count.
    assert_eq!(
        symbols
            .iter()
            .filter(|symbol| symbol.name == "KhoLuu")
            .count(),
        1,
        "{symbols:#?}"
    );

    // `doc` is declared in the trait and `dong` in the impl; both must have a parent.
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

    // A class method takes the class name as its parent.
    assert_eq!(find(&symbols, "them").kind, SymbolKind::Function);
    assert_eq!(find(&symbols, "them").parent.as_deref(), Some("SoDangKy"));

    // `export const f = () => {}` matches both patterns; the ranking must pick the function.
    assert_eq!(find(&symbols, "dungSo").kind, SymbolKind::Function);

    // A non-exported function is still a declaration people search for.
    assert_eq!(find(&symbols, "noiBo").kind, SymbolKind::Function);

    // `private hang: HopDong[]` is a class field, not a module constant.
    assert!(
        !symbols.iter().any(|symbol| symbol.name == "hang"),
        "{symbols:#?}"
    );
}

#[tokio::test]
async fn namespace_typescript_lam_cha_cua_ham_va_hang() {
    let (dir, index) = bench(&[(
        "namespace.ts",
        "namespace N { export function f() {} export const HANG = 1; }\n",
    )]);
    index.sync().await.unwrap();
    let path = dir.path().canonicalize().unwrap().join("namespace.ts");
    let symbols = index.outline(&path).await.unwrap().unwrap();

    assert_eq!(find(&symbols, "f").parent.as_deref(), Some("N"));
    assert_eq!(find(&symbols, "HANG").parent.as_deref(), Some("N"));
}

/// The crate's central invariant: rescanning an unchanged tree parses nothing.
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

    // Editing one file re-parses exactly one file, not both.
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
    // The FTS rows must follow too, not only the `symbols` rows.
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

    // The kind filter must cut the right rows and no others.
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

    // Searching by the tail of a name is common and FTS5 alone cannot do it; the `LIKE` pass exists for this.
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

    // The healthy file must still be indexed in full.
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

#[tokio::test]
async fn completion_co_ca_tep_khong_thuoc_ngon_ngu_parse_duoc() {
    let (_dir, index) = bench(&[
        ("src/main.rs", "fn main() {}\n"),
        ("README.md", "# Chao\n"),
        ("Cargo.toml", "[package]\nname = \"demo\"\n"),
        ("config/app.yaml", "enabled: true\n"),
    ]);
    index.sync().await.unwrap();

    assert_eq!(index.paths("readme", 5).await.unwrap().len(), 1);
    assert_eq!(index.paths("cargo", 5).await.unwrap().len(), 1);
    assert_eq!(index.paths("yaml", 5).await.unwrap().len(), 1);
    assert_eq!(
        index.stats().await.unwrap().files,
        1,
        "chỉ tệp Rust được parse"
    );
}

/// `outline` resolves then checks its path exactly as `read` does; otherwise the index bypasses the roots.
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
    // Indentation is the only way a reader sees that `dong` sits inside `KhoLuu`.
    assert!(ok.content.contains("\n  "), "{}", ok.content);

    let meta = ok.meta.get("search").expect("phải có meta.search");
    assert_eq!(meta["shape"], "matches");
    assert_eq!(meta["truncated"], false);
    assert!(meta["groups"][0]["matches"][0]["line"].is_number());
    assert!(meta["groups"][0]["matches"][0]["text"].is_string());
}

/// Symbol names are user-authored, so they are data rather than instructions.
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

/// Stored paths must be resolved, exactly what `read` accepts, or the model copies line numbers to a rejected path.
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

#[tokio::test]
async fn hang_trong_class_python_co_ky_hieu_va_dung_cha() {
    let (dir, index) = bench(&[("constant.py", "class A:\n    HANG = 1\n")]);
    index.sync().await.unwrap();
    let path = dir.path().canonicalize().unwrap().join("constant.py");
    let symbols = index.outline(&path).await.unwrap().unwrap();

    let constant = find(&symbols, "HANG");
    assert_eq!(constant.kind, SymbolKind::Constant);
    assert_eq!(constant.parent.as_deref(), Some("A"));
}

/// The index survives a restart; without this, "incremental" holds only within one session.
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

/// The real entry point is the plugin, not `CodeIndex::new`: build the tree, mount it, see all five tools registered.
#[tokio::test]
async fn plugin_cam_dung_nam_tool_va_mot_provider() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("kho.rs"), RUST_SOURCE).unwrap();
    std::fs::write(root.join("README.md"), "# du an\n").unwrap();
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
    for wanted in [
        "symbol_search",
        "outline",
        "code.graph",
        "code.trace",
        "code.overview",
    ] {
        assert!(names.iter().any(|name| name == wanted), "{names:?}");
    }

    // The index filename is derived from the working directory, so two workspaces never share one store.
    let files: Vec<String> = std::fs::read_dir(kho.path())
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().to_string()))
        .filter(|name| name.ends_with(".sqlite"))
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");

    // The seam must also be usable from outside, not only by the two tools within.
    let index = ctx.require::<Index>().unwrap();
    assert_eq!(
        index.paths("readme", 5).await.unwrap().len(),
        1,
        "path inventory phải sẵn ngay khi plugin vừa gắn"
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if !index.search("KhoLuu", None, 5).await.unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("background sync phải làm ký hiệu sẵn sàng");
}
