use serde::{Serialize, Deserialize};

/// Build mode for compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BuildModeConfig {
    #[default]
    Debug,
    Release,
}

/// Configuration for the Glyim compiler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerConfig {
    /// Whether to use the query-driven incremental pipeline.
    #[serde(default)]
    pub incremental: bool,
    /// Build mode (debug, release).
    #[serde(default)]
    pub mode: BuildModeConfig,
    /// Target triple for cross-compilation.
    #[serde(default)]
    pub target: Option<String>,
    /// Whether to produce a bare binary (without standard runtime).
    #[serde(default)]
    pub bare: bool,
    /// Whether to run in JIT mode.
    #[serde(default)]
    pub jit: bool,
    /// Whether to compile in library mode (no main required).
    #[serde(default)]
    pub library_mode: bool,
    /// Coverage instrumentation mode.
    #[serde(default)]
    pub coverage: CoverageMode,
    /// Mutation testing configuration.
    #[serde(default)]
    pub mutation: MutationConfig,
    /// LSP server configuration.
    #[serde(default)]
    pub lsp: LspConfig,
    /// E-graph optimization configuration.
    #[serde(default)]
    pub egraph: EGraphConfig,
    /// Remote cache configuration.
    #[serde(default)]
    pub remote_cache: Option<RemoteCacheConfig>,
    /// Performance profiling.
    #[serde(default)]
    pub profiling: ProfilingConfig,
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            incremental: false,
            mode: BuildModeConfig::Debug,
            target: None,
            bare: false,
            jit: false,
            library_mode: false,
            coverage: CoverageMode::Off,
            mutation: MutationConfig::default(),
            lsp: LspConfig::default(),
            egraph: EGraphConfig::default(),
            remote_cache: None,
            profiling: ProfilingConfig::default(),
        }
    }
}

/// Controls the level of coverage instrumentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CoverageMode {
    #[default]
    Off,
    Function,
    Branch,
    Full,
}

/// Configuration for mutation testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationConfig {
    /// Mutation operators to apply.
    #[serde(default = "default_mutation_operators")]
    pub operators: Vec<String>,
    /// Maximum mutations per function.
    #[serde(default = "default_max_mutations")]
    pub max_mutations_per_fn: usize,
    /// Skip test functions when generating mutants.
    #[serde(default = "default_skip_tests")]
    pub skip_tests: bool,
}

fn default_mutation_operators() -> Vec<String> {
    vec![
        "plus-to-minus".into(),
        "minus-to-plus".into(),
        "mul-to-div".into(),
        "div-to-mul".into(),
        "eq-to-neq".into(),
        "neq-to-eq".into(),
        "and-to-or".into(),
        "or-to-and".into(),
        "not-elim".into(),
        "const-zero".into(),
        "stmt-del".into(),
        "cond-flip".into(),
    ]
}
fn default_max_mutations() -> usize { 50 }
fn default_skip_tests() -> bool { true }

impl Default for MutationConfig {
    fn default() -> Self {
        Self {
            operators: default_mutation_operators(),
            max_mutations_per_fn: default_max_mutations(),
            skip_tests: default_skip_tests(),
        }
    }
}

/// Configuration for the LSP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    /// TCP port to listen on (None = stdio).
    #[serde(default)]
    pub port: Option<u16>,
    /// Log file for debug output.
    #[serde(default)]
    pub log_file: Option<String>,
    /// Debounce interval for file change analysis (ms).
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
}

fn default_debounce_ms() -> u64 { 100 }

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            port: None,
            log_file: None,
            debounce_ms: default_debounce_ms(),
        }
    }
}

/// Configuration for the e-graph optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EGraphConfig {
    /// Maximum number of iterations for equality saturation.
    #[serde(default = "default_egraph_max_iterations")]
    pub max_iterations: usize,
    /// Maximum number of nodes in the e-graph before early termination.
    #[serde(default = "default_egraph_max_nodes")]
    pub max_nodes: usize,
    /// Maximum memory (in bytes) the e-graph may consume.
    #[serde(default = "default_egraph_memory_budget")]
    pub memory_budget: usize,
    /// Whether to use the invariant certificate for caching.
    #[serde(default = "default_egraph_use_invariant")]
    pub use_invariant_certificates: bool,
}

fn default_egraph_max_iterations() -> usize { 50 }
fn default_egraph_max_nodes() -> usize { 100_000 }
fn default_egraph_memory_budget() -> usize { 100 * 1024 * 1024 }
fn default_egraph_use_invariant() -> bool { true }

impl Default for EGraphConfig {
    fn default() -> Self {
        Self {
            max_iterations: default_egraph_max_iterations(),
            max_nodes: default_egraph_max_nodes(),
            memory_budget: default_egraph_memory_budget(),
            use_invariant_certificates: default_egraph_use_invariant(),
        }
    }
}

/// Configuration for remote cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCacheConfig {
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
}

/// Configuration for profiling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct ProfilingConfig {
    /// Whether profiling is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Whether to write a Chrome trace file.
    #[serde(default)]
    pub trace: bool,
    /// Whether to show spans as an indented tree.
    #[serde(default)]
    pub tree: bool,
}

