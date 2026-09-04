//! Paths: canonicalise, and only then check, or `root/../../etc/passwd` still starts with `root/`.
//! Reads use the OS `canonicalize` so symlinks out of the root count as out of the root.
//! Writes resolve `..` lexically, canonicalise the nearest existing ancestor, then rejoin the tail.

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

/// Resolve `.` and `..` without touching the disk; an unresolved `..` is how a path escapes a root.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                // A leading `..` has nothing to pop; keep it so the roots layer refuses it.
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

    /// Exact match, not prefix match: what is protected is that file, not the tree beneath it.
    pub fn is_protected(&self, resolved: &Path) -> bool {
        self.protected.iter().any(|p| p == resolved)
    }

    fn within_roots(&self, resolved: &Path) -> bool {
        // No roots means nobody granted anything, so an empty config refuses everything.
        self.roots.iter().any(|root| resolved.starts_with(root))
    }

    /// Resolve a path for reading; follows symlinks.
    pub fn resolve_read(&self, path: &Path) -> Result<PathBuf, PathError> {
        let resolved = path
            .canonicalize()
            .map_err(|err| PathError::Unresolvable(path.to_path_buf(), err.to_string()))?;
        self.authorize(resolved)
    }

    /// Resolve a path for writing; the file need not exist, the parent directory must.
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
        // Protected is asked first, so a protected file inside a root reports the right reason.
        if self.is_protected(&resolved) {
            return Err(PathError::Protected(resolved));
        }
        if !self.within_roots(&resolved) {
            return Err(PathError::OutsideRoots(resolved));
        }
        Ok(resolved)
    }
}

/// A NUL byte in the first 4 KiB means binary; refuse rather than feed the model garbage.
pub fn looks_binary(head: &[u8]) -> bool {
    head.iter().take(4096).any(|byte| *byte == 0)
}
