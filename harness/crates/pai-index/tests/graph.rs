//! Invariants of the code graph.
//! A wrong graph answers "nobody calls this" with an empty list that looks like the truth,
//! so every test here asserts one specific edge rather than a count of edges.

use std::path::Path;
use std::sync::Arc;

use pai_core::{Context, Plugin};
use pai_fs::FileRoots;
use pai_index::index::SymbolIndex;
use pai_index::tools::graph::CodeGraph;
use pai_index::tools::overview::CodeOverview;
use pai_index::tools::trace::CodeTrace;
use pai_index::{CodeIndex, EdgeKind, GraphNode, IndexPlugin, MAX_DEPTH, MAX_NODES};
use pai_tools::{Invocation, Resolution, Tool, ToolMeta, ToolName, Tools, ToolsPlugin};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

const A_RS: &str = r#"
pub fn main() -> usize {
    helper(1)
}

fn helper(x: usize) -> usize {
    x
}
"#;

const B_TS: &str = r#"
export class A {
  chay(): void {}
}

export class B extends A {
  chay(): void {
    tinh();
  }
}

export function tinh(): number {
  return 1;
}
"#;

const C_PY: &str = r#"
class A:
    pass


class B(A):
    def chay(self):
        return tinh()


def tinh():
    return 1
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

type Edges = Vec<(GraphNode, EdgeKind, GraphNode)>;

/// Whether the edge `src -kind-> dst` is present, matched by name.
fn has(edges: &Edges, src: &str, kind: EdgeKind, dst: &str) -> bool {
    edges
        .iter()
        .any(|(a, k, b)| a.name == src && *k == kind && b.name == dst)
}

fn count(edges: &Edges, src: &str, kind: EdgeKind, dst: &str) -> usize {
    edges
        .iter()
        .filter(|(a, k, b)| a.name == src && *k == kind && b.name == dst)
        .count()
}

fn call(name: &str, args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from(name), "c1", map)
}

/// The central test: exactly these edges, in exactly these three languages.
#[tokio::test]
async fn trich_dung_cac_canh_cu_the_o_ba_ngon_ngu() {
    let (dir, index) = bench(&[("a.rs", A_RS), ("b.ts", B_TS), ("c.py", C_PY)]);
    index.sync().await.expect("quét được");
    let root = dir.path().canonicalize().unwrap();

    let rust = index.edges_of_file(&root.join("a.rs")).unwrap();
    assert!(
        has(&rust, "main", EdgeKind::Calls, "helper"),
        "thiếu `main —calls→ helper`: {rust:#?}"
    );
    // Module contains function; the module node is named for the file stem and never leaks to `symbol_search`.
    assert!(
        has(&rust, "a", EdgeKind::Contains, "main"),
        "thiếu `a —contains→ main`: {rust:#?}"
    );
    assert!(has(&rust, "a", EdgeKind::Contains, "helper"), "{rust:#?}");
    // `helper(x: usize) -> usize` names no repo type, so there is no spurious edge.
    assert_eq!(rust.len(), 3, "{rust:#?}");

    let ts = index.edges_of_file(&root.join("b.ts")).unwrap();
    assert!(
        has(&ts, "B", EdgeKind::Extends, "A"),
        "thiếu `class B —extends→ class A`: {ts:#?}"
    );
    assert!(
        has(&ts, "B", EdgeKind::Contains, "chay"),
        "class chứa method: {ts:#?}"
    );
    // A method of `B` calls a same-file function; the owner must be `chay`, not `B`.
    assert!(has(&ts, "chay", EdgeKind::Calls, "tinh"), "{ts:#?}");
    assert!(
        !has(&ts, "B", EdgeKind::Calls, "tinh"),
        "chủ nhà của lời gọi phải là ký hiệu trong cùng nhất: {ts:#?}"
    );

    let py = index.edges_of_file(&root.join("c.py")).unwrap();
    assert!(
        has(&py, "B", EdgeKind::Extends, "A"),
        "thiếu `class B(A)`: {py:#?}"
    );
    assert!(has(&py, "B", EdgeKind::Contains, "chay"), "{py:#?}");
    assert!(has(&py, "chay", EdgeKind::Calls, "tinh"), "{py:#?}");
    assert!(has(&py, "c", EdgeKind::Contains, "tinh"), "{py:#?}");

    // `A` and `B` exist in both `b.ts` and `c.py`; the same-file tier must win, or Python extends TypeScript.
    let (_, _, cha) = py
        .iter()
        .find(|(a, k, _)| a.name == "B" && *k == EdgeKind::Extends)
        .expect("phải có cạnh extends trong c.py");
    assert!(cha.path.ends_with("c.py"), "{cha:#?}");
    assert_eq!(count(&py, "B", EdgeKind::Extends, "A"), 1, "{py:#?}");
}

