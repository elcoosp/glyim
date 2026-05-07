use crate::symbol_index::SymbolIndex;
use crate::reference_graph::ReferenceGraph;
use glyim_diag::{FileId, SourceMap};
use glyim_hir::Hir;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time;
use parking_lot::RwLock;
pub struct FileMap {
    path_to_id: HashMap<PathBuf, FileId>,
    id_to_path: HashMap<FileId, PathBuf>,
    next_id: u32,
}
impl Default for FileMap {
    fn default() -> Self {
        Self::new()
    }
}

impl FileMap {
    pub fn new() -> Self {
        Self { path_to_id: HashMap::new(), id_to_path: HashMap::new(), next_id: 0 }
    }
    pub fn get_or_create(&mut self, path: &PathBuf) -> FileId {
        if let Some(id) = self.path_to_id.get(path) {
            return *id;
        }
        let id = FileId(self.next_id);
        self.next_id += 1;
        self.path_to_id.insert(path.clone(), id);
        self.id_to_path.insert(id, path.clone());
        id
    }
    pub fn get_by_path(&self, path: &Path) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }
    pub fn path(&self, id: FileId) -> Option<&PathBuf> {
        self.id_to_path.get(&id)
    }
    pub fn remove(&mut self, path: &PathBuf) {
        if let Some(id) = self.path_to_id.remove(path) {
            self.id_to_path.remove(&id);
        }
    }
}
pub struct AnalysisDatabase {
    pub file_map: RwLock<FileMap>,
    pub source_maps: RwLock<HashMap<FileId, SourceMap>>,
    pub symbol_index: RwLock<SymbolIndex>,
    pub reference_graph: RwLock<ReferenceGraph>,
    pub hirs: RwLock<HashMap<FileId, Hir>>,
    pub diagnostics: RwLock<HashMap<FileId, Vec<lsp_types::Diagnostic>>>,
}
impl Default for AnalysisDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisDatabase {
    pub fn new() -> Self {
        Self {
            file_map: RwLock::new(FileMap::new()),
            source_maps: RwLock::new(HashMap::new()),
            symbol_index: RwLock::new(SymbolIndex::new()),
            reference_graph: RwLock::new(ReferenceGraph::new()),
            hirs: RwLock::new(HashMap::new()),
            diagnostics: RwLock::new(HashMap::new()),
            file_access_times: RwLock::new(HashMap::new()),
        }
    }

    /// Evict entries that have not been accessed within the given duration (no‑op for now).
    pub fn evict_stale(&self, _max_age_secs: u64) {
        // Placeholder: in production, would remove items from hirs, csts, etc.
    }

}

    /// Evict entries that have not been accessed within `max_age`.
    pub fn evict_stale(&self, max_age: std::time::Duration) {
        let now = std::time::Instant::now();
        let mut stale_ids = Vec::new();
        {
            let access_times = self.file_access_times.read();
            for (file_id, last_access) in access_times.iter() {
                if now.duration_since(*last_access) > max_age {
                    stale_ids.push(*file_id);
                }
            }
        }
        if stale_ids.is_empty() {
            return;
        }
        // Remove stale entries
        {
            let mut hirs = self.hirs.write();
            let mut source_maps = self.source_maps.write();
            let mut diags = self.diagnostics.write();
            let mut access_times = self.file_access_times.write();
            for id in &stale_ids {
                hirs.remove(id);
                source_maps.remove(id);
                diags.remove(id);
                access_times.remove(id);
            }
        }
        // Also remove from symbol index and reference graph
        {
            let mut sym_index = self.symbol_index.write();
            let mut ref_graph = self.reference_graph.write();
            for id in &stale_ids {
                sym_index.clear_file(*id);
                // Reference graph does not have a per-file clear method; we'll keep it as is
            }
        }
    }

    /// Record access to a file (call before LSP reads).
    pub fn touch(&self, file_id: FileId) {
        self.file_access_times.write().insert(file_id, std::time::Instant::now());
    }

