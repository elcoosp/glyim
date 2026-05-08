use std::collections::HashMap;
use std::time::Duration;

/// A collected profile for a single compilation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompilationProfile {
    pub id: u64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub total_duration: Duration,
    pub stages: HashMap<StageName, StageProfile>,
    pub memory: MemoryProfile,
    pub incremental: IncrementalProfile,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StageName {
    MacroExpand,
    Parse,
    Declarations,
    Lower,
    TypeCheck,
    Desugar,
    Monomorphize,
    EGraphOptimize,
    Codegen,
    Link,
    MerkleStore,
    SemanticHash,
    IncrementalStatePersist,
    LspAnalysis,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StageProfile {
    pub duration: Duration,
    pub items_processed: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub bytes_allocated: usize,
    pub skipped: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryProfile {
    pub peak_rss: usize,
    pub total_allocated: usize,
    pub total_freed: usize,
    pub allocation_count: usize,
    pub deallocation_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IncrementalProfile {
    pub red_items: usize,
    pub green_items: usize,
    pub total_items: usize,
    pub cache_hit_ratio: f64,
}
