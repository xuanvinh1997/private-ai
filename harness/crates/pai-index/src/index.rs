//! The index seam, and the on-disk implementation for this machine.
//! One invariant: an unchanged file is never re-parsed, so the common scan is just `stat`.
//! The caps below are hard ceilings, not defaults; truncating operations report when they were cut.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
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

/// Past four hops the slice stops being "around this symbol" and becomes "most of the repo".
pub const MAX_DEPTH: u32 = 4;
/// Node ceiling for one neighborhood.
pub const MAX_NODES: usize = 200;
/// How many nodes when the caller says nothing.
pub const DEFAULT_NODES: usize = 60;
/// Path ceiling for one trace.
pub const MAX_PATHS: usize = 40;
/// Expansion ceiling for one trace: cycles explode before [`MAX_PATHS`] is reached, so this bounds time, that bounds size.
const TRACE_BUDGET: usize = 4_000;
/// How many directories and central symbols an architecture map holds.
const OVERVIEW_DIRS: usize = 40;
const OVERVIEW_CENTRAL: usize = 20;
/// A committed bundle or generated dump must not make tree-sitter allocate without bound.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum source files parsed and kept in the symbol graph.
pub const MAX_FILES: usize = 5_000;
/// Enough parallelism to saturate parsing without reading eight 64 MiB files per hardware thread.
const MAX_PARSE_THREADS: usize = 8;
/// Machine output and vendored dependencies are noise even when a repository forgot to ignore them.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".venv",
    "venv",
    "env",
    "node_modules",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".cache",
    ".idea",
    ".vscode",
    ".tox",
    "site-packages",
    ".gradle",
    ".terraform",
    "vendor",
    ".DS_Store",
    "$RECYCLE.BIN",
];

/// The result of one sync: numbers for reading logs, not for the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Supported source files visible after ignore and generated-directory filtering, before hard caps.
    pub scanned: usize,
    /// Files actually re-parsed this time.
    pub parsed: usize,
    /// Files gone from disk and just forgotten.
    pub forgotten: usize,
    /// Supported source files ignored because they exceed [`MAX_FILE_BYTES`].
    pub oversized: usize,
    /// Supported source files ignored after [`MAX_FILES`] was reached.
    pub over_limit: usize,
    /// Edges in the graph after the scan.
    pub edges: usize,
}

#[async_trait]
pub trait SymbolIndex: Send + Sync + 'static {
    /// Bring the index back in line with disk; incremental, so calling it before each query is fine.
    async fn sync(&self) -> Result<SyncReport, IndexError>;

    async fn search(
        &self,
        query: &str,
        kind: Option<SymbolKind>,
        limit: usize,
    ) -> Result<Vec<Symbol>, IndexError>;

    /// `Ok(None)` means the file is not indexed; `Ok(Some(vec![]))` means indexed with no symbols.
    async fn outline(&self, path: &Path) -> Result<Option<Vec<Symbol>>, IndexError>;

    /// A slice around one symbol; `depth` and `limit` are clamped, and `truncated` says so.
    async fn neighborhood(
        &self,
        symbol: &str,
        depth: u32,
        limit: usize,
    ) -> Result<Neighborhood, IndexError>;

    /// Paths backwards along `calls`: who calls this, and who calls them.
    async fn callers(&self, symbol: &str, depth: u32) -> Result<Vec<Vec<GraphNode>>, IndexError>;

    /// Paths forwards along `calls`.
    async fn callees(&self, symbol: &str, depth: u32) -> Result<Vec<Vec<GraphNode>>, IndexError>;

    /// The architecture map: directories, languages, highest-degree symbols.
    async fn overview(&self) -> Result<Overview, IndexError>;

    async fn stats(&self) -> Result<Stats, IndexError>;

    /// Ranked paths for an `@` completion query; separate from `search`, which ranks symbols by BM25.
    async fn paths(&self, query: &str, limit: usize) -> Result<Vec<String>, IndexError>;
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
    syncing: tokio::sync::Mutex<()>,
}

