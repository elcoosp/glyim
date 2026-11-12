//! `ledger/policy.toml` — the M1.8 level-computation policy schema.
//!
//! The master plan (§6 M1.8) defines the level engine as a pure function of
//! *(registry, evidence, policy)*. This module owns the third input: a strict
//! (`deny_unknown_fields`) TOML schema declaring
//!
//! - the per-level evidence rules `[levels.L1]` … `[levels.L4]` (a rule is a
//!   minimum count of distinct fixtures whose evidence matches a
//!   mode/outcome/backend predicate),
//! - whether open-gap caps apply (`[gap_caps]`),
//! - the hardening thresholds `[hardening]` (plan's "CPU-hour thresholds"):
//!   the fuzz-hour and mutation-kill gates an L5 target implies. No evidence
//!   kind records these yet, so the engine reports them as *unmeasured*
//!   rather than pretending they are satisfied.
//!
//! `L0` is the registry-declaration baseline (no rules can strengthen it) and
//! `L5` is deliberately *not* expressible as evidence rules: a rule table for
//! L5 would let a policy claim hardening the evidence store cannot measure,
//! which is exactly the "typed status" the plan's C1 constitution forbids.
//!
//! Loading is deterministic and diagnostic-bearing like every other loader in
//! this crate: failures are sorted [`Diagnostic`]s, never panics, and any
//! error-severity diagnostic refuses the whole policy (a half-accepted policy
//! would compute half-typed status).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::{Backend, Diagnostic, Level, Severity};

/// Schema tag pinned for the policy file shape.
pub const POLICY_SCHEMA: &str = "glyim.ledger-policy/1";

/// Canonical test-mode spellings a rule's `mode` (or an evidence record's
/// `mode`) may carry — the same set `glyim-test`'s `TestMode::dir_name`
/// produces. Shared by the policy validation and the ledger-side evidence
/// validation so both sides reject the same vocabulary.
pub const CANONICAL_MODES: [&str; 5] =
    ["compile-pass", "compile-fail", "ui", "run-pass", "run-fail"];

/// Canonical outcome spellings (mirror of the harness `EvidenceOutcome`
/// serde spelling).
pub const CANONICAL_OUTCOMES: [&str; 3] = ["passed", "failed", "ignored"];

/// Canonical backend spellings (mirror of [`Backend`]'s serde spelling);
/// evidence records declaring anything else are invalid.
pub const CANONICAL_BACKENDS: [&str; 4] = ["gam", "llvm", "cranelift", "bytecode"];

// -- diagnostics -----------------------------------------------------------

/// `ledger/policy.toml` is missing or unreadable.
pub const CODE_POLICY_LOAD: &str = "LEDGER-POLICY-LOAD";
/// `ledger/policy.toml` does not match the policy schema.
pub const CODE_POLICY_SCHEMA: &str = "LEDGER-POLICY-SCHEMA";
/// A policy rule is malformed (mode/outcome/count/level key/threshold).
pub const CODE_POLICY_RULE: &str = "LEDGER-POLICY-RULE";

fn diagnostic(code: &'static str, message: String) -> Diagnostic {
    Diagnostic {
        code,
        message,
        severity: Severity::Error,
    }
}

// -- schema types -----------------------------------------------------------

/// One evidence predicate: a minimum count of distinct fixtures whose
/// records match the mode/outcome/backend triple.
///
/// `outcome` is validated against [`CANONICAL_OUTCOMES`] and `mode` against
/// [`CANONICAL_MODES`] at load time; `backend`, when pinned, must be a
/// [`Backend`] spelling. Distinctness is by fixture content hash, so a
/// fixture run twice (two records) still counts once.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRule {
    /// Canonical test mode the evidence must have run in.
    pub mode: String,
    /// Outcome the evidence must show (`passed` for positive evidence).
    pub outcome: String,
    /// Minimum number of distinct fixtures satisfying the predicate.
    pub count: usize,
    /// Backend the evidence must have run on; `None` matches any backend
    /// the feature declares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<Backend>,
}