/// Rescanning an edited file must drop all its old edges and touch no other file, or the graph fills with junk.
#[tokio::test]
async fn quet_lai_mot_tep_don_sach_canh_cu_cua_dung_tep_do() {
    let (dir, index) = bench(&[
        ("alpha.rs", "pub fn a1() { a2(); }\npub fn a2() {}\n"),
        ("beta.rs", "pub fn b1() { b2(); }\npub fn b2() {}\n"),
    ]);
    let root = dir.path().canonicalize().unwrap();
    index.sync().await.unwrap();

    let alpha = root.join("alpha.rs");
    let beta = root.join("beta.rs");
    // Two `contains` edges from the module node and one `calls`; count rows, not presence.
    assert_eq!(index.edges_of_file(&alpha).unwrap().len(), 3);
    assert_eq!(index.edges_of_file(&beta).unwrap().len(), 3);
    assert!(has(
        &index.edges_of_file(&alpha).unwrap(),
        "a1",
        EdgeKind::Calls,
        "a2"
    ));
    let tong = index.edge_count().unwrap();
    assert_eq!(tong, 6);

    std::fs::write(&alpha, "pub fn a3() {}\n").unwrap();
    let report = index.sync().await.expect("quét lại");
    assert_eq!(report.parsed, 1, "chỉ một tệp được parse lại");

    let sau = index.edges_of_file(&alpha).unwrap();
    assert_eq!(sau.len(), 1, "{sau:#?}");
    assert!(has(&sau, "alpha", EdgeKind::Contains, "a3"), "{sau:#?}");
    assert!(
        !has(&sau, "a1", EdgeKind::Calls, "a2"),
        "cạnh của phiên bản cũ vẫn còn: {sau:#?}"
    );

    let khac = index.edges_of_file(&beta).unwrap();
    assert_eq!(khac.len(), 3, "cạnh của tệp khác bị đụng: {khac:#?}");
    assert!(has(&khac, "b1", EdgeKind::Calls, "b2"), "{khac:#?}");
    assert_eq!(index.edge_count().unwrap(), 4);
}

/// A newly appearing file must connect to edges left pending by an earlier scan.
#[tokio::test]
async fn canh_lien_tep_xuat_hien_khi_dich_cua_no_duoc_them_vao() {
    let (dir, index) = bench(&[("goi.rs", "pub fn goi() { dich(); }\n")]);
    let root = dir.path().canonicalize().unwrap();
    index.sync().await.unwrap();
    let goi = root.join("goi.rs");
    // `dich` exists nowhere yet, so the reference to it is not an edge.
    assert!(!has(
        &index.edges_of_file(&goi).unwrap(),
        "goi",
        EdgeKind::Calls,
        "dich"
    ));

    std::fs::write(root.join("dich.rs"), "pub fn dich() {}\n").unwrap();
    index.sync().await.unwrap();
    assert!(
        has(
            &index.edges_of_file(&goi).unwrap(),
            "goi",
            EdgeKind::Calls,
            "dich"
        ),
        "cạnh liên tệp phải xuất hiện mà không cần sửa lại tệp gọi"
    );
}

