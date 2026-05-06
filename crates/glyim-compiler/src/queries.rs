//! Query-driven incremental compilation pipeline.
//!
//! Each pipeline stage is a memoized query keyed by the semantic hash
//! of its inputs. The QueryPipeline orchestrates the stages and manages
//! the incremental state, Merkle store, and dependency tracking.

use glyim_query::{QueryContext, Fingerprint, Dependency, IncrementalState};
use glyim_merkle::MerkleStore;
use glyim_interner::Interner;
use glyim_hir::{Hir, HirItem};              // Hir, HirItem used by item_fingerprints
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::pipeline::{
    CompiledHir, PipelineConfig, PipelineError,
    compile_source_to_hir,
};

/// Diagnostic report for incremental compilation.
#[derive(Debug, Clone, Default)]
pub struct IncrementalReport {
    pub total_items: usize,
    pub red_items: Vec<ItemReport>,
    pub green_items: Vec<String>,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub stage_timings: Vec<(String, Duration)>,
    pub total_elapsed: Duration,
    pub was_full_rebuild: bool,
}

#[derive(Debug, Clone)]
pub struct ItemReport {
    pub name: String,
    pub reason: RedReason,
    pub elapsed: Duration,
    pub stages_executed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedReason {
    SourceChanged,
    DependencyChanged(String),
    NotCached,
    InvariantCertificateChanged,
    RuleSetUpdated,
}

/// Orchestrates the query-driven incremental compilation pipeline.
pub struct QueryPipeline {
    /// The query context for memoization.
    ctx: QueryContext,
    /// Directory for persistent incremental state.
    cache_dir: PathBuf,
    /// The incremental state (source hashes, item hashes, dep graph).
    state: IncrementalState,
    /// Compilation configuration.
    config: PipelineConfig,
    /// Incremental build report.
    report: IncrementalReport,
    /// The underlying CAS store for Merkle artifact caching.
    merkle_store: Option<Arc<MerkleStore>>,
}

impl QueryPipeline {
    /// Create a new query-driven pipeline.
    pub fn new(
        cache_dir: &Path,
        config: PipelineConfig,
    ) -> Self {
        let state = IncrementalState::load_or_create(cache_dir);
        let ctx = QueryContext::new();
        // Initialize MerkleStore if possible
        let merkle_store = std::fs::create_dir_all(cache_dir.join("artifacts"))
            .ok()
            .and_then(|_| {
                glyim_macro_vfs::LocalContentStore::new(cache_dir.join("artifacts"))
                    .ok()
                    .map(|store| Arc::new(MerkleStore::new(Arc::new(store))))
            });

        Self {
            ctx,
            cache_dir: cache_dir.to_path_buf(),
            state,
            config,
            report: IncrementalReport::default(),
            merkle_store,
        }
    }

    /// Compile source code using the incremental query pipeline.
    pub fn compile(
        &mut self,
        source: &str,
        input_path: &Path,
    ) -> Result<CompiledHir, PipelineError> {
        let start = Instant::now();
        self.report = IncrementalReport::default();

        // Step 1: Compute source fingerprint and check if anything changed
        let source_fp = Fingerprint::of(source.as_bytes());
        let input_str = input_path.to_string_lossy().to_string();
        let module_key = Fingerprint::combine(
            Fingerprint::of_str(&input_str),
            source_fp,
        );

        // Step 2: If source unchanged, try to return cached result
        if self.ctx.is_green(&module_key) {
            self.report.cache_hits += 1;
            self.report.total_elapsed = start.elapsed();
            self.report.was_full_rebuild = false;
            // Need to re-run the full pipeline since we don't yet store CompiledHir in cache
            // (Phase 4C will add per-item caching)
            tracing::info!("Incremental: source unchanged, but running full pipeline (per-item cache not yet implemented)");
        } else {
            self.report.was_full_rebuild = true;
            // Record the change
            self.state.record_source(&input_str, source_fp);
        }

        // Step 3: Run the pipeline (for now, full linear pipeline)
        let compiled = compile_source_to_hir(
            source.to_string(),
            input_path,
            &self.config,
        )?;

        // Record the result in the query context
        // In Phase 4B/C, this will store per-item artifacts in MerkleStore
        self.ctx.insert(
            module_key,
            Arc::new(()), // placeholder - Phase 4B will store actual CompiledHir
            source_fp,
            vec![Dependency::file(&input_str, source_fp)],
        );

        // Save incremental state
        if let Err(e) = self.state.save() {
            tracing::warn!("Failed to save incremental state: {e}");
        }

        self.report.total_elapsed = start.elapsed();
        Ok(compiled)
    }

    /// Get a reference to the incremental report.
    pub fn report(&self) -> &IncrementalReport {
        &self.report
    }

    /// Get a reference to the query context.
    pub fn ctx(&self) -> &QueryContext {
        &self.ctx
    }
}

/// Compute a per-item fingerprint from the HIR.
pub fn item_fingerprints(hir: &Hir, interner: &Interner) -> Vec<(String, Fingerprint)> {
    use glyim_hir::semantic_hash::semantic_hash_item;
    hir.items.iter().filter_map(|item| {
        let name = match item {
            HirItem::Fn(f) => interner.resolve(f.name).to_string(),
            HirItem::Struct(s) => interner.resolve(s.name).to_string(),
            HirItem::Enum(e) => interner.resolve(e.name).to_string(),
            _ => return None,
        };
        let hash = semantic_hash_item(item, interner);
        Some((name, Fingerprint::of(hash.as_bytes().as_slice())))
    }).collect()
}
