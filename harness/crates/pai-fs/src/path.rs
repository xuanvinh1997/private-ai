//! Paths: canonicalise, and only then check.
//!
//! That order is the entire content of this file. Checking before canonicalising lets
//! `root/../../etc/passwd` through, because at comparison time it still starts with `root/`.
//!
//! There are two ways to canonicalise and both are needed. The **read** path uses the
//! operating system's `canonicalize` — it follows symlinks, which is exactly what we want: a
//! symlink pointing outside the root has to count as outside the root. The **write** path
//! cannot use it, because the file does not exist yet; so we resolve `..` lexically, then
//! `canonicalize` the nearest existing ancestor and rejoin the tail. The existing part is
//! still checked through symlinks, and the part that does not exist has no symlink to
//! follow.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("{0} nằm ngoài những thư mục ứng dụng được phép đọc")]
    OutsideRoots(PathBuf),
    #[error("{0} là tệp được bảo vệ và không bao giờ đọc được")]
    Protected(PathBuf),
    #[error("không phân giải được {0}: {1}")]
    Unresolvable(PathBuf, String),
}

/// Resolve `.` and `..` lexically, without touching the disk.
///
/// `canonicalize` cannot do this because it requires the file to exist. Skipping it is not
/// an option either: an unresolved `..` is how a path escapes the root while still looking
/// like it is inside.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                // A leading `..` on a relative path has nothing to pop; keep it and let
                // the roots layer refuse, rather than silently swallowing it.
                if !out.pop() {
                    out.push(Component::ParentDir);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The directories the application may touch, and the files it must never touch.
#[derive(Debug, Default, Clone)]
pub struct FileRoots {
    roots: Vec<PathBuf>,
    protected: Vec<PathBuf>,
}

impl FileRoots {
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        protected: impl IntoIterator<Item = PathBuf>,
    ) -> FileRoots {
        FileRoots {
            roots: roots.into_iter().map(|p| lexical(&p)).collect(),
            protected: protected.into_iter().map(|p| lexical(&p)).collect(),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Exact match, not prefix match: what is protected is *that file*, not the whole tree
    /// beneath it.
    pub fn is_protected(&self, resolved: &Path) -> bool {
        self.protected.iter().any(|p| p == resolved)
    }

    fn within_roots(&self, resolved: &Path) -> bool {
        // No roots means nobody granted anything — refuse everything, not allow
        // everything. An empty config is the tightest config.
        self.roots.iter().any(|root| resolved.starts_with(root))
    }

    /// Resolve a path for **reading**. Follows symlinks.
    pub fn resolve_read(&self, path: &Path) -> Result<PathBuf, PathError> {
        let resolved = path
            .canonicalize()
            .map_err(|err| PathError::Unresolvable(path.to_path_buf(), err.to_string()))?;
        self.authorize(resolved)
    }

    /// Resolve a path for **writing**. The file need not exist; the parent directory must.
    pub fn resolve_write(&self, path: &Path) -> Result<PathBuf, PathError> {
        let lexical = lexical(path);
        if let Ok(resolved) = lexical.canonicalize() {
            return self.authorize(resolved);
        }
        // Climb to the nearest existing ancestor, resolve that, then rejoin the tail.
        let mut ancestors = lexical.ancestors().skip(1);
        let existing = ancestors
            .find(|a| a.exists())
            .ok_or_else(|| PathError::OutsideRoots(lexical.clone()))?;
        let base = existing
            .canonicalize()
            .map_err(|err| PathError::Unresolvable(existing.to_path_buf(), err.to_string()))?;
        let tail = lexical
            .strip_prefix(existing)
            .map_err(|_| PathError::OutsideRoots(lexical.clone()))?;
        self.authorize(base.join(tail))
    }

    fn authorize(&self, resolved: PathBuf) -> Result<PathBuf, PathError> {
        // Protected is asked **first**: for a file that is both inside a root and
        // protected the answer is no, and the reason has to be the right one.
        if self.is_protected(&resolved) {
            return Err(PathError::Protected(resolved));
        }
        if !self.within_roots(&resolved) {
            return Err(PathError::OutsideRoots(resolved));
        }
        Ok(resolved)
    }
}

/// A NUL byte in the first four thousand bytes means binary.
///
/// Refuse outright rather than returning a string full of replacement characters: a binary
/// file read badly looks exactly like a text file with the wrong encoding, and the model
/// will reason over garbage.
pub fn looks_binary(head: &[u8]) -> bool {
    head.iter().take(4096).any(|byte| *byte == 0)
}
