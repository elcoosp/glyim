//! Query-driven incremental compilation pipeline.
//!
//! Each pipeline stage is a memoized query keyed by the semantic hash
//! of its inputs. The QueryPipeline orchestrates the stages and manages
//! the incremental state, Merkle store, and dependency tracking.

use glyim_query::{QueryContext, Fingerprint, Dependency, IncrementalState};
use glyim_merkle::{MerkleStore, MerkleNode, MerkleNodeData, MerkleNodeHeader};
use glyim_macro_vfs::{ContentHash};
use glyim_interner::Interner;
use glyim_hir::{Hir, HirItem};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glyim_profiler::ProfileCollector;

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

/// The query-driven incremental compilation pipeline.
///
/// # Stability
/// *Stable.*
#[allow(dead_code)]
pub struct QueryPipeline {
    ctx: QueryContext,
    pub(crate) cache_dir: PathBuf,
    state: IncrementalState,
    config: PipelineConfig,
    report: IncrementalReport,
    pub(crate) merkle_store: Option<Arc<MerkleStore>>,
    /// Previous per‑function fingerprints loaded from Merkle root.
    prev_fingerprints: HashMap<String, Fingerprint>,
}

impl QueryPipeline {
    pub fn new(
        cache_dir: &Path,
        config: PipelineConfig,
    ) -> Self {
        let state = IncrementalState::load_or_create(cache_dir);
        let ctx = QueryContext::new();
        #[allow(clippy::arc_with_non_send_sync)]
        let merkle_store = glyim_macro_vfs::LocalContentStore::new(cache_dir.join("artifacts"))
            .ok()
            .map(|store| { Arc::new(MerkleStore::new(Arc::new(store))) });
        let mut prev = HashMap::new();
        // Load previous fingerprints from the Merkle root if available.
        if let Some(ref m) = merkle_store
            && let Some(root_hash) = m.resolve_name("fingerprints_root")
                && let Some(node) = m.get(&root_hash)
                    && let MerkleNodeData::HirItem { serialized, .. } = &node.data
                        && let Ok(map) = postcard::from_bytes::<HashMap<String, Fingerprint>>(serialized) {
                            prev = map;
                        }
        Self {
            ctx,
            cache_dir: cache_dir.to_path_buf(),
            state,
            config,
            report: IncrementalReport::default(),
            merkle_store,
            prev_fingerprints: prev,
        }
    }

    pub fn compile(
        &mut self,
        source: &str,
        input_path: &Path,
    ) -> Result<CompiledHir, PipelineError> {
        ProfileCollector::enter_stage(glyim_profiler::StageName::Parse);
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
            ProfileCollector::exit_stage(glyim_profiler::StageName::Parse, 1, 1, 0);
            // Return a dummy CompiledHir (quick path, data not used)
            // For safety, fall through to compile_source_to_hir
        } else {
            self.report.was_full_rebuild = true;
            self.state.record_source(&input_str, source_fp);
        }
        ProfileCollector::exit_stage(glyim_profiler::StageName::Parse, 1, 0, 1);
        ProfileCollector::enter_stage(glyim_profiler::StageName::TypeCheck);

        let compiled = compile_source_to_hir(
            source.to_string(),
            input_path,
            &self.config,
        )?;

        ProfileCollector::exit_stage(glyim_profiler::StageName::TypeCheck, 1, 0, 0);

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

    /// Compile incrementally: diff items, load cached objects for unchanged items,
    /// recompile only red items, store new objects in the Merkle store.
    #[allow(clippy::type_complexity)]
    #[allow(clippy::type_complexity)]
    pub fn compile_incremental(
        &mut self,
        source: &str,
        input_path: &Path,
    ) -> Result<(CompiledHir, Vec<(String, Vec<u8>)>), PipelineError> {
        // 1. Full compilation to get HIR + type info
        let compiled = self.compile(source, input_path)?;

        // 2. Compute per‑function fingerprints
        let current_fps: Vec<(String, Fingerprint)> = item_fingerprints(&compiled.hir, &compiled.interner);
        let current_map: HashMap<String, Fingerprint> = current_fps.iter().cloned().collect();

        // 3. Diff with previous
        let (red_names, green_names) = compute_item_diff(
            &compiled.hir,
            &compiled.interner,
            &self.prev_fingerprints,
        );

        let mut per_fn_objects: Vec<(String, Vec<u8>)> = Vec::new();

        // 4. Load green object code from Merkle cache
        if let Some(ref merkle) = self.merkle_store {
            for name in &green_names {
                let key = format!("obj:{}", name);
                if let Some(hash) = merkle.resolve_name(&key)
                    && let Some(node) = merkle.get(&hash)
                        && let MerkleNodeData::ObjectCode { bytes, .. } = &node.data {
                            per_fn_objects.push((name.clone(), bytes.clone()));
                        }
            }
        }

        // 5. Codegen only red items (indices)
        let red_indices: Vec<usize> = compiled.hir.items.iter()
            .enumerate()
            .filter(|(_, item)| {
                let name = match item {
                    glyim_hir::HirItem::Fn(f) => compiled.interner.resolve(f.name).to_string(),
                    _ => return false,
                };
                red_names.contains(&name)
            })
            .map(|(i, _)| i)
            .collect();

        if !red_indices.is_empty() {
            let new_objects = glyim_codegen_llvm::compile_items_to_objects(
                &compiled.hir,
                &compiled.mono_hir,
                &compiled.interner,
                &compiled.merged_types,
                &red_indices,
            ).map_err(PipelineError::Codegen)?;

            // Store new objects in Merkle cache
            if let Some(ref merkle) = self.merkle_store {
                for (name, obj_data) in &new_objects {
                    let node = MerkleNode {
                        hash: ContentHash::ZERO,
                        children: vec![],
                        data: MerkleNodeData::ObjectCode {
                            symbol_name: name.clone(),
                            bytes: obj_data.clone(),
                        },
                        header: MerkleNodeHeader {
                            data_type_tag: 0x04, // ObjectCode
                            child_count: 0,
                        },
                    };
                    let hash = merkle.put(node);
                    merkle.register_name(&format!("obj:{}", name), hash);
                }
            }
            per_fn_objects.extend(new_objects);
        }

        // 6. Update previous fingerprints
        self.prev_fingerprints = current_map;

        // Save fingerprints map into Merkle store for next run
        if let Some(ref merkle) = self.merkle_store {
            let fp_bytes = postcard::to_allocvec(&self.prev_fingerprints).unwrap_or_default();
            let fp_node = MerkleNode {
                hash: ContentHash::ZERO,
                children: vec![],
                data: MerkleNodeData::HirItem {
                    kind: "fingerprints".to_string(),
                    name: "root".to_string(),
                    serialized: fp_bytes,
                },
                header: MerkleNodeHeader {
                    data_type_tag: 0x02,
                    child_count: 0,
                },
            };
            let root_hash = merkle.put(fp_node);
            merkle.register_name("fingerprints_root", root_hash);
            merkle.flush();
        }

        Ok((compiled, per_fn_objects))
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