/// The name-resolution decision, locked down: same file always wins, ties write every candidate, over the cap writes none.
#[tokio::test]
async fn ten_trung_o_nhieu_tep_theo_dung_bac_uu_tien() {
    let (dir, index) = bench(&[
        (
            "mot/goi.rs",
            "pub fn goi() { rieng(); nhieu(); }\npub fn cuc_bo() { rieng(); }\npub fn rieng() {}\n",
        ),
        ("mot/x1.rs", "pub fn rieng() {}\n"),
        ("mot/x2.rs", "pub fn rieng() {}\n"),
        ("hai/y1.rs", "pub fn nhieu() {}\n"),
        ("hai/y2.rs", "pub fn nhieu() {}\n"),
        ("hai/y3.rs", "pub fn nhieu() {}\n"),
        ("hai/y4.rs", "pub fn nhieu() {}\n"),
        ("hai/y5.rs", "pub fn nhieu() {}\n"),
        ("ba/xa.rs", "pub fn xa() { rieng(); }\n"),
    ]);
    let root = dir.path().canonicalize().unwrap();
    index.sync().await.unwrap();

    // Tier 1, same file: `rieng` has three declarations, but the local one wins and only that edge is written.
    let mot = index.edges_of_file(&root.join("mot/goi.rs")).unwrap();
    assert_eq!(count(&mot, "goi", EdgeKind::Calls, "rieng"), 1, "{mot:#?}");
    let (_, _, dich) = mot
        .iter()
        .find(|(a, k, b)| a.name == "goi" && *k == EdgeKind::Calls && b.name == "rieng")
        .expect("phải có cạnh");
    assert!(dich.path.ends_with("goi.rs"), "{dich:#?}");
    assert_eq!(count(&mot, "cuc_bo", EdgeKind::Calls, "rieng"), 1);

    // With no same-directory match, `ba/xa.rs` falls to the whole-store tier and all three candidates are written.
    let ba = index.edges_of_file(&root.join("ba/xa.rs")).unwrap();
    assert_eq!(count(&ba, "xa", EdgeKind::Calls, "rieng"), 3, "{ba:#?}");

    // Past four candidates the reference is dropped: five edges from one call site narrow nothing.
    assert_eq!(count(&mot, "goi", EdgeKind::Calls, "nhieu"), 0, "{mot:#?}");
}

/// `depth` and `limit` are hard ceilings, and being cut has to be reported.
#[tokio::test]
async fn lan_can_bi_chan_o_tran_va_noi_ra_rang_da_cat() {
    let (_dir, index) = bench(&[("a.rs", A_RS), ("b.ts", B_TS), ("c.py", C_PY)]);
    index.sync().await.unwrap();

    let rong = index.neighborhood("main", 50, 60).await.unwrap();
    assert!(!rong.nodes.is_empty(), "phải tìm thấy `main`");
    assert!(
        rong.truncated,
        "xin sâu 50 mà không nói là đã cắt xuống {MAX_DEPTH}"
    );

    let nhieu = index.neighborhood("main", 1, MAX_NODES + 1).await.unwrap();
    assert!(nhieu.truncated, "xin quá trần số đỉnh cũng là một lần cắt");

    // Within the caps nothing may claim truncation, or the model learns to ignore the flag.
    let vua = index.neighborhood("main", 1, 60).await.unwrap();
    assert!(!vua.truncated, "{vua:#?}");
    assert!(
        vua.nodes.iter().any(|node| node.name == "helper"),
        "{vua:#?}"
    );
    assert!(
        vua.edges
            .iter()
            .all(|edge| vua.nodes.iter().any(|node| node.id == edge.src)
                && vua.nodes.iter().any(|node| node.id == edge.dst)),
        "một cạnh có đầu nằm ngoài tập đỉnh là một cạnh không vẽ được"
    );

    // An unknown name returns empty, not somebody else's slice.
    let khong = index.neighborhood("khong_ton_tai", 2, 60).await.unwrap();
    assert!(khong.nodes.is_empty() && khong.edges.is_empty());
    assert!(!khong.truncated);
}

