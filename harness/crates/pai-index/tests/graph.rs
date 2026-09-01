//! Những bất biến của đồ thị mã nguồn.
//!
//! Một chỉ mục ký hiệu sai thì mô hình đọc nhầm chỗ và tự nhận ra. Một **đồ thị** sai thì
//! tệ hơn: nó trả lời "không ai gọi hàm này" bằng một danh sách rỗng trông y hệt sự thật,
//! và mô hình xoá hàm đi. Vì thế mỗi bài ở đây khẳng định **một cạnh cụ thể**, chứ không
//! khẳng định "có nhiều hơn không cạnh" — một phép đếm như thế vẫn xanh khi mọi cạnh đều
//! nối sai chỗ.

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

/// Cạnh `src —kind→ dst` có mặt không, so bằng tên.
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

/// Bài trung tâm: đúng những cạnh này, ở đúng ba ngôn ngữ.
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
    // Module chứa hàm. Đỉnh module mang tên thân tệp và không bao giờ lọt ra `symbol_search`.
    assert!(
        has(&rust, "a", EdgeKind::Contains, "main"),
        "thiếu `a —contains→ main`: {rust:#?}"
    );
    assert!(has(&rust, "a", EdgeKind::Contains, "helper"), "{rust:#?}");
    // `helper(x: usize) -> usize` không nhắc tới kiểu nào của kho, nên không có cạnh thừa.
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
    // Phương thức của `B` gọi một hàm cùng tệp; chủ nhà phải là `chay`, không phải `B`.
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

    // `A` và `B` có mặt trong cả `b.ts` lẫn `c.py`. Bậc "cùng tệp" phải thắng, nếu không
    // một lớp Python kế thừa một lớp TypeScript — vô nghĩa, và trông vẫn như một cạnh thật.
    let (_, _, cha) = py
        .iter()
        .find(|(a, k, _)| a.name == "B" && *k == EdgeKind::Extends)
        .expect("phải có cạnh extends trong c.py");
    assert!(cha.path.ends_with("c.py"), "{cha:#?}");
    assert_eq!(count(&py, "B", EdgeKind::Extends, "A"), 1, "{py:#?}");
}

/// Quét lại một tệp đã sửa phải **xoá hết** cạnh cũ của nó, và không đụng tệp khác.
///
/// Không có bài này thì đồ thị lớn dần bằng rác, và rác trong đồ thị trông y hệt sự thật.
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
    // Hai cạnh `contains` từ đỉnh module, một cạnh `calls`. Đếm hàng, không đếm "có".
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

/// Một tệp mới xuất hiện phải nối được vào cạnh đã nằm chờ từ lần quét trước.
#[tokio::test]
async fn canh_lien_tep_xuat_hien_khi_dich_cua_no_duoc_them_vao() {
    let (dir, index) = bench(&[("goi.rs", "pub fn goi() { dich(); }\n")]);
    let root = dir.path().canonicalize().unwrap();
    index.sync().await.unwrap();
    let goi = root.join("goi.rs");
    // `dich` chưa tồn tại ở đâu cả, nên tham chiếu tới nó chưa thành cạnh.
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

/// Quyết định phân giải tên, khoá lại nguyên văn.
///
/// Cùng tệp thắng tuyệt đối; còn nhiều ứng viên trong một bậc thì **ghi cả n**; quá trần
/// thì bỏ hẳn. Đổi bất kỳ vế nào trong ba vế đó cũng phải đổi bài này.
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

    // Bậc 1 — cùng tệp: `rieng` có ba khai báo trong kho, nhưng một trong số đó ở ngay
    // đây, nên đúng **một** cạnh được ghi và nó trỏ vào cái ở đây.
    let mot = index.edges_of_file(&root.join("mot/goi.rs")).unwrap();
    assert_eq!(count(&mot, "goi", EdgeKind::Calls, "rieng"), 1, "{mot:#?}");
    let (_, _, dich) = mot
        .iter()
        .find(|(a, k, b)| a.name == "goi" && *k == EdgeKind::Calls && b.name == "rieng")
        .expect("phải có cạnh");
    assert!(dich.path.ends_with("goi.rs"), "{dich:#?}");
    assert_eq!(count(&mot, "cuc_bo", EdgeKind::Calls, "rieng"), 1);

    // Bậc 2 — cùng thư mục thì không còn; `ba/xa.rs` phải rơi xuống bậc "toàn kho", nơi
    // `rieng` có ba ứng viên. Cả ba được ghi: bỏ hết thì mô hình đọc thành "không ai gọi",
    // và sai theo hướng đó đắt hơn ba cạnh trong đó có một cạnh đúng.
    let ba = index.edges_of_file(&root.join("ba/xa.rs")).unwrap();
    assert_eq!(count(&ba, "xa", EdgeKind::Calls, "rieng"), 3, "{ba:#?}");

    // Quá trần bốn ứng viên thì bỏ hẳn: năm cạnh từ một chỗ gọi không thu hẹp được gì.
    assert_eq!(count(&mot, "goi", EdgeKind::Calls, "nhieu"), 0, "{mot:#?}");
}