impl CodeIndex {
    pub fn open(roots: FileRoots, db: &Path) -> Result<CodeIndex, IndexError> {
        if let Some(parent) = db.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| IndexError::Store(format!("{}: {err}", parent.display())))?;
        }
        CodeIndex::with_store(roots, Store::open(db)?)
    }

    /// For tests, and for sessions that need not outlive this run.
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
            syncing: tokio::sync::Mutex::new(()),
        })
    }

    /// How many files went through tree-sitter since opening: the only visible proof the index is incremental.
    pub fn parse_count(&self) -> u64 {
        self.parses.load(Ordering::Relaxed)
    }

    pub fn symbol_count(&self) -> Result<i64, IndexError> {
        self.store.symbol_count()
    }

    pub fn edge_count(&self) -> Result<i64, IndexError> {
        self.store.edge_count()
    }

    /// Edges observed within one file, both ends attached; off the seam because only tests ask this.
    pub fn edges_of_file(
        &self,
        path: &Path,
    ) -> Result<Vec<(GraphNode, EdgeKind, GraphNode)>, IndexError> {
        self.store.edges_of_file(&path.display().to_string())
    }

    /// Populate completion paths before the slower symbol parse starts in the background.
    pub(crate) async fn refresh_paths(&self) -> Result<usize, IndexError> {
        let _guard = self.syncing.lock().await;
        let roots = self.roots.clone();
        let store = self.store.clone();
        blocking(move || {
            let walked = walk(&roots)?;
            store.sync_paths(&walked.paths)?;
            Ok(walked.paths.len())
        })
        .await
    }
}

