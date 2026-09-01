//! Chỉ mục: seam, và bản cài đặt chạy trên đĩa của chính máy này.
//!
//! Bất biến của tệp này gọn trong một câu: **một tệp không đổi thì không được parse
//! lại.** Mọi thứ khác — bảng ngôn ngữ, truy vấn, FTS5 — chỉ quyết định chỉ mục *tốt* đến
//! đâu; câu đó quyết định nó có được bật hay không. Một lần quét lại toàn repo cho mỗi
//! câu hỏi là thứ khiến người ta tắt tính năng đi, và một tính năng bị tắt thì tốt đến
//! đâu cũng bằng không.
//!
//! Vì thế lần quét thường xuyên nhất chỉ là một loạt `stat`: đọc `mtime` và kích thước,
//! so với bảng `files`, và dừng ở đó cho mọi tệp không đổi. Parse chỉ xảy ra ở phần chênh.
//!
//! # Trần, và vì sao chúng là trần cứng chứ không phải mặc định
//!
//! Ba con số dưới đây — [`MAX_DEPTH`], [`MAX_NODES`], [`MAX_PATHS`] — không nhận giá trị
//! từ người gọi, chỉ cắt xuống. Một đỉnh bậc bốn trăm trả về nguyên vẹn là một quả cầu
//! đen trên màn hình và mười nghìn token trong cửa sổ ngữ cảnh, và cả hai hậu quả đó đều
//! xảy ra **sau** khi lời gọi đã thành công, tức là quá muộn để người gọi tự sửa. Cái duy
//! nhất người gọi cần biết là nó đã bị cắt, và [`Neighborhood::truncated`] nói ra.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use ignore::WalkBuilder;
use pai_core::ServiceKey;
use pai_fs::FileRoots;

use crate::error::IndexError;
use crate::extract::Extractor;
use crate::graph::{EdgeKind, GraphEdge, GraphNode, Neighborhood, Overview, Stats};
use crate::lang::{self, Lang};
use crate::store::Store;
use crate::symbol::{Symbol, SymbolKind};

/// Xa hơn bốn bước thì lát cắt không còn là "quanh ký hiệu này" nữa mà là "gần hết repo",
/// và một đồ thị bằng cả repo trả lời được đúng bằng số câu hỏi mà không có đồ thị nào.
pub const MAX_DEPTH: u32 = 4;
/// Trần số đỉnh của một lân cận.
pub const MAX_NODES: usize = 200;
/// Bao nhiêu đỉnh khi người gọi không nói gì.
pub const DEFAULT_NODES: usize = 60;
/// Trần số đường đi của một lần truy vết.
pub const MAX_PATHS: usize = 40;
/// Trần số lần mở rộng của một lần truy vết. Một đồ thị có chu trình dày làm số đường đi
/// nổ theo hàm mũ trước khi kịp chạm [`MAX_PATHS`]; cái này chặn thời gian, cái kia chặn
/// kích thước kết quả.
const TRACE_BUDGET: usize = 4_000;
/// Bao nhiêu thư mục và bao nhiêu ký hiệu trung tâm trong một bản đồ kiến trúc.
const OVERVIEW_DIRS: usize = 40;
const OVERVIEW_CENTRAL: usize = 20;

/// Kết quả một lần đồng bộ. Đây là số để đọc log bằng, không phải để mô hình đọc.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Số tệp mã nguồn nhìn thấy được sau khi đã lọc `.gitignore`.
    pub scanned: usize,
    /// Số tệp thật sự phải parse lại lần này.
    pub parsed: usize,
    /// Số tệp đã biến mất khỏi đĩa và vừa bị quên.
    pub forgotten: usize,
    /// Số cạnh trong đồ thị sau lần quét.
    pub edges: usize,
}

