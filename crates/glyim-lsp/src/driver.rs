use crate::AnalysisDatabase;
use glyim_diag::SourceMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

pub enum AnalysisMessage {
    FileChanged { path: PathBuf, content: String, version: i32 },
    FileClosed { path: PathBuf },
    Shutdown,
}

pub struct AnalysisDriver {
    db: Arc<AnalysisDatabase>,
    rx: mpsc::UnboundedReceiver<AnalysisMessage>,
}

impl AnalysisDriver {
    pub fn new(db: Arc<AnalysisDatabase>, rx: mpsc::UnboundedReceiver<AnalysisMessage>) -> Self { Self { db, rx } }
    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                AnalysisMessage::FileChanged { path, content, .. } => self.analyze_file(&path, &content),
                AnalysisMessage::FileClosed { path } => { self.db.file_map.write().unwrap().remove(&path); }
                AnalysisMessage::Shutdown => break,
            }
        }
    }
    fn analyze_file(&self, path: &PathBuf, content: &str) {
        let file_id = { self.db.file_map.write().unwrap().get_or_create(path) };
        let mut sm = self.db.source_maps.write().unwrap();
        sm.insert(file_id, SourceMap::new(path.clone(), file_id, content.to_string()));
        let parse_out = glyim_parse::parse(content);
        let mut diagnostics = Vec::new();
        if !parse_out.errors.is_empty() {
            diagnostics.extend(crate::diagnostics::convert_parse_errors(file_id, sm.get(&file_id).unwrap(), &parse_out.errors));
        }
        self.db.diagnostics.write().unwrap().insert(file_id, diagnostics);
        if parse_out.errors.is_empty() {
            let mut interner = parse_out.interner;
            let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
            self.db.hirs.write().unwrap().insert(file_id, hir.clone());
            self.db.symbol_index.write().unwrap().build_from_hir(file_id, &hir, &interner);
            self.db.reference_graph.write().unwrap().build_from_hir(file_id, &hir, &interner);
        }
    }
}
