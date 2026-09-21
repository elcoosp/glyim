use crate::AnalysisDatabase;
use crate::database::SourceMap;
use crate::dep_graph::DependencyGraph;
use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::{lex, parse_to_syntax};
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use notify::RecommendedWatcher;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tracing::debug;

/// AnalysisMessage.
pub enum AnalysisMessage {
    /// Variant.
    FileChanged {
        /// Struct.
        path: PathBuf,
        /// Struct.
        content: String,
        /// Struct.
        version: i32,
    },
    /// Variant.
    FileClosed {
        /// Struct.
        path: PathBuf,
    },
    /// Variant.
    Shutdown,
}

/// AnalysisDriver.
pub struct AnalysisDriver {
    db: Arc<AnalysisDatabase>,
    rx: Receiver<AnalysisMessage>,
    #[allow(unused)]
    cache_dir: PathBuf,
    dep_graph: Arc<parking_lot::RwLock<DependencyGraph>>,
    _watcher: Option<RecommendedWatcher>, // kept for drop
}

impl AnalysisDriver {
    /// new.
    pub fn new(
        db: Arc<AnalysisDatabase>,
        rx: Receiver<AnalysisMessage>,
        cache_dir: PathBuf,
    ) -> Self {
        // Create a channel for file system events, but we won't spawn a thread for now
        // to keep compilation simple. The watcher can be added later.
        let _watcher: Option<RecommendedWatcher> = None;
        Self {
            db,
            rx,
            cache_dir,
            dep_graph: Arc::new(parking_lot::RwLock::new(DependencyGraph::new())),
            _watcher,
        }
    }

    /// run.
    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                AnalysisMessage::FileChanged {
                    path,
                    content,
                    version: _,
                } => {
                    self.analyze_file(&path, &content).await;
                }
                AnalysisMessage::FileClosed { path } => {
                    self.db.file_map.write().remove(&path);
                    self.dep_graph.write().clear_deps(&path);
                }
                AnalysisMessage::Shutdown => break,
            }
        }
    }

    async fn analyze_file(&self, path: &PathBuf, content: &str) {
        self.dep_graph.write().clear_deps(path);

        let file_id = { self.db.file_map.write().get_or_create(path) };
        let sm = SourceMap::new(path.clone(), file_id, content.to_string());
        self.db.source_maps.write().insert(file_id, sm.clone());

        let crate_id = CrateId::from_raw(0);
        let lex_result = lex(content, file_id);
        let parse_result = parse_to_syntax(content, file_id);
        let mut interner = Interner::new();
        let (def_map, def_diagnostics) =
            build_def_map(&parse_result.root, crate_id, interner.clone());

        let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);

        // Tier 6.4: run the type checker so completions/hover can resolve
        // expression types. Mirrors the pipeline's `typeck_crate` invocation.
        let ty_ctx_mut = glyim_type::TyCtxMut::new(interner.clone());
        let trait_ctx = glyim_solve::TraitContext::new();
        let mut solver = glyim_solve::SimpleTraitSolver::new(&trait_ctx);
        let (ty_ctx, typeck_result) =
            glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);

        self.extract_dependencies(path, &hir, &interner);

        self.db
            .symbol_index
            .write()
            .build_from_hir(file_id, &hir, &interner);
        self.db
            .reference_graph
            .write()
            .build_from_hir(file_id, &hir, &interner);
        self.db.hirs.write().insert(file_id, hir);
        self.db
            .typeck
            .write()
            .insert(file_id, (std::sync::Arc::new(ty_ctx), typeck_result));

        let mut all_diagnostics = Vec::new();
        all_diagnostics.extend(lex_result.diagnostics);
        all_diagnostics.extend(parse_result.diagnostics);
        all_diagnostics.extend(def_diagnostics);

        let lsp_diagnostics =
            crate::diagnostics::convert_diagnostics(file_id, &sm, &all_diagnostics);
        if lsp_diagnostics.is_empty() {
            self.db.diagnostics.write().remove(&file_id);
            self.db.raw_diagnostics.write().remove(&file_id);
        } else {
            self.db.diagnostics.write().insert(file_id, lsp_diagnostics);
            self.db
                .raw_diagnostics
                .write()
                .insert(file_id, all_diagnostics.clone());
        }

        debug!(
            "Analyzed file {:?} with {} diagnostics",
            path,
            all_diagnostics.len()
        );
    }

    fn extract_dependencies(
        &self,
        _path: &PathBuf,
        _hir: &glyim_hir::CrateHir,
        _interner: &glyim_core::Interner,
    ) {
        // Placeholder for dependency extraction
    }
}
