use crate::AnalysisDatabase;
use glyim_compiler::queries::QueryPipeline;
use glyim_compiler::pipeline::PipelineConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

pub enum AnalysisMessage {
    FileChanged { path: PathBuf, content: String, version: i32 },
    FileClosed { path: PathBuf },
    Shutdown,
}

pub struct AnalysisDriver {
    cache_dir: std::path::PathBuf,
    db: Arc<AnalysisDatabase>,
    rx: Receiver<AnalysisMessage>,
}

impl AnalysisDriver {
    pub fn new(db: Arc<AnalysisDatabase>, rx: Receiver<AnalysisMessage>, cache_dir: std::path::PathBuf) -> Self {
        Self { db, rx, cache_dir }
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
        // Use the incremental query pipeline for cached recompilation
        let config = PipelineConfig::default();
        let mut qp = QueryPipeline::new(&self.cache_dir, config);
        let mut diagnostics = Vec::new();
        match qp.compile(content, &path) {
            Ok(compiled) => {
                let hir = compiled.mono_hir.clone();
                let interner = compiled.interner.clone();
                self.db.hirs.write().insert(file_id, hir.clone());
                self.db.symbol_index.write().build_from_hir(file_id, &hir, &interner);
                self.db.reference_graph.write().build_from_hir(file_id, &hir, &interner);
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                // Fall back to simple parse for error reporting
                let parse_out = glyim_parse::parse(content);
                if !parse_out.errors.is_empty() {
                    let source_maps = self.db.source_maps.read();
                    if let Some(sm) = source_maps.get(&file_id) {
                        diagnostics.extend(
                            crate::diagnostics::convert_parse_errors(file_id, sm, &parse_out.errors),
                        );
                    }
                } else {
                    let d = lsp_types::Diagnostic {
                        range: lsp_types::Range {
                            start: lsp_types::Position { line: 0, character: 0 },
                            end: lsp_types::Position { line: 0, character: 0 },
                        },
                        severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                        message: err_msg,
                        ..Default::default()
                    };
                    diagnostics.push(d);
                }
            }
        }
        self.db.diagnostics.write().insert(file_id, diagnostics);
    }
}
