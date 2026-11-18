//! Virtual file system.
//!
//! All mutable state consolidated into a single `RwLock<VfsInner>`.
//! `add_file_from_disk()` returns `Result`; `add_file_content()`
//! is pure registration. `file_content()` returns `Arc<str>`.

use glyim_span::FileId;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct VfsInner {
    files: Vec<VfsFile>,
    path_to_id: HashMap<PathBuf, FileId>,
    next_id: u32,
}

#[derive(Clone, Debug)]
struct VfsFile {
    path: PathBuf,
    content: Arc<str>,
}

/// The virtual file system every driver sits on: path-keyed source
/// registration (`add_file_content`), disk loading (`add_file_from_disk`),
/// and in-place content updates (`set_file_content`) behind a shared
/// read/write lock, so the LSP change path and the pipeline flatten path
/// mutate the same store the compiler reads.
pub struct Vfs {
    inner: RwLock<VfsInner>,
}

/// The typed error the VFS write faces return.
///
/// The VFS never silently drops a write: a `FileId` that does not name a
/// file in this VFS (a stale id, a forged id) is refused explicitly, so a
/// caller cannot mistake a lost write for a successful one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VfsError {
    /// The `FileId` does not name a file in this VFS: the write was
    /// refused and the stored content is untouched.
    UnknownFile(FileId),
}

impl std::fmt::Display for VfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VfsError::UnknownFile(id) => {
                write!(f, "unknown file id (index {})", id.index())
            }
        }
    }
}

impl std::error::Error for VfsError {}

impl Vfs {
    /// Create an empty VFS (no files registered).
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(VfsInner {
                files: Vec::new(),
                path_to_id: HashMap::new(),
                next_id: 0,
            }),
        }
    }

    /// Read one file from disk and register it under `path`, returning the
    /// minted [`FileId`]. An unreadable path (missing file, not UTF-8)
    /// surfaces as the underlying [`std::io::Error`] instead of a silent
    /// empty registration.
    ///
    /// # Errors
    ///
    /// Returns the `std::io::Error` the disk read produced.
    pub fn add_file_from_disk(&self, path: &Path) -> std::io::Result<FileId> {
        let content = std::fs::read_to_string(path)?;
        Ok(self.add_file_content(path, Arc::from(content)))
    }

    /// Register in-memory source under `path` (or overwrite the content of
    /// an already-registered path), returning the file's [`FileId`]. The
    /// registration is pure — no disk access — so test harnesses and the
    /// LSP can push source that does not exist on disk.
    pub fn add_file_content(&self, path: &Path, content: Arc<str>) -> FileId {
        let mut inner = self.inner.write();
        if let Some(&id) = inner.path_to_id.get(path) {
            if let Some(file) = inner.files.get_mut(id.index()) {
                file.content = content;
            }
            return id;
        }
        let id = FileId::from_raw(inner.next_id);
        inner.next_id += 1;
        inner.files.push(VfsFile {
            path: path.to_path_buf(),
            content,
        });
        inner.path_to_id.insert(path.to_path_buf(), id);
        id
    }

    /// Overwrite one registered file's content in place (the LSP change
    /// path). The write is total: a `FileId` that does not name a file in
    /// this VFS is refused through [`VfsError::UnknownFile`] — a stale id
    /// never silently loses the write (the caller decides whether a stale
    /// id is a bug or a dropped edit).
    ///
    /// # Errors
    ///
    /// Returns [`VfsError::UnknownFile`] when `file_id` does not name a
    /// file in this VFS.
    pub fn set_file_content(&self, file_id: FileId, content: Arc<str>) -> Result<(), VfsError> {
        let mut inner = self.inner.write();
        let file = inner
            .files
            .get_mut(file_id.index())
            .ok_or(VfsError::UnknownFile(file_id))?;
        file.content = content;
        Ok(())
    }

    /// One file's content (`None` when the id does not name a file here).
    pub fn file_content(&self, file_id: FileId) -> Option<Arc<str>> {
        let inner = self.inner.read();
        inner
            .files
            .get(file_id.index())
            .map(|f| Arc::clone(&f.content))
    }

    /// Run one closure over a file's content under the read lock, avoiding
    /// the [`Arc<str>`] clone [`Vfs::file_content`] pays (`None` when the
    /// id does not name a file here).
    pub fn file_content_ref<R>(&self, file_id: FileId, f: impl FnOnce(&str) -> R) -> Option<R> {
        let inner = self.inner.read();
        inner
            .files
            .get(file_id.index())
            .map(|file| f(&file.content))
    }

    /// One file's registered path (`None` when the id does not name a file
    /// here).
    pub fn file_path(&self, file_id: FileId) -> Option<PathBuf> {
        let inner = self.inner.read();
        inner.files.get(file_id.index()).map(|f| f.path.clone())
    }

    /// The [`FileId`] one path is registered under (`None` when the path
    /// was never registered).
    pub fn file_id(&self, path: &Path) -> Option<FileId> {
        let inner = self.inner.read();
        inner.path_to_id.get(path).copied()
    }

    /// How many files the VFS holds.
    pub fn len(&self) -> usize {
        self.inner.read().files.len()
    }

    /// Whether no file is registered.
    pub fn is_empty(&self) -> bool {
        self.inner.read().files.is_empty()
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
