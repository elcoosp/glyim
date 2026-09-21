use crate::domain_types::{ApplyLimits, BannedPattern, DependencyRule};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
/// PilotConfig.
pub struct PilotConfig {
    /// Struct.
    pub server: ServerConfig,
    #[serde(default)]
    /// Struct.
    pub defaults: DefaultsConfig,
    /// Struct.
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    /// Struct.
    pub execution: ExecutionConfig,
    #[serde(default)]
    /// Struct.
    pub gates: GatesConfig,
    #[serde(default)]
    /// Struct.
    pub context: ContextConfig,
    #[serde(default)]
    /// Struct.
    pub dispatch: DispatchConfig,
    #[serde(default)]
    /// Struct.
    pub limits: ApplyLimits,
}

impl PilotConfig {
    /// default_for_testing.
    pub fn default_for_testing() -> Self {
        let mut providers = HashMap::new();
        providers.insert("test-provider".into(), ProviderConfig::default());
        Self {
            server: ServerConfig::default(),
            defaults: DefaultsConfig::default(),
            providers,
            execution: ExecutionConfig::default(),
            gates: GatesConfig::default(),
            context: ContextConfig::default(),
            dispatch: DispatchConfig::default(),
            limits: ApplyLimits::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// ServerConfig.
pub struct ServerConfig {
    #[serde(default = "default_port")]
    /// Struct.
    pub port: u16,
    #[serde(default = "default_host")]
    /// Struct.
    pub host: String,
}
fn default_port() -> u16 {
    8420
}
fn default_host() -> String {
    "127.0.0.1".into()
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// DefaultsConfig.
pub struct DefaultsConfig {
    #[serde(default)]
    /// Struct.
    pub provider: String,
    #[serde(default)]
    /// Struct.
    pub auto_execute: bool,
    #[serde(default = "default_max_turns")]
    /// Struct.
    pub max_turns: u32,
    #[serde(default = "default_true")]
    /// Struct.
    pub retry_on_rate_limit: bool,
    #[serde(default = "default_retry_max_wait")]
    /// Struct.
    pub retry_max_wait: u64,
}
fn default_max_turns() -> u32 {
    50
}
fn default_true() -> bool {
    true
}
fn default_retry_max_wait() -> u64 {
    120
}
impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            auto_execute: false,
            max_turns: default_max_turns(),
            retry_on_rate_limit: true,
            retry_max_wait: default_retry_max_wait(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// ProviderConfig.
pub struct ProviderConfig {
    #[serde(default = "default_true")]
    /// Struct.
    pub enabled: bool,
    #[serde(default)]
    /// Struct.
    pub url: String,
    #[serde(default = "default_max_concurrent")]
    /// Struct.
    pub max_concurrent: usize,
    #[serde(default = "default_cooldown")]
    /// Struct.
    pub rate_limit_cooldown: u64,
    #[serde(default)]
    /// Struct.
    pub error_patterns: Vec<String>,
    #[serde(default = "default_input_selector")]
    /// Struct.
    pub input_selector: String,
    #[serde(default = "default_send_selector")]
    /// Struct.
    pub send_selector: String,
    #[serde(default)]
    /// Struct.
    pub streaming_indicator: String,
    #[serde(default)]
    /// Struct.
    pub assistant_selector: String,
    #[serde(default = "default_code_block_selector")]
    /// Struct.
    pub code_block_selector: String,
}
fn default_max_concurrent() -> usize {
    3
}
fn default_cooldown() -> u64 {
    60
}
fn default_input_selector() -> String {
    "textarea".into()
}
fn default_send_selector() -> String {
    "button[type='submit']".into()
}
fn default_code_block_selector() -> String {
    "pre code".into()
}
impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            url: String::new(),
            max_concurrent: default_max_concurrent(),
            rate_limit_cooldown: default_cooldown(),
            error_patterns: Vec::new(),
            input_selector: default_input_selector(),
            send_selector: default_send_selector(),
            streaming_indicator: String::new(),
            assistant_selector: String::new(),
            code_block_selector: default_code_block_selector(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// ExecutionConfig.
pub struct ExecutionConfig {
    #[serde(default = "default_worktree_base")]
    /// Struct.
    pub worktree_base: String,
    #[serde(default = "default_require_confirmation")]
    /// Struct.
    pub require_confirmation: String,
    #[serde(default = "default_dangerous_patterns")]
    /// Struct.
    pub dangerous_patterns: Vec<String>,
    #[serde(default = "default_max_fix_rounds")]
    /// Struct.
    pub max_fix_rounds: u32,
    #[serde(default = "default_command_timeout")]
    /// Struct.
    pub command_timeout: u64,
    #[serde(default = "default_branch")]
    /// Struct.
    pub default_branch: String,
    #[serde(default = "default_branch_version")]
    /// Struct.
    pub branch_version: String,
}
fn default_worktree_base() -> String {
    "../glyim-worktrees".into()
}
fn default_require_confirmation() -> String {
    "first".into()
}
fn default_dangerous_patterns() -> Vec<String> {
    vec![
        "rm -rf".into(),
        "git push".into(),
        "git reset --hard".into(),
        "cargo publish".into(),
        "sudo".into(),
    ]
}
fn default_max_fix_rounds() -> u32 {
    5
}
fn default_command_timeout() -> u64 {
    300
}
fn default_branch() -> String {
    "main".into()
}
fn default_branch_version() -> String {
    "v0.1.0".into()
}
impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            worktree_base: default_worktree_base(),
            require_confirmation: default_require_confirmation(),
            dangerous_patterns: default_dangerous_patterns(),
            max_fix_rounds: default_max_fix_rounds(),
            command_timeout: default_command_timeout(),
            default_branch: default_branch(),
            branch_version: default_branch_version(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
/// GateLevel.
pub enum GateLevel {
    /// Variant.
    Relaxed,
    #[default]
    /// Variant.
    Normal,
    /// Variant.
    Strict,
    /// Variant.
    Production,
}
impl std::fmt::Display for GateLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Relaxed => write!(f, "relaxed"),
            Self::Normal => write!(f, "normal"),
            Self::Strict => write!(f, "strict"),
            Self::Production => write!(f, "production"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
/// GatesConfig.
pub struct GatesConfig {
    #[serde(default)]
    /// Struct.
    pub level: GateLevel,
    #[serde(default)]
    /// Struct.
    pub commit: CommitGatesConfig,
    #[serde(default)]
    /// Struct.
    pub done: DoneGatesConfig,
    #[serde(default)]
    /// Struct.
    pub banned_patterns: Vec<BannedPattern>,
    #[serde(default)]
    /// Struct.
    pub architecture_rules: Vec<DependencyRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
/// CommitGatesConfig.
pub struct CommitGatesConfig {
    /// Struct.
    pub fmt: Option<bool>,
    /// Struct.
    pub check: Option<bool>,
    /// Struct.
    pub clippy: Option<bool>,
    /// Struct.
    pub test: Option<bool>,
    /// Struct.
    pub banned_patterns: Option<bool>,
    /// Struct.
    pub architecture: Option<bool>,
    /// Struct.
    pub contracts: Option<bool>,
}

impl CommitGatesConfig {
    /// resolve.
    pub fn resolve(
        &self,
        level: GateLevel,
        default_branch: String,
        branch_version: String,
    ) -> ResolvedCommitGates {
        let d = level.commit_defaults();
        ResolvedCommitGates {
            fmt: self.fmt.unwrap_or(d.fmt),
            check: self.check.unwrap_or(d.check),
            clippy: self.clippy.unwrap_or(d.clippy),
            test: self.test.unwrap_or(d.test),
            banned_patterns: self.banned_patterns.unwrap_or(d.banned_patterns),
            architecture: self.architecture.unwrap_or(d.architecture),
            contracts: self.contracts.unwrap_or(d.contracts),
            default_branch,
            branch_version,
        }
    }
}

#[derive(Debug, Clone)]
/// ResolvedCommitGates.
pub struct ResolvedCommitGates {
    /// Struct.
    pub fmt: bool,
    /// Struct.
    pub check: bool,
    /// Struct.
    pub clippy: bool,
    /// Struct.
    pub test: bool,
    /// Struct.
    pub banned_patterns: bool,
    /// Struct.
    pub architecture: bool,
    /// Struct.
    pub contracts: bool,
    /// Struct.
    pub default_branch: String,
    /// Struct.
    pub branch_version: String,
}

struct CommitDefaults {
    fmt: bool,
    check: bool,
    clippy: bool,
    test: bool,
    banned_patterns: bool,
    architecture: bool,
    contracts: bool,
}
impl GateLevel {
    fn commit_defaults(self) -> CommitDefaults {
        match self {
            Self::Relaxed => CommitDefaults {
                fmt: true,
                check: true,
                clippy: false,
                test: false,
                banned_patterns: false,
                architecture: false,
                contracts: false,
            },
            Self::Normal => CommitDefaults {
                fmt: true,
                check: true,
                clippy: true,
                test: true,
                banned_patterns: false,
                architecture: false,
                contracts: false,
            },
            Self::Strict | Self::Production => CommitDefaults {
                fmt: true,
                check: true,
                clippy: true,
                test: true,
                banned_patterns: true,
                architecture: true,
                contracts: true,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// DoneGatesConfig.
pub struct DoneGatesConfig {
    /// Struct.
    pub dead_code: Option<bool>,
    /// Struct.
    pub coverage: Option<bool>,
    #[serde(default = "default_coverage_min")]
    /// Struct.
    pub coverage_min: f64,
    /// Struct.
    pub mutation: Option<bool>,
    #[serde(default = "default_mutation_kill_rate")]
    /// Struct.
    pub mutation_kill_rate: f64,
    /// Struct.
    pub workspace_check: Option<bool>,
    /// Struct.
    pub audit: Option<bool>,
    /// Struct.
    pub self_review: Option<bool>,
}
fn default_coverage_min() -> f64 {
    0.80
}
fn default_mutation_kill_rate() -> f64 {
    0.75
}
impl Default for DoneGatesConfig {
    fn default() -> Self {
        Self {
            dead_code: None,
            coverage: None,
            coverage_min: default_coverage_min(),
            mutation: None,
            mutation_kill_rate: default_mutation_kill_rate(),
            workspace_check: None,
            audit: None,
            self_review: None,
        }
    }
}

impl DoneGatesConfig {
    /// resolve.
    pub fn resolve(&self, level: GateLevel) -> ResolvedDoneGates {
        let d = level.done_defaults();
        ResolvedDoneGates {
            dead_code: self.dead_code.unwrap_or(d.dead_code),
            coverage: self.coverage.unwrap_or(d.coverage),
            coverage_min: self.coverage_min,
            mutation: self.mutation.unwrap_or(d.mutation),
            mutation_kill_rate: self.mutation_kill_rate,
            workspace_check: self.workspace_check.unwrap_or(d.workspace_check),
            audit: self.audit.unwrap_or(d.audit),
            self_review: self.self_review.unwrap_or(d.self_review),
        }
    }
}

#[derive(Debug, Clone)]
/// ResolvedDoneGates.
pub struct ResolvedDoneGates {
    /// Struct.
    pub dead_code: bool,
    /// Struct.
    pub coverage: bool,
    /// Struct.
    pub coverage_min: f64,
    /// Struct.
    pub mutation: bool,
    /// Struct.
    pub mutation_kill_rate: f64,
    /// Struct.
    pub workspace_check: bool,
    /// Struct.
    pub audit: bool,
    /// Struct.
    pub self_review: bool,
}

#[allow(dead_code)]
struct DoneDefaults {
    dead_code: bool,
    coverage: bool,
    coverage_min: f64,
    mutation: bool,
    mutation_kill_rate: f64,
    workspace_check: bool,
    audit: bool,
    self_review: bool,
}
impl GateLevel {
    fn done_defaults(self) -> DoneDefaults {
        match self {
            Self::Relaxed | Self::Normal => DoneDefaults {
                dead_code: false,
                coverage: false,
                coverage_min: 0.80,
                mutation: false,
                mutation_kill_rate: 0.75,
                workspace_check: false,
                audit: false,
                self_review: false,
            },
            Self::Strict => DoneDefaults {
                dead_code: true,
                coverage: false,
                coverage_min: 0.80,
                mutation: false,
                mutation_kill_rate: 0.75,
                workspace_check: true,
                audit: false,
                self_review: false,
            },
            Self::Production => DoneDefaults {
                dead_code: true,
                coverage: true,
                coverage_min: 0.80,
                mutation: true,
                mutation_kill_rate: 0.75,
                workspace_check: true,
                audit: true,
                self_review: true,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// ContextConfig.
pub struct ContextConfig {
    #[serde(default = "default_max_context_tokens")]
    /// Struct.
    pub max_context_tokens: usize,
    #[serde(default)]
    /// Struct.
    pub providers: HashMap<String, ProviderContextConfig>,
}
fn default_max_context_tokens() -> usize {
    15000
}
impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: default_max_context_tokens(),
            providers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// ProviderContextConfig.
pub struct ProviderContextConfig {
    #[serde(default = "default_max_context_tokens")]
    /// Struct.
    pub max_context_tokens: usize,
}
impl Default for ProviderContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: default_max_context_tokens(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// DispatchConfig.
pub struct DispatchConfig {
    #[serde(default = "default_strategy")]
    /// Struct.
    pub strategy: String,
    #[serde(default = "default_true")]
    /// Struct.
    pub fallback_on_rate_limit: bool,
    #[serde(default = "default_max_reassign")]
    /// Struct.
    pub max_reassign_attempts: u32,
}
fn default_strategy() -> String {
    "most_slots_first".into()
}
fn default_max_reassign() -> u32 {
    2
}
impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            fallback_on_rate_limit: true,
            max_reassign_attempts: default_max_reassign(),
        }
    }
}
