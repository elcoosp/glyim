//! Domain types shared between config and implementation modules.
//! Defined here so `config` does not depend on `applier` or `gates`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// ApplyLimits.
pub struct ApplyLimits {
    /// Struct.
    pub max_file_size: usize,
    /// Struct.
    pub max_total_content: usize,
    /// Struct.
    pub max_ops_per_block: usize,
}

impl Default for ApplyLimits {
    fn default() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024,
            max_total_content: 50 * 1024 * 1024,
            max_ops_per_block: 100,
        }
    }
}

impl ApplyLimits {
    /// strict.
    pub fn strict() -> Self {
        Self {
            max_file_size: 1024 * 1024,
            max_total_content: 5 * 1024 * 1024,
            max_ops_per_block: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// BannedPattern.
pub struct BannedPattern {
    /// Struct.
    pub pattern: String,
    /// Struct.
    pub description: String,
}

impl BannedPattern {
    /// new.
    pub fn new(pattern: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            description: description.into(),
        }
    }
}

/// default_banned_patterns.
pub fn default_banned_patterns() -> Vec<BannedPattern> {
    vec![
        BannedPattern::new("todo!()", "`todo!()` in non-test code"),
        BannedPattern::new("unwrap()", "`.unwrap()` in non-test code"),
        BannedPattern::new("panic!()", "`panic!()` in non-test code"),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// DependencyRule.
pub struct DependencyRule {
    /// Struct.
    pub from_crate: String,
    /// Struct.
    pub forbidden_dep: String,
    /// Struct.
    pub reason: String,
}

/// default_architecture_rules.
pub fn default_architecture_rules() -> Vec<DependencyRule> {
    vec![
        DependencyRule {
            from_crate: "glyim-frontend".into(),
            forbidden_dep: "glyim-type".into(),
            reason: "frontend must not depend on type directly".into(),
        },
        DependencyRule {
            from_crate: "glyim-frontend".into(),
            forbidden_dep: "glyim-ir".into(),
            reason: "frontend must not depend on IR".into(),
        },
        DependencyRule {
            from_crate: "glyim-syntax".into(),
            forbidden_dep: "glyim-ir".into(),
            reason: "syntax must not depend on IR".into(),
        },
        DependencyRule {
            from_crate: "glyim-type".into(),
            forbidden_dep: "glyim-codegen".into(),
            reason: "type must not depend on codegen".into(),
        },
    ]
}