#[async_trait]
pub trait SymbolIndex: Send + Sync + 'static {
    /// Kéo chỉ mục về khớp với đĩa. Tăng dần, nên gọi trước mỗi lần tra là hợp lý.
    async fn sync(&self) -> Result<SyncReport, IndexError>;

    async fn search(
        &self,
        query: &str,
        kind: Option<SymbolKind>,
        limit: usize,
    ) -> Result<Vec<Symbol>, IndexError>;

    /// `Ok(None)` nghĩa là tệp không nằm trong chỉ mục — khác hẳn `Ok(Some(vec![]))`, là
    /// một tệp đã quét và thật sự không có ký hiệu nào.
    async fn outline(&self, path: &Path) -> Result<Option<Vec<Symbol>>, IndexError>;

    /// Lát cắt quanh một ký hiệu. `depth` và `limit` đều bị cắt xuống trần cứng, và
    /// [`Neighborhood::truncated`] nói ra khi điều đó xảy ra.
    async fn neighborhood(
        &self,
        symbol: &str,
        depth: u32,
        limit: usize,
    ) -> Result<Neighborhood, IndexError>;

    /// Các đường đi **ngược** theo cạnh `calls`: ai gọi, rồi ai gọi cái đó.
    async fn callers(&self, symbol: &str, depth: u32) -> Result<Vec<Vec<GraphNode>>, IndexError>;

    /// Các đường đi **xuôi** theo cạnh `calls`.
    async fn callees(&self, symbol: &str, depth: u32) -> Result<Vec<Vec<GraphNode>>, IndexError>;

    /// Bản đồ kiến trúc: thư mục, ngôn ngữ, ký hiệu bậc cao nhất.
    async fn overview(&self) -> Result<Overview, IndexError>;

    async fn stats(&self) -> Result<Stats, IndexError>;
}

pub enum Index {}
impl ServiceKey for Index {
    type Api = dyn SymbolIndex;
    const NAME: &'static str = "index";
}

pub struct CodeIndex {
    roots: FileRoots,
    store: Arc<Store>,
    extractor: Arc<Extractor>,
    parses: Arc<AtomicU64>,
}