/// Hardening thresholds an `L5` target implies (plan M1.8: "CPU-hour
/// thresholds"). No evidence kind records these values yet, so the engine
/// treats every axis as *unmeasured*; the thresholds shape the reported
/// requirement, not a satisfied predicate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardeningPolicy {
    /// Fuzz CPU hours required for L5 (release gate RG-xx semantics).
    #[serde(default)]
    pub fuzz_cpu_hours: u64,
    /// Mutation kill rate required for L5, in whole percent (the
    /// `glyim-pilot` mutation gate's 75% bar).
    #[serde(default)]
    pub mutation_kill_rate_pct: u8,
}

/// `[gap_caps]` table: whether open-gap level caps apply during compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapCapsTable {
    /// `false` disables capping (diagnostic dry runs).
    pub enabled: bool,
}

/// Rules for one level: the `[levels.L1]` sub-table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelRules {
    /// The evidence rules that must all hold for this level.
    pub rules: Vec<EvidenceRule>,
}

/// On-disk shape of `ledger/policy.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    /// Schema tag; must equal [`POLICY_SCHEMA`].
    schema: String,
    /// Per-level rule tables keyed by level spelling (`L1`…`L4`).
    #[serde(default)]
    levels: BTreeMap<String, LevelRules>,
    /// Open-gap caps switch (default: enabled).
    #[serde(default)]
    gap_caps: Option<GapCapsTable>,
    /// Hardening thresholds (default: all zero — unmeasured anyway).
    #[serde(default)]
    hardening: Option<HardeningPolicy>,
}

/// The validated, engine-facing policy.
///
/// Levels are keyed by the typed [`Level`] (only `L1`–`L4` are expressible);
/// unknown or out-of-range keys are load diagnostics, never silently
/// dropped rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelPolicy {
    /// Evidence rules per computable level, ascending.
    pub levels: BTreeMap<Level, Vec<EvidenceRule>>,
    /// Whether open-gap caps apply (`[gap_caps] enabled`).
    pub gap_caps_enabled: bool,
    /// Hardening thresholds (`[hardening]`).
    pub hardening: HardeningPolicy,
}

/// Parses a level spelling (`"L0"`…`"L5"`).
#[must_use]
pub fn parse_level(spelling: &str) -> Option<Level> {
    match spelling {
        "L0" => Some(Level::L0),
        "L1" => Some(Level::L1),
        "L2" => Some(Level::L2),
        "L3" => Some(Level::L3),
        "L4" => Some(Level::L4),
        "L5" => Some(Level::L5),
        _ => None,
    }
}

