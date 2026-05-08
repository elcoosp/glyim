use crate::AnalysisDatabase;
use glyim_compiler::pipeline::PipelineConfig;
use glyim_compiler::pipeline::PipelineError;
use glyim_compiler::queries::QueryPipeline;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

pub enum AnalysisMessage {
    FileChanged {
        path: PathBuf,
        content: String,
        version: i32,
    },
    FileClosed {
        path: PathBuf,
    },
    Shutdown,
}

pub struct AnalysisDriver {
    cache_dir: std::path::PathBuf,
    db: Arc<AnalysisDatabase>,
    rx: Receiver<AnalysisMessage>,
}

impl AnalysisDriver {
    pub fn new(
        db: Arc<AnalysisDatabase>,
        rx: Receiver<AnalysisMessage>,
        cache_dir: std::path::PathBuf,
    ) -> Self {
        Self { db, rx, cache_dir }
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                AnalysisMessage::FileChanged {
                    path,
                    content,
                    version: _version,
                } => {
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
        let source_maps = self.db.source_maps.read();
        match qp.compile(content, path) {
            Ok(compiled) => {
                let hir = compiled.mono_hir.clone();
                let interner = compiled.interner.clone();
                self.db.hirs.write().insert(file_id, hir.clone());
                self.db
                    .symbol_index
                    .write()
                    .build_from_hir(file_id, &hir, &interner);
                self.db
                    .reference_graph
                    .write()
                    .build_from_hir(file_id, &hir, &interner);
            }
            Err(e) => {
                // If the error contains Diagnostics, convert them directly
                if let PipelineError::Diagnostics(diags) = &e {
                    for d in diags {
                        let severity = match d.severity {
                            glyim_diag::diagnostic::Severity::Error => {
                                lsp_types::DiagnosticSeverity::ERROR
                            }
                            glyim_diag::diagnostic::Severity::Warning => {
                                lsp_types::DiagnosticSeverity::WARNING
                            }
                            _ => lsp_types::DiagnosticSeverity::INFORMATION,
                        };
                        let range = if let Some(sm) = source_maps.get(&file_id) {
                            let (start, end) =
                                sm.span_to_position(d.span.start, d.span.end).unwrap_or((
                                    glyim_diag::LineCol { line: 0, column: 0 },
                                    glyim_diag::LineCol { line: 0, column: 0 },
                                ));
                            lsp_types::Range {
                                start: lsp_types::Position {
                                    line: start.line as u32,
                                    character: start.column as u32,
                                },
                                end: lsp_types::Position {
                                    line: end.line as u32,
                                    character: end.column as u32,
                                },
                            }
                        } else {
                            lsp_types::Range::default()
                        };
                        diagnostics.push(lsp_types::Diagnostic {
                            range,
                            severity: Some(severity),
                            message: d.message.clone(),
                            ..Default::default()
                        });
                    }
                }
                let err_msg = format!("{}", e);
                // Fall back to simple parse for error reporting
                let parse_out = glyim_parse::parse(content);
                if !parse_out.errors.is_empty() {
                    let source_maps = self.db.source_maps.read();
                    if let Some(sm) = source_maps.get(&file_id) {
                        diagnostics.extend(crate::diagnostics::convert_parse_errors(
                            file_id,
                            sm,
                            &parse_out.errors,
                        ));
                    }
                } else {
                    let d = lsp_types::Diagnostic {
                        range: lsp_types::Range {
                            start: lsp_types::Position {
                                line: 0,
                                character: 0,
                            },
                            end: lsp_types::Position {
                                line: 0,
                                character: 0,
                            },
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
