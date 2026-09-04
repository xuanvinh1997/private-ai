//! The index seam, and the on-disk implementation for this machine.
//! One invariant: an unchanged file is never re-parsed, so the common scan is just `stat`.
//! The three caps below are hard ceilings, not defaults; callers only learn they were cut.

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

/// The result of one sync: numbers for reading logs, not for the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Source files visible after `.gitignore` filtering.
    pub scanned: usize,
    /// Files actually re-parsed this time.
    pub parsed: usize,
    /// Files gone from disk and just forgotten.
    pub forgotten: usize,
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
}

#[async_trait]
impl SymbolIndex for CodeIndex {
    async fn sync(&self) -> Result<SyncReport, IndexError> {
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
                // Record an unreadable file with no symbols, or every later scan retries and fails on it again.
                tracing::debug!(path, error = %err, "skipping unreadable file");
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

    // Re-resolve the whole store on any change: a new file can be the target of long-pending `refs`.
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
        // Canonicalise first: stored paths must match `FileRoots::resolve_read` byte for byte (macOS `/var`).
        let base = canonical(root)?;
        let mut builder = WalkBuilder::new(&base);
        // `.gitignore` applies even without `git init`: it states intent, not a git detail.
        builder.require_git(false);
        for entry in builder.build().flatten() {
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            let Some(lang) = lang::for_path(path) else {
                continue;
            };
            // Hidden from the index, not merely unreadable: naming a protected file already leaks it.
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