/// Loads and validates `ROOT/ledger/policy.toml`.
///
/// Returns the policy (`None` when loading or validation failed) plus the
/// sorted diagnostics; any error-severity diagnostic refuses the whole
/// policy so a partial acceptance can never compute a half-typed status.
#[must_use]
pub fn load_policy(root: &Path) -> (Option<LevelPolicy>, Vec<Diagnostic>) {
    let path = root.join("ledger").join("policy.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            return (
                None,
                vec![diagnostic(
                    CODE_POLICY_LOAD,
                    format!("cannot read {}: {err}", path.display()),
                )],
            );
        }
    };
    let file: PolicyFile = match toml::from_str(&text) {
        Ok(file) => file,
        Err(err) => {
            return (
                None,
                vec![diagnostic(
                    CODE_POLICY_SCHEMA,
                    format!("{}: {err}", path.display()),
                )],
            );
        }
    };

    let mut diagnostics = Vec::new();
    if file.schema != POLICY_SCHEMA {
        diagnostics.push(diagnostic(
            CODE_POLICY_SCHEMA,
            format!(
                "{}: schema tag is {:?}, expected {POLICY_SCHEMA:?}",
                path.display(),
                file.schema
            ),
        ));
    }

    let mut levels = BTreeMap::new();
    for (key, table) in &file.levels {
        let Some(level) = parse_level(key) else {
            diagnostics.push(diagnostic(
                CODE_POLICY_RULE,
                format!(
                    "{}: [levels] key {key:?} is not a level spelling (L0–L5)",
                    path.display()
                ),
            ));
            continue;
        };
        if !matches!(level, Level::L1 | Level::L2 | Level::L3 | Level::L4) {
            diagnostics.push(diagnostic(
                CODE_POLICY_RULE,
                format!(
                    "{}: [levels.{key}] is not a rule level (L0 is the baseline; \
                     L5 is governed by [hardening])",
                    path.display()
                ),
            ));
            continue;
        }
        for rule in &table.rules {
            if !CANONICAL_MODES.contains(&rule.mode.as_str()) {
                diagnostics.push(diagnostic(
                    CODE_POLICY_RULE,
                    format!(
                        "{}: [levels.{key}] rule mode {:?} is outside {CANONICAL_MODES:?}",
                        path.display(),
                        rule.mode
                    ),
                ));
            }
            if !CANONICAL_OUTCOMES.contains(&rule.outcome.as_str()) {
                diagnostics.push(diagnostic(
                    CODE_POLICY_RULE,
                    format!(
                        "{}: [levels.{key}] rule outcome {:?} is outside {CANONICAL_OUTCOMES:?}",
                        path.display(),
                        rule.outcome
                    ),
                ));
            }
            if rule.count == 0 {
                diagnostics.push(diagnostic(
                    CODE_POLICY_RULE,
                    format!(
                        "{}: [levels.{key}] rule count must be at least 1",
                        path.display()
                    ),
                ));
            }
        }
        levels.insert(level, table.rules.clone());
    }

    if let Some(hardening) = &file.hardening {
        if hardening.mutation_kill_rate_pct > 100 {
            diagnostics.push(diagnostic(
                CODE_POLICY_RULE,
                format!(
                    "{}: [hardening] mutation_kill_rate_pct must be 0–100, got {}",
                    path.display(),
                    hardening.mutation_kill_rate_pct
                ),
            ));
        }
    }

    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let policy = LevelPolicy {
        levels,
        gap_caps_enabled: file.gap_caps.is_none_or(|caps| caps.enabled),
        hardening: file.hardening.unwrap_or_default(),
    };
    (Some(policy), diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> &'static str {
        // The seed policy (mirrors `ledger/policy.toml`); one literal the
        // schema tests mutate so every diagnostic has a precise trigger.
        r#"schema = "glyim.ledger-policy/1"

# Per-level evidence rules: a rule is a minimum count of distinct fixtures
# whose `glyim.evidence/1` records match the mode/outcome/backend triple.
[levels.L1]
rules = [{ mode = "compile-pass", outcome = "passed", count = 1 }]

[levels.L2]
rules = [
    { mode = "compile-pass", outcome = "passed", count = 1 },
    { mode = "compile-fail", outcome = "passed", count = 1 },
]

[levels.L3]
rules = [{ mode = "run-pass", outcome = "passed", backend = "gam", count = 1 }]

[levels.L4]
rules = [{ mode = "run-pass", outcome = "passed", backend = "llvm", count = 1 }]

[gap_caps]
enabled = true

[hardening]
fuzz_cpu_hours = 24
mutation_kill_rate_pct = 75
"#
    }

    fn write_policy(root: &Path, text: &str) {
        let dir = root.join("ledger");
        fs::create_dir_all(&dir).expect("mkdir ledger");
        fs::write(dir.join("policy.toml"), text).expect("write policy.toml");
    }

    fn loaded(text: &str) -> (Option<LevelPolicy>, Vec<Diagnostic>) {
        let dir = tempfile::tempdir().expect("tmp");
        write_policy(dir.path(), text);
        load_policy(dir.path())
    }

    #[test]
    fn seed_policy_parses_with_all_computable_levels() {
        let (policy, diagnostics) = loaded(seed());
        assert!(
            diagnostics.is_empty(),
            "seed policy must validate cleanly: {diagnostics:?}"
        );
        let policy = policy.expect("policy loaded");
        assert_eq!(policy.levels.len(), 4);
        assert!(policy.levels.contains_key(&Level::L1));
        assert!(policy.levels.contains_key(&Level::L4));
        assert!(policy.gap_caps_enabled);
        assert_eq!(policy.hardening.fuzz_cpu_hours, 24);
        assert_eq!(policy.hardening.mutation_kill_rate_pct, 75);
    }

    #[test]
    fn missing_policy_file_is_a_load_error() {
        let dir = tempfile::tempdir().expect("tmp");
        let (policy, diagnostics) = load_policy(dir.path());
        assert!(policy.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_POLICY_LOAD)
        ));
    }

    #[test]
    fn schema_tag_mismatch_refuses_the_policy() {
        let text = seed().replacen("glyim.ledger-policy/1", "glyim.ledger-policy/2", 1);
        let (policy, diagnostics) = loaded(&text);
        assert!(policy.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_POLICY_SCHEMA)
        ));
    }

    #[test]
    fn unknown_top_level_key_is_a_schema_error() {
        let text = format!("{}\n[extra]\nnope = true\n", seed());
        let (policy, diagnostics) = loaded(&text);
        assert!(policy.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_POLICY_SCHEMA)
        ));
    }

    #[test]
    fn unknown_level_key_is_a_rule_error() {
        let text = format!("{}\n\n[levels.L9]\nrules = []\n", seed());
        let (policy, diagnostics) = loaded(&text);
        assert!(policy.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_POLICY_RULE)
        ));
    }

    #[test]
    fn l5_is_not_expressible_as_evidence_rules() {
        let text = format!(
            "{}\n\n[levels.L5]\nrules = [{{ mode = \"run-pass\", outcome = \"passed\", count = 1 }}]\n",
            seed()
        );
        let (policy, diagnostics) = loaded(&text);
        assert!(policy.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_POLICY_RULE)
        ));
    }

    #[test]
    fn rule_mode_outcome_and_count_are_validated() {
        // Minimal policies: appending a second [levels.L1] to the seed
        // would collide in TOML, so each bad rule gets its own file.
        let schema_line = "schema = \"glyim.ledger-policy/1\"\n";
        for bad in [
            r#"[levels.L1]
rules = [{ mode = "parse-pass", outcome = "passed", count = 1 }]"#,
            r#"[levels.L1]
rules = [{ mode = "compile-pass", outcome = "green", count = 1 }]"#,
            r#"[levels.L1]
rules = [{ mode = "compile-pass", outcome = "passed", count = 0 }]"#,
        ] {
            let text = format!("{schema_line}\n{bad}\n");
            let (policy, diagnostics) = loaded(&text);
            assert!(policy.is_none(), "policy must refuse: {bad}");
            assert!(matches!(
                diagnostics.first().map(|d| d.code),
                Some(CODE_POLICY_RULE)
            ));
        }
    }

    #[test]
    fn mutation_rate_above_100_is_rejected() {
        let text = seed().replacen(
            "mutation_kill_rate_pct = 75",
            "mutation_kill_rate_pct = 101",
            1,
        );
        let (policy, diagnostics) = loaded(&text);
        assert!(policy.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_POLICY_RULE)
        ));
    }

    #[test]
    fn defaults_apply_when_tables_are_absent() {
        let text = "schema = \"glyim.ledger-policy/1\"\n";
        let (policy, diagnostics) = loaded(text);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let policy = policy.expect("policy loaded");
        assert!(policy.levels.is_empty());
        // Caps default to enabled: the diagnostic dry-run switch is opt-in.
        assert!(policy.gap_caps_enabled);
        assert_eq!(policy.hardening, HardeningPolicy::default());
    }
}