#[tokio::test]
async fn truy_vet_tra_ve_duong_di_ca_hai_chieu() {
    let (_dir, index) = bench(&[(
        "chuoi.rs",
        "pub fn mot() { hai(); }\npub fn hai() { ba(); }\npub fn ba() {}\n",
    )]);
    index.sync().await.unwrap();

    let xuoi = index.callees("mot", 3).await.unwrap();
    let ten: Vec<Vec<&str>> = xuoi
        .iter()
        .map(|path| path.iter().map(|node| node.name.as_str()).collect())
        .collect();
    assert_eq!(ten, vec![vec!["mot", "hai", "ba"]], "{ten:?}");

    let nguoc = index.callers("ba", 3).await.unwrap();
    let ten: Vec<Vec<&str>> = nguoc
        .iter()
        .map(|path| path.iter().map(|node| node.name.as_str()).collect())
        .collect();
    assert_eq!(ten, vec![vec!["ba", "hai", "mot"]], "{ten:?}");

    // The depth ceiling shortens paths rather than lengthening them.
    let ngan = index.callees("mot", 1).await.unwrap();
    assert_eq!(ngan.len(), 1);
    assert_eq!(ngan[0].len(), 2, "{ngan:#?}");
}

#[tokio::test]
async fn ban_do_kien_truc_dem_dung_va_khong_ke_dinh_module() {
    let (_dir, index) = bench(&[("a.rs", A_RS), ("b.ts", B_TS), ("c.py", C_PY)]);
    index.sync().await.unwrap();

    let stats = index.stats().await.unwrap();
    assert_eq!(stats.files, 3);
    assert!(stats.scanned_at.is_some());
    assert_eq!(stats.symbols as i64, index.symbol_count().unwrap());
    assert_eq!(stats.edges as i64, index.edge_count().unwrap());
    assert_eq!(stats.languages.len(), 3, "{:?}", stats.languages);

    let map = index.overview().await.unwrap();
    assert_eq!(map.directories.len(), 1);
    assert_eq!(map.directories[0].files, 3);
    assert_eq!(map.directories[0].symbols, stats.symbols);
    assert!(
        map.central
            .iter()
            .all(|central| central.node.kind != "module"),
        "đỉnh module không phải một khai báo người ta đi đọc: {:#?}",
        map.central
    );
    assert!(
        map.central.iter().any(|central| central.node.name == "A"),
        "{:#?}",
        map.central
    );
}

/// Module nodes live only in the graph; leaking into `symbol_search` or `outline` misreports the declaration count.
#[tokio::test]
async fn dinh_module_khong_lot_ra_chi_muc_ky_hieu() {
    let (dir, index) = bench(&[("a.rs", A_RS)]);
    index.sync().await.unwrap();
    let root = dir.path().canonicalize().unwrap();

    // `a` is the module node of `a.rs`, the only name here that could leak, so its absence proves the filter.
    let hits = index.search("a", None, 20).await.unwrap();
    assert!(
        hits.iter().all(|symbol| symbol.name != "a"),
        "đỉnh module lọt ra `symbol_search`: {hits:#?}"
    );
    let outline = index.outline(&root.join("a.rs")).await.unwrap().unwrap();
    assert_eq!(outline.len(), 2, "{outline:#?}");
    assert_eq!(index.symbol_count().unwrap(), 2);
}

