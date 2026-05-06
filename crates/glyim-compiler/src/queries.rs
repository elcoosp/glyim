//! Query-driven incremental compilation pipeline.
//!
//! Each pipeline stage is a memoized query keyed by the semantic hash
//! of its inputs. The QueryPipeline orchestrates the stages and manages
//! the incremental state, Merkle store, and dependency tracking.

use glyim_query::{QueryContext, Fingerprint, Dependency, IncrementalState};
use glyim_merkle::{MerkleStore, MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::{ContentHash, ContentStore};
use glyim_interner::Interner;
use glyim_hir::{Hir, HirItem};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::pipeline::{
    CompiledHir, PipelineConfig, PipelineError,
    compile_source_to_hir,
};


/// Compute which items changed since the last compilation.
pub fn compute_item_diff(
    hir: &Hir,
    interner: &Interner,
    prev_hashes: &HashMap<String, Fingerprint>,
) -> (Vec<String>, Vec<String>) {
    let current = item_fingerprints(hir, interner);
    let current_map: HashMap<String, Fingerprint> = current.into_iter().collect();

    let mut red = Vec::new();
    let mut green = Vec::new();

    for (name, fp) in &current_map {
        match prev_hashes.get(name) {
            Some(old_fp) if old_fp == fp => green.push(name.clone()),
            _ => red.push(name.clone()),
        }
    }

    (red, green)
}

/// Store a per-item compilation artifact in the Merkle store.
pub fn store_item_artifact(
    merkle: &MerkleStore,
    item_name: &str,
    artifact_kind: &str,
    data: &[u8],
) -> ContentHash {
    let node = MerkleNode {
        hash: ContentHash::ZERO,
        children: vec![],
        data: MerkleNodeData::HirItem {
            kind: artifact_kind.to_string(),
            name: item_name.to_string(),
            serialized: data.to_vec(),
        },
        header: MerkleNodeHeader {
            data_type_tag: 0x02, // HirItem
            child_count: 0,
        },
    };
    merkle.put(node)
}

/// Load a per-item artifact from the Merkle store.
pub fn load_item_artifact(
    merkle: &MerkleStore,
    hash: &ContentHash,
) -> Option<Vec<u8>> {
    merkle.get(hash).map(|node| {
        match node.data {
            MerkleNodeData::HirItem { serialized, .. } => serialized,
            _ => vec![],
        }
    })
}

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
    ctx: QueryContext,
    cache_dir: PathBuf,
    state: IncrementalState,
    config: PipelineConfig,
    report: IncrementalReport,
    merkle_store: Option<Arc<MerkleStore>>,
}

impl QueryPipeline {
    pub fn new(
        cache_dir: &Path,
        config: PipelineConfig,
    ) -> Self {
        let state = IncrementalState::load_or_create(cache_dir);
        let ctx = QueryContext::new();
        let merkle_store = glyim_macro_vfs::LocalContentStore::new(cache_dir.join("artifacts"))
            .ok()
            .map(|store| Arc::new(MerkleStore::new(Arc::new(store))));
        Self {
            ctx,
            cache_dir: cache_dir.to_path_buf(),
            state,
            config,
            report: IncrementalReport::default(),
            merkle_store,
        }
    }

    pub fn compile(
        &mut self,
        source: &str,
        input_path: &Path,
    ) -> Result<CompiledHir, PipelineError> {
        let start = Instant::now();
        self.report = IncrementalReport::default();

        let source_fp = Fingerprint::of(source.as_bytes());
        let input_str = input_path.to_string_lossy().to_string();
        let module_key = Fingerprint::combine(
            Fingerprint::of_str(&input_str),
            source_fp,
        );

        if self.ctx.is_green(&module_key) {
            self.report.cache_hits += 1;
            self.report.total_elapsed = start.elapsed();
            self.report.was_full_rebuild = false;
        } else {
            self.report.was_full_rebuild = true;
            self.state.record_source(&input_str, source_fp);
        }

        let compiled = compile_source_to_hir(
            source.to_string(),
            input_path,
            &self.config,
        )?;

        self.ctx.insert(
            module_key,
            Arc::new(()),
            source_fp,
            vec![Dependency::file(&input_str, source_fp)],
        );

        if let Err(e) = self.state.save() {
            tracing::warn!("Failed to save incremental state: {e}");
        }

        self.report.total_elapsed = start.elapsed();
        Ok(compiled)
    }

    pub fn report(&self) -> &IncrementalReport {
        &self.report
    }

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