/// `depth` và `limit` là trần cứng, và việc bị cắt phải được nói ra.
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

    // Trong trần thì không được kêu bị cắt — nếu không mô hình học cách bỏ qua cờ đó.
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

    // Không có ký hiệu nào tên như thế thì trả về rỗng, không phải một lát cắt của ai đó.
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

    // Trần độ sâu cắt đường đi chứ không kéo dài nó.
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

/// Đỉnh module chỉ sống trong đồ thị. Lọt ra `symbol_search` hay `outline` là chỉ mục nói
/// dối về số lượng khai báo trong kho.
#[tokio::test]
async fn dinh_module_khong_lot_ra_chi_muc_ky_hieu() {
    let (dir, index) = bench(&[("a.rs", A_RS)]);
    index.sync().await.unwrap();
    let root = dir.path().canonicalize().unwrap();

    // `a` là tên đỉnh module của `a.rs`. Nó là cái tên duy nhất ở đây có thể lọt ra, nên
    // không thấy nó trong kết quả nghĩa là bộ lọc đang chặn đúng chỗ.
    let hits = index.search("a", None, 20).await.unwrap();
    assert!(
        hits.iter().all(|symbol| symbol.name != "a"),
        "đỉnh module lọt ra `symbol_search`: {hits:#?}"
    );
    let outline = index.outline(&root.join("a.rs")).await.unwrap().unwrap();
    assert_eq!(outline.len(), 2, "{outline:#?}");
    assert_eq!(index.symbol_count().unwrap(), 2);
}

/// Ba tool mới phải đi qua **sổ đăng ký thật**, kể cả bước giải mã tên `__`, và kết quả
/// phải mang lời cảnh báo rằng cạnh là suy đoán.
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

    // Tên đi tới sổ đăng ký ở dạng wire, đúng như mô hình gửi.
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

/// Số đo trên chính kho này. Không phải một khẳng định về tốc độ — nó là chỗ con số được
/// in ra để người đọc báo cáo so được với lần trước.
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
    // Cố ý **không** khẳng định `hai.parsed == 0` ở đây, dù đó là bất biến trung tâm của
    // crate. Bài này đo trên kho mã đang sống: bất cứ ai — một trình soạn thảo đang mở,
    // một `cargo fmt`, một tiến trình khác — chạm vào một tệp giữa hai lần quét là bài
    // này đỏ vì một lý do không liên quan gì tới thứ nó đo. Bất biến ấy đã được khoá ở
    // `tests/index.rs`, trên một cây trong tempdir mà không ai khác chạm vào; khoá nó hai
    // lần chỉ thêm một nguồn nhiễu.
    assert!(report.scanned > 100, "{report:?}");
    assert!(stats.edges > 0);

    // Riêng `crates/`: đây là cái so được với số của bộ chỉ mục phẳng trước đây, vì lúc
    // đó `app/` và `ui/` chưa nằm trong phép đo.
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