/// The three new tools must go through the real registry, `__` name decoding included, and carry the name-based notice.
#[tokio::test]
async fn ba_tool_do_thi_dang_ky_duoc_va_noi_ra_rang_canh_la_suy_doan() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), A_RS).unwrap();
    let kho = TempDir::new().unwrap();

    let ctx = Context::root();
    ToolsPlugin.apply(&ctx.plugin("tools")).await.unwrap();
    IndexPlugin::new([root], [], kho.path().to_path_buf())
        .apply(&ctx.plugin("index"))
        .await
        .expect("cắm được chỉ mục");
    let tools = ctx.require::<Tools>().unwrap();

    let names: Vec<String> = tools
        .schemas(None)
        .into_iter()
        .map(|schema| schema.name.to_string())
        .collect();
    for wanted in ["code.graph", "code.trace", "code.overview"] {
        assert!(names.iter().any(|name| name == wanted), "{names:?}");
    }

    // Names reach the registry in wire form, exactly as the model sends them.
    for (wire, args) in [
        ("code__graph", json!({ "symbol": "main" })),
        (
            "code__trace",
            json!({ "symbol": "helper", "direction": "callers" }),
        ),
        ("code__overview", json!({})),
    ] {
        let Resolution::Found(tool, name) = tools.resolve(None, wire) else {
            panic!("{wire} không phân giải được qua sổ đăng ký");
        };
        let out = tool
            .execute(&call(name.as_str(), args))
            .await
            .unwrap_or_else(|err| panic!("{wire} hỏng: {err}"));
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("suy đoán theo tên"),
            "{wire} không nói ra rằng cạnh là phỏng đoán:\n{}",
            out.content
        );
    }
}

#[tokio::test]
async fn ba_tool_do_thi_deu_chi_doc_va_khong_dang_tin() {
    let roots = FileRoots::new([std::env::temp_dir()], []);
    let index: Arc<dyn SymbolIndex> = Arc::new(CodeIndex::in_memory(roots).unwrap());
    for meta in [
        CodeGraph::new(index.clone()).meta(),
        CodeTrace::new(index.clone()).meta(),
        CodeOverview::new(index).meta(),
    ] {
        assert_eq!(
            meta,
            ToolMeta::read_only().untrusted().concurrency_safe(true)
        );
        assert!(!meta.mutating);
        assert!(meta.returns_untrusted_content);
    }
}

/// A measurement over this repo; not a speed assertion, just where the number gets printed.
#[tokio::test]
async fn do_tren_chinh_kho_nay() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("gốc kho");
    let index = CodeIndex::in_memory(FileRoots::new([repo.clone()], [])).unwrap();

    let bat_dau = std::time::Instant::now();
    let report = index.sync().await.expect("quét được kho thật");
    let mat = bat_dau.elapsed();
    let stats = index.stats().await.unwrap();

    println!(
        "[đo] {} — {} tệp, {} ký hiệu, {} cạnh, {} ms ({:?})",
        repo.display(),
        report.scanned,
        stats.symbols,
        stats.edges,
        mat.as_millis(),
        stats.languages
    );

    let lai = std::time::Instant::now();
    let hai = index.sync().await.unwrap();
    println!(
        "[đo] quét lại không đổi: {} tệp, parse {}, {} ms",
        hai.scanned,
        hai.parsed,
        lai.elapsed().as_millis()
    );
    // Deliberately no `hai.parsed == 0` here: this runs on the live repo, where anything touching a file makes it flaky.
    assert!(report.scanned > 100, "{report:?}");
    assert!(stats.edges > 0);

    // `crates/` alone, the only figure comparable with the earlier flat indexer's number.
    let chi_crates = repo.join("crates");
    let hep = CodeIndex::in_memory(FileRoots::new([chi_crates], [])).unwrap();
    let bat_dau = std::time::Instant::now();
    let rieng = hep.sync().await.unwrap();
    let mat = bat_dau.elapsed();
    let so = hep.stats().await.unwrap();
    println!(
        "[đo] chỉ crates/ — {} tệp, {} ký hiệu, {} cạnh, {} ms",
        rieng.scanned,
        so.symbols,
        so.edges,
        mat.as_millis()
    );
}