#[async_trait]
impl SymbolIndex for CodeIndex {
    async fn sync(&self) -> Result<SyncReport, IndexError> {
        let _guard = self.syncing.lock().await;
        let roots = self.roots.clone();
        let store = self.store.clone();
        let extractor = self.extractor.clone();
        let parses = self.parses.clone();
        // Walking, reading and parsing all block, and a big repo blocks for a long time.
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

    async fn paths(&self, query: &str, limit: usize) -> Result<Vec<String>, IndexError> {
        let store = self.store.clone();
        let query = query.to_string();
        // Read all of `files` and score in memory: cheap, and "filename beats directory" is ugly in SQL.
        blocking(move || Ok(crate::complete::rank(&store.paths()?, &query, limit))).await
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

/// A slice around one symbol, expanded ring by ring: one query per frontier, and the caps are checked each ring.
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

    // An edge with an end outside the node set cannot be drawn; it only appears after a cut, which `truncated` reports.
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

/// One-directional paths along `calls` only: `contains` would make every same-file function two hops apart.
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
                // A path holding only its seed node is not a path.
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

/// Enough to say "unchanged", not "identical": `mtime` plus size misses only a hand-reset `mtime`, and hashing costs a full read.
struct Fingerprint {
    lang: &'static Lang,
    mtime: i64,
    size: i64,
}

struct Walked {
    current: HashMap<String, Fingerprint>,
    paths: Vec<String>,
    scanned: usize,
    oversized: usize,
    over_limit: usize,
}

struct ParseJob {
    path: String,
    lang: &'static Lang,
    mtime: i64,
    size: i64,
}

struct ParsedFile {
    job: ParseJob,
    found: crate::extract::Extraction,
    parsed: bool,
}

fn scan(
    roots: &FileRoots,
    store: &Store,
    extractor: &Extractor,
    parses: &AtomicU64,
) -> Result<SyncReport, IndexError> {
    let walked = walk(roots)?;
    store.sync_paths(&walked.paths)?;
    let current = walked.current;
    let known = store.known_files()?;

    let jobs: Vec<ParseJob> = current
        .iter()
        .filter(|(path, print)| {
            !known
                .get(*path)
                .is_some_and(|state| state.mtime == print.mtime && state.size == print.size)
        })
        .map(|(path, print)| ParseJob {
            path: path.clone(),
            lang: print.lang,
            mtime: print.mtime,
            size: print.size,
        })
        .collect();

    let results = parse_files(&jobs, extractor, parses)?;
    let mut parsed = 0usize;
    let mut affected_names = HashSet::new();
    let mut changed_paths = Vec::with_capacity(results.len());
    for result in results {
        let job = result.job;
        if result.parsed {
            parsed += 1;
        }
        affected_names.extend(store.replace_file(
            &job.path,
            job.lang.name,
            job.mtime,
            job.size,
            &result.found,
        )?);
        changed_paths.push(job.path);
    }

    let gone: Vec<String> = known
        .keys()
        .filter(|path| !current.contains_key(*path))
        .cloned()
        .collect();
    let forgotten = gone.len();
    affected_names.extend(store.forget_files(&gone)?);
    changed_paths.extend(gone);

    let edges = if changed_paths.is_empty() {
        store.edge_count()? as usize
    } else {
        store.rebuild_edges(&affected_names, &changed_paths)?
    };
    store.mark_scanned(now_ms())?;

    Ok(SyncReport {
        scanned: walked.scanned,
        parsed,
        forgotten,
        oversized: walked.oversized,
        over_limit: walked.over_limit,
        edges,
    })
}

fn parse_files(
    jobs: &[ParseJob],
    extractor: &Extractor,
    parses: &AtomicU64,
) -> Result<Vec<ParsedFile>, IndexError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    let workers = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(MAX_PARSE_THREADS)
        .min(jobs.len());
    let chunk_size = jobs.len().div_ceil(workers);

    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in jobs.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|job| match std::fs::read_to_string(&job.path) {
                        Ok(source) => {
                            let found = extractor.extract(job.lang, &job.path, &source);
                            parses.fetch_add(1, Ordering::Relaxed);
                            ParsedFile {
                                job: ParseJob {
                                    path: job.path.clone(),
                                    lang: job.lang,
                                    mtime: job.mtime,
                                    size: job.size,
                                },
                                found,
                                parsed: true,
                            }
                        }
                        Err(err) => {
                            tracing::debug!(path = job.path, error = %err, "skipping unreadable file");
                            ParsedFile {
                                job: ParseJob {
                                    path: job.path.clone(),
                                    lang: job.lang,
                                    mtime: job.mtime,
                                    size: job.size,
                                },
                                found: Default::default(),
                                parsed: false,
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            }));
        }

        let mut out = Vec::with_capacity(jobs.len());
        for handle in handles {
            let mut chunk = handle
                .join()
                .map_err(|_| IndexError::Unavailable("luồng parse chỉ mục bị panic".into()))?;
            out.append(&mut chunk);
        }
        Ok(out)
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

fn walk(roots: &FileRoots) -> Result<Walked, IndexError> {
    walk_with_limits(roots, MAX_FILES, MAX_FILE_BYTES)
}

fn walk_with_limits(
    roots: &FileRoots,
    max_files: usize,
    max_file_bytes: u64,
) -> Result<Walked, IndexError> {
    let mut paths = HashSet::new();
    let mut eligible = HashMap::new();
    let mut oversized = HashSet::new();
    for root in roots.roots() {
        // Canonicalise first: stored paths must match `FileRoots::resolve_read` byte for byte (macOS `/var`).
        let base = canonical(root)?;
        let mut builder = WalkBuilder::new(&base);
        // `.gitignore` applies even without `git init`: it states intent, not a git detail.
        builder.require_git(false);
        let filter_base = base.clone();
        builder.filter_entry(move |entry| {
            entry.path() == filter_base
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIP_DIRS.contains(&name))
        });
        for entry in builder.build().flatten() {
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            // Hidden from the index, not merely unreadable: naming a protected file already leaks it.
            if roots.is_protected(path) {
                continue;
            }
            let stored = path.display().to_string();
            paths.insert(stored.clone());

            let Some(lang) = lang::for_path(path) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.len() > max_file_bytes {
                oversized.insert(stored);
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_nanos() as i64)
                .unwrap_or_default();
            eligible.insert(
                stored,
                Fingerprint {
                    lang,
                    mtime,
                    size: meta.len() as i64,
                },
            );
        }
    }

    let scanned = eligible.len() + oversized.len();
    let mut files: Vec<(String, Fingerprint)> = eligible.into_iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let over_limit = files.len().saturating_sub(max_files);
    files.truncate(max_files);

    let mut paths: Vec<String> = paths.into_iter().collect();
    paths.sort();
    Ok(Walked {
        current: files.into_iter().collect(),
        paths,
        scanned,
        oversized: oversized.len(),
        over_limit,
    })
}

fn canonical(root: &Path) -> Result<PathBuf, IndexError> {
    root.canonicalize()
        .map_err(|err| IndexError::Scan(root.display().to_string(), err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_chan_kich_thuoc_va_so_tep_nhung_van_ghi_path_inventory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "a").unwrap();
        std::fs::write(dir.path().join("b.rs"), "b").unwrap();
        std::fs::write(dir.path().join("huge.rs"), "12345").unwrap();
        std::fs::write(dir.path().join("README.md"), "docs").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let roots = FileRoots::new([root], []);

        let walked = walk_with_limits(&roots, 1, 4).unwrap();
        assert_eq!(walked.current.len(), 1);
        assert_eq!(walked.scanned, 3);
        assert_eq!(walked.oversized, 1);
        assert_eq!(walked.over_limit, 1);
        assert_eq!(
            walked.paths.len(),
            4,
            "mọi tệp thường vẫn hiện trong completion"
        );
    }

    #[test]
    fn walk_bo_thu_muc_may_sinh_du_khong_co_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("vendor/pkg")).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("vendor/pkg/dep.rs"), "fn dep() {}\n").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let roots = FileRoots::new([root], []);

        let walked = walk(&roots).unwrap();
        assert_eq!(walked.current.len(), 1);
        assert_eq!(walked.paths.len(), 1);
        assert!(
            walked.paths[0].ends_with("src/main.rs"),
            "{:?}",
            walked.paths
        );
    }
}
