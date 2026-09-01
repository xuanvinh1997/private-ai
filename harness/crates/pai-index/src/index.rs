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

use std::collections::HashMap;
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
use crate::lang::{self, Lang};
use crate::store::Store;
use crate::symbol::{Symbol, SymbolKind};

/// Kết quả một lần đồng bộ. Đây là số để đọc log bằng, không phải để mô hình đọc.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Số tệp mã nguồn nhìn thấy được sau khi đã lọc `.gitignore`.
    pub scanned: usize,
    /// Số tệp thật sự phải parse lại lần này.
    pub parsed: usize,
    /// Số tệp đã biến mất khỏi đĩa và vừa bị quên.
    pub forgotten: usize,
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
                let symbols = extractor.extract(print.lang, path, &source);
                parses.fetch_add(1, Ordering::Relaxed);
                store.replace_file(path, print.lang.name, print.mtime, print.size, &symbols)?;
                parsed += 1;
            }
            Err(err) => {
                // Một tệp có đuôi `.rs` nhưng không đọc được dưới dạng UTF-8 vẫn được ghi
                // vào bảng với **không ký hiệu nào**. Bỏ qua im lặng thì nó bị đọc hỏng
                // lại ở mọi lần quét sau; ghi lại thì nó im cho tới khi có người sửa nó.
                tracing::debug!(path, error = %err, "bỏ qua tệp không đọc được");
                store.replace_file(path, print.lang.name, print.mtime, print.size, &[])?;
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

    Ok(SyncReport {
        scanned: current.len(),
        parsed,
        forgotten,
    })
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