impl CodeIndex {
    pub fn open(roots: FileRoots, db: &Path) -> Result<CodeIndex, IndexError> {
        if let Some(parent) = db.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| IndexError::Store(format!("{}: {err}", parent.display())))?;
        }
        CodeIndex::with_store(roots, Store::open(db)?)
    }

    /// Cho bài kiểm chứng, và cho phiên không cần sống qua lần khởi động sau.
    pub fn in_memory(roots: FileRoots) -> Result<CodeIndex, IndexError> {
        CodeIndex::with_store(roots, Store::open_in_memory()?)
    }

    fn with_store(roots: FileRoots, store: Store) -> Result<CodeIndex, IndexError> {
        let extractor = Extractor::new().map_err(|err| IndexError::Store(err.to_string()))?;
        Ok(CodeIndex {
            roots,
            store: Arc::new(store),
            extractor: Arc::new(extractor),
            parses: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Số lần một tệp thật sự đi qua tree-sitter kể từ lúc mở.
    ///
    /// Số này là cách duy nhất nhìn thấy được rằng chỉ mục đang tăng dần chứ không đang
    /// âm thầm quét lại toàn bộ: hai lần `sync` liên tiếp trên một cây không đổi phải để
    /// nó nguyên. Bài kiểm chứng soi vào đây, và log cũng vậy.
    pub fn parse_count(&self) -> u64 {
        self.parses.load(Ordering::Relaxed)
    }

    pub fn symbol_count(&self) -> Result<i64, IndexError> {
        self.store.symbol_count()
    }

    pub fn edge_count(&self) -> Result<i64, IndexError> {
        self.store.edge_count()
    }

    /// Cạnh quan sát được **trong một tệp**, đã kèm cả hai đầu.
    ///
    /// Nó không nằm trên seam vì mô hình không hỏi câu này; nó nằm ở đây vì một bài kiểm
    /// chứng phải khẳng định được một cạnh **cụ thể** tồn tại, chứ không phải "có hơn
    /// không cạnh".
    pub fn edges_of_file(
        &self,
        path: &Path,
    ) -> Result<Vec<(GraphNode, EdgeKind, GraphNode)>, IndexError> {
        self.store.edges_of_file(&path.display().to_string())
    }
}

#[async_trait]
impl SymbolIndex for CodeIndex {
    async fn sync(&self) -> Result<SyncReport, IndexError> {
        let roots = self.roots.clone();
        let store = self.store.clone();
        let extractor = self.extractor.clone();
        let parses = self.parses.clone();
        // Đi cây thư mục, đọc tệp và parse đều là việc chặn, và một repo lớn thì chặn
        // lâu. Ra khỏi runtime, nếu không cả reactor đứng trong lúc quét.
        blocking(move || scan(&roots, &store, &extractor, &parses)).await
    }

    async fn search(
        &self,
        query: &str,
        kind: Option<SymbolKind>,
        limit: usize,
    ) -> Result<Vec<Symbol>, IndexError> {
        let store = self.store.clone();
        let query = query.to_string();
        blocking(move || store.search(&query, kind, limit)).await
    }

    async fn outline(&self, path: &Path) -> Result<Option<Vec<Symbol>>, IndexError> {
        let store = self.store.clone();
        let path = path.display().to_string();
        blocking(move || {
            if !store.knows(&path)? {
                return Ok(None);
            }
            Ok(Some(store.outline(&path)?))
        })
        .await
    }

    async fn neighborhood(
        &self,
        symbol: &str,
        depth: u32,
        limit: usize,
    ) -> Result<Neighborhood, IndexError> {
        let store = self.store.clone();
        let symbol = symbol.to_string();
        blocking(move || neighborhood(&store, &symbol, depth, limit)).await
    }

    async fn callers(&self, symbol: &str, depth: u32) -> Result<Vec<Vec<GraphNode>>, IndexError> {
        let store = self.store.clone();
        let symbol = symbol.to_string();
        blocking(move || trace(&store, &symbol, depth, false)).await
    }

    async fn callees(&self, symbol: &str, depth: u32) -> Result<Vec<Vec<GraphNode>>, IndexError> {
        let store = self.store.clone();
        let symbol = symbol.to_string();
        blocking(move || trace(&store, &symbol, depth, true)).await
    }

    async fn overview(&self) -> Result<Overview, IndexError> {
        let store = self.store.clone();
        blocking(move || store.overview(OVERVIEW_DIRS, OVERVIEW_CENTRAL)).await
    }

    async fn stats(&self) -> Result<Stats, IndexError> {
        let store = self.store.clone();
        blocking(move || store.stats()).await
    }
}

async fn blocking<T, F>(body: F) -> Result<T, IndexError>
where
    F: FnOnce() -> Result<T, IndexError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(body).await {
        Ok(result) => result,
        Err(err) => Err(IndexError::Unavailable(err.to_string())),
    }
}

/// Lát cắt quanh một ký hiệu, mở dần từng vòng.
///
/// Mở theo vòng chứ không đệ quy vì mỗi vòng là **một** câu truy vấn cho cả biên giới,
/// chứ không phải một câu cho mỗi đỉnh; và vì trần phải được kiểm sau mỗi vòng, không
/// phải sau khi đã trót lấy hết.
fn neighborhood(
    store: &Store,
    symbol: &str,
    depth: u32,
    limit: usize,
) -> Result<Neighborhood, IndexError> {
    let seeds = store.nodes_named(symbol)?;
    if seeds.is_empty() {
        return Ok(Neighborhood::default());
    }
    let reach = depth.min(MAX_DEPTH);
    let cap = limit.clamp(1, MAX_NODES);
    let edge_cap = cap.saturating_mul(4);
    let mut truncated = depth > MAX_DEPTH || limit > MAX_NODES;

    let mut order: Vec<i64> = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    for node in &seeds {
        if seen.insert(node.id) {
            order.push(node.id);
        }
    }
    if order.len() > cap {
        order.truncate(cap);
        seen = order.iter().copied().collect();
        truncated = true;
    }

    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut recorded: HashSet<GraphEdge> = HashSet::new();
    let mut frontier = order.clone();
    for _ in 0..reach {
        if frontier.is_empty() {
            break;
        }
        let mut next: Vec<i64> = Vec::new();
        let mut queued: HashSet<i64> = HashSet::new();
        for edge in store.edges_touching(&frontier)? {
            if recorded.insert(edge) {
                edges.push(edge);
            }
            for id in [edge.src, edge.dst] {
                if !seen.contains(&id) && queued.insert(id) {
                    next.push(id);
                }
            }
        }
        if edges.len() > edge_cap {
            edges.truncate(edge_cap);
            truncated = true;
        }
        let room = cap.saturating_sub(order.len());
        if next.len() > room {
            next.truncate(room);
            truncated = true;
        }
        for id in &next {
            seen.insert(*id);
            order.push(*id);
        }
        frontier = next;
    }

    // Một cạnh có một đầu nằm ngoài tập đỉnh là một cạnh không vẽ được và không đọc được.
    // Nó chỉ xuất hiện khi đã cắt, và `truncated` đã nói điều đó rồi.
    edges.retain(|edge| seen.contains(&edge.src) && seen.contains(&edge.dst));

    let mut fetched: HashMap<i64, GraphNode> = store
        .nodes_by_ids(&order)?
        .into_iter()
        .map(|node| (node.id, node))
        .collect();
    let nodes: Vec<GraphNode> = order
        .iter()
        .filter_map(|id| fetched.remove(id))
        .collect::<Vec<_>>();

    Ok(Neighborhood {
        nodes,
        edges,
        truncated,
    })
}

/// Các đường đi theo cạnh `calls`, một chiều.
///
/// Chỉ `calls`: `contains` nối mọi tệp với mọi ký hiệu của nó, nên để nó vào thì mọi hàm
/// đều "gọi tới" mọi hàm cùng tệp qua hai bước, và câu trả lời hết nói lên điều gì.
fn trace(
    store: &Store,
    symbol: &str,
    depth: u32,
    forward: bool,
) -> Result<Vec<Vec<GraphNode>>, IndexError> {
    let seeds = store.nodes_named(symbol)?;
    let reach = depth.clamp(1, MAX_DEPTH);
    let mut found: Vec<Vec<i64>> = Vec::new();
    let mut budget = TRACE_BUDGET;

    'seeds: for seed in &seeds {
        let mut stack: Vec<Vec<i64>> = vec![vec![seed.id]];
        while let Some(path) = stack.pop() {
            if found.len() >= MAX_PATHS || budget == 0 {
                break 'seeds;
            }
            budget -= 1;
            let Some(last) = path.last().copied() else {
                continue;
            };
            if path.len() as u32 > reach {
                found.push(path);
                continue;
            }
            let mut next: Vec<i64> = Vec::new();
            for edge in store.step(&[last], EdgeKind::Calls, forward)? {
                let id = if forward { edge.dst } else { edge.src };
                if !path.contains(&id) && !next.contains(&id) {
                    next.push(id);
                }
            }
            if next.is_empty() {
                // Một đường đi chỉ có mình cái đỉnh xuất phát không phải một đường đi.
                if path.len() > 1 {
                    found.push(path);
                }
                continue;
            }
            for id in next {
                let mut branch = path.clone();
                branch.push(id);
                stack.push(branch);
            }
        }
    }

    found.sort();
    found.dedup();
    let ids: Vec<i64> = {
        let mut all: Vec<i64> = found.iter().flatten().copied().collect();
        all.sort_unstable();
        all.dedup();
        all
    };
    let nodes: HashMap<i64, GraphNode> = store
        .nodes_by_ids(&ids)?
        .into_iter()
        .map(|node| (node.id, node))
        .collect();
    let mut paths: Vec<Vec<GraphNode>> = found
        .into_iter()
        .map(|path| {
            path.into_iter()
                .filter_map(|id| nodes.get(&id).cloned())
                .collect::<Vec<_>>()
        })
        .filter(|path| path.len() > 1)
        .collect();
    paths.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| names(a).cmp(&names(b))));
    Ok(paths)
}

