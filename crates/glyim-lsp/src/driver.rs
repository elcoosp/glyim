use crate::AnalysisDatabase;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

pub enum AnalysisMessage {
    FileChanged { path: PathBuf, content: String, version: i32 },
    FileClosed { path: PathBuf },
    Shutdown,
}

pub struct AnalysisDriver {
    db: Arc<AnalysisDatabase>,
    rx: Receiver<AnalysisMessage>,
}

impl AnalysisDriver {
    pub fn new(db: Arc<AnalysisDatabase>, rx: Receiver<AnalysisMessage>) -> Self {
        Self { db, rx }
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                AnalysisMessage::FileChanged { path, content, version: _version } => {
                    self.analyze_file(&path, &content);
                }
                AnalysisMessage::FileClosed { path } => {
                    self.db.file_map.write().remove(&path);
                }
                AnalysisMessage::Shutdown => break,
            }
        }
    }

    fn analyze_file(&self, path: &PathBuf, content: &str) {
        let file_id = { self.db.file_map.write().get_or_create(path) };
        let sm = glyim_diag::SourceMap::new(path.clone(), file_id, content.to_string());
        {
            let mut source_maps = self.db.source_maps.write();
            source_maps.insert(file_id, sm);
        }
        let parse_out = glyim_parse::parse(content);
        let mut diagnostics = Vec::new();
        if !parse_out.errors.is_empty() {
            let source_maps = self.db.source_maps.read();
            if let Some(sm) = source_maps.get(&file_id) {
                diagnostics.extend(
                    crate::diagnostics::convert_parse_errors(file_id, sm, &parse_out.errors),
                );
            }
        }
        self.db.diagnostics.write().insert(file_id, diagnostics);
        if parse_out.errors.is_empty() {
            let mut interner = parse_out.interner;
            let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
            self.db.hirs.write().insert(file_id, hir.clone());
            self.db.symbol_index.write().build_from_hir(file_id, &hir, &interner);
            self.db.reference_graph.write().build_from_hir(file_id, &hir, &interner);
        }
    }
}
