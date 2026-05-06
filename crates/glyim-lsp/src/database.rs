use crate::symbol_index::SymbolIndex;
use crate::reference_graph::ReferenceGraph;
use glyim_diag::{FileId, SourceMap};
use glyim_hir::Hir;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
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
        }
    }
}