fn names(path: &[GraphNode]) -> Vec<&str> {
    path.iter().map(|node| node.name.as_str()).collect()
}

/// Dấu vân tay của một tệp trên đĩa: đủ để nói "không đổi", không đủ để nói "giống hệt".
///
/// `mtime` + kích thước bỏ sót đúng một trường hợp: sửa tệp giữ nguyên độ dài **và** giữ
/// nguyên `mtime`. Cách duy nhất tạo ra nó là đặt lại `mtime` bằng tay. Bắt trường hợp
/// đó đòi băm nội dung của mọi tệp ở mọi lần quét — tức là đọc toàn bộ repo mỗi lần, đúng
/// cái giá mà chỉ mục tăng dần sinh ra để khỏi phải trả.
struct Fingerprint {
    lang: &'static Lang,
    mtime: i64,
    size: i64,
}

fn scan(
    roots: &FileRoots,
    store: &Store,
    extractor: &Extractor,
    parses: &AtomicU64,
) -> Result<SyncReport, IndexError> {
    let current = walk(roots)?;
    let known = store.known_files()?;

    let mut parsed = 0;
    for (path, print) in &current {
        if known
            .get(path)
            .is_some_and(|state| state.mtime == print.mtime && state.size == print.size)
        {
            continue;
        }
        match std::fs::read_to_string(path) {
            Ok(source) => {
                let found = extractor.extract(print.lang, path, &source);
                parses.fetch_add(1, Ordering::Relaxed);
                store.replace_file(path, print.lang.name, print.mtime, print.size, &found)?;
                parsed += 1;
            }
            Err(err) => {
                // Một tệp có đuôi `.rs` nhưng không đọc được dưới dạng UTF-8 vẫn được ghi
                // vào bảng với **không ký hiệu nào**. Bỏ qua im lặng thì nó bị đọc hỏng
                // lại ở mọi lần quét sau; ghi lại thì nó im cho tới khi có người sửa nó.
                tracing::debug!(path, error = %err, "bỏ qua tệp không đọc được");
                store.replace_file(
                    path,
                    print.lang.name,
                    print.mtime,
                    print.size,
                    &Default::default(),
                )?;
            }
        }
    }

    let gone: Vec<String> = known
        .keys()
        .filter(|path| !current.contains_key(*path))
        .cloned()
        .collect();
    let forgotten = gone.len();
    store.forget_files(&gone)?;

    // Phân giải lại **toàn kho** khi có bất cứ thứ gì đổi, và không làm gì khi không có gì
    // đổi. Một tệp mới có thể là đích của những cạnh đã nằm chờ trong `refs` từ lâu, nên
    // "chỉ phân giải lại tệp vừa đổi" sẽ để chúng nằm chờ mãi mãi.
    let edges = if parsed > 0 || forgotten > 0 {
        store.rebuild_edges()?
    } else {
        store.edge_count()? as usize
    };
    store.mark_scanned(now_ms())?;

    Ok(SyncReport {
        scanned: current.len(),
        parsed,
        forgotten,
        edges,
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

fn walk(roots: &FileRoots) -> Result<HashMap<String, Fingerprint>, IndexError> {
    let mut current = HashMap::new();
    for root in roots.roots() {
        // Phân giải gốc trước khi đi: đường lưu trong chỉ mục phải trùng từng byte với
        // đường mà `FileRoots::resolve_read` trả về, nếu không `outline` sẽ tra một chuỗi
        // và bảng lại chứa một chuỗi khác cho cùng một tệp. Trên macOS, `/var` với
        // `/private/var` là đúng cặp chuỗi đó.
        let base = canonical(root)?;
        let mut builder = WalkBuilder::new(&base);
        // `.gitignore` phải có tác dụng kể cả khi thư mục chưa `git init`: người dùng viết
        // tệp đó để nói "đừng nhìn vào đây", và đó là ý định chứ không phải một chi tiết
        // của git.
        builder.require_git(false);
        for entry in builder.build().flatten() {
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            let Some(lang) = lang::for_path(path) else {
                continue;
            };
            // Giấu khỏi chỉ mục, không chỉ chặn đọc — cùng lý do với `glob`: kể tên một
            // tệp được bảo vệ là đã nói cho mô hình biết có cái gì ở đó.
            if roots.is_protected(path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_nanos() as i64)
                .unwrap_or_default();
            current.insert(
                path.display().to_string(),
                Fingerprint {
                    lang,
                    mtime,
                    size: meta.len() as i64,
                },
            );
        }
    }
    Ok(current)
}

fn canonical(root: &Path) -> Result<PathBuf, IndexError> {
    root.canonicalize()
        .map_err(|err| IndexError::Scan(root.display().to_string(), err.to_string()))
}
