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

/// Vfs.
pub struct Vfs {
    inner: RwLock<VfsInner>,
}

impl Vfs {
    /// new.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(VfsInner {
                files: Vec::new(),
                path_to_id: HashMap::new(),
                next_id: 0,
            }),
        }
    }

    #[tracing::instrument(skip(self))]
    /// add_file_from_disk.
    pub fn add_file_from_disk(&self, path: &Path) -> std::io::Result<FileId> {
        let content = std::fs::read_to_string(path)?;
        Ok(self.add_file_content(path, Arc::from(content)))
    }

    #[tracing::instrument(skip(self, content))]
    /// add_file_content.
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

    /// set_file_content.
    pub fn set_file_content(&self, file_id: FileId, content: Arc<str>) {
        let mut inner = self.inner.write();
        if let Some(file) = inner.files.get_mut(file_id.index()) {
            file.content = content;
        }
    }

    /// file_content.
    pub fn file_content(&self, file_id: FileId) -> Option<Arc<str>> {
        let inner = self.inner.read();
        inner
            .files
            .get(file_id.index())
            .map(|f| Arc::clone(&f.content))
    }

    /// file_content_ref.
    pub fn file_content_ref<R>(&self, file_id: FileId, f: impl FnOnce(&str) -> R) -> Option<R> {
        let inner = self.inner.read();
        inner
            .files
            .get(file_id.index())
            .map(|file| f(&file.content))
    }

    /// file_path.
    pub fn file_path(&self, file_id: FileId) -> Option<PathBuf> {
        let inner = self.inner.read();
        inner.files.get(file_id.index()).map(|f| f.path.clone())
    }

    /// file_id.
    pub fn file_id(&self, path: &Path) -> Option<FileId> {
        let inner = self.inner.read();
        inner.path_to_id.get(path).copied()
    }

    /// len.
    pub fn len(&self) -> usize {
        self.inner.read().files.len()
    }
    /// is_empty.
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
