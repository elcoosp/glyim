use crate::AnalysisDatabase;
use glyim_diag::SourceMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver, Sender};
use std::collections::HashMap;

/// Message from the LSP handler to the analysis driver.
pub enum AnalysisMessage {
    FileChanged { path: PathBuf, content: String, version: i32 },
    FileClosed { path: PathBuf },
    FullReanalysis,
    Shutdown,
}

pub struct AnalysisDriver {
    db: Arc<AnalysisDatabase>,
    rx: Receiver<AnalysisMessage>,
    /// Pending coalesced file changes: path -> (content, version)
    pending: HashMap<PathBuf, (String, i32)>,
    /// Coalesce timer (none for now – immediate processing)
}

impl AnalysisDriver {
    pub fn new(db: Arc<AnalysisDatabase>, rx: Receiver<AnalysisMessage>) -> Self {
        Self { db, rx, pending: HashMap::new() }
    }

    pub async fn run(mut self) {
        loop {
            // Wait for first message
            let msg = match self.rx.recv().await {
                Some(m) => m,
                None => break, // channel closed
            };
            match msg {
                AnalysisMessage::Shutdown => break,
                AnalysisMessage::FileChanged { path, content, version } => {
                    self.pending.insert(path.clone(), (content, version));
                    // Coalesce: drain any additional messages in the queue
                    while let Ok(msg) = self.rx.try_recv() {
                        match msg {
                            AnalysisMessage::FileChanged { path, content, version } => {
                                self.pending.insert(path, (content, version));
                            }
                            AnalysisMessage::FileClosed { path } => {
                                self.pending.remove(&path);
                                self.db.file_map.write().unwrap().remove(&path);
                            }
                            AnalysisMessage::FullReanalysis => {
                                // process immediately and break out of coalesce loop
                                self.reanalyze_all();
                                break;
                            }
                            AnalysisMessage::Shutdown => return,
                        }
                    }
                    // Now process all pending changes
                    for (path, (content, _version)) in self.pending.drain() {
                        self.analyze_file(&path, &content);
                    }
                }
                AnalysisMessage::FileClosed { path } => {
                    self.db.file_map.write().unwrap().remove(&path);
                }
                AnalysisMessage::FullReanalysis => {
                    self.reanalyze_all();
                }
            }
        }
    }

    fn analyze_file(&self, _path: &PathBuf, _content: &str) {
        // TODO: actual incremental pipeline
    }

    fn reanalyze_all(&self) {
        // TODO
    }
}
