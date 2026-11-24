//! `policy-mutation` — the M1.13 policy-mutation slice (plan §6 M1.13:
//! "mutation-test `policy` evaluation").
//!
//! The level engine ([`compute_levels`]) is a pure function of (registry,
//! evidence, policy); the committed `STATUS.json`/`STATUS.md` goldens are
//! its test oracle (the M1.10 "STATUS is up to date" gate fails on any
//! drift). A policy rule that the engine silently ignores would make
//! policy edits unobservable — the golden would keep passing while the
//! computation drifted from the documented semantics. This harness closes
//! that hole the same way a mutation gate closes test holes: it generates
//! a fixed, deterministic catalogue of *policy mutants* (rules dropped,
//! counts inflated, outcomes flipped), evaluates the engine under each
//! mutant against the same registry and evidence the production compute
//! run sees, and requires every mutant to be **killed** — to change at
//! least one feature's computed outcome. A surviving mutant names a
//! policy surface the evaluation does not actually consume, and the
//! command fails (the same gate-not-report contract `verify` pins).
//!
//! # Kill criterion
//!
//! A mutant is killed iff the mutant's per-feature
//! [`FeatureLevel`] vector differs from the baseline vector anywhere
//! (`computed_level`, `missing` reasons, `capped_by`, or `unmeasured`).
//! That is exactly the difference the committed STATUS goldens would catch
//! through the M1.10 drift gate — the harness proves the detection works
//! per rule, instead of trusting it.
//!
//! # Catalogue scope (honest non-observables excluded)
//!
//! The catalogue mutates the surfaces the engine demonstrably consumes:
//! the per-level evidence rules (presence, `count`, `outcome`) for L1–L4.
//! Two policy surfaces are deliberately **not** in the shipped catalogue:
//!
//! - *Backend pins*: flipping an L3/L4 rule's `backend` is unobservable on
//!   rule shapes where the lower level's evidence doubles for the flipped
//!   rule (a gam fixture satisfies a gam-pinned L4 rule too), so a flip
//!   mutant survives *by the policy's own semantics*, not by an engine
//!   bug. The backend-gating behaviour is pinned by the synthetic
//!   `gated_away_level_stops_the_ladder` test in `compute.rs` instead.
//! - *Hardening thresholds*: `fuzz_cpu_hours`/`mutation_kill_rate_pct`
//!   shape the *reported requirement* only (plan M1.8); no computed
//!   predicate reads their values today, so a value mutant is unobservable
//!   by construction. The `unmeasured` reporting they shape is pinned by
//!   the compute golden.
//!
//! Excluding unobservable mutants keeps the gate strict: every shipped
//! mutant must kill, so a green `policy-mutation` run means every rule
//! surface is load-bearing, not that the catalogue was tuned to pass.

use std::fmt::Write as _;
use std::path::Path;

use ledger_schema::policy::LevelPolicy;
use ledger_schema::{Diagnostic, Level, Severity};
use serde::Serialize;

use crate::compute::{
    EvidenceSource, FeatureLevel, LedgerEvidence, ProjectEvidenceSource, compute_levels, diagnostic,
};
use ledger_schema::load_project_registry;

/// Schema tag of the mutation report's JSON contract.
pub const MUTATION_SCHEMA: &str = "glyim.ledger-mutation/1";

/// One deterministic policy mutant: a named, described mutation of the
/// baseline policy.
#[derive(Debug, Clone)]
pub struct PolicyMutant {
    /// Stable mutant name (part of the JSON contract; never renamed).
    pub name: &'static str,
    /// What the mutation does to the baseline policy, one line.
    pub description: String,
    /// The mutated policy the engine evaluates.
    pub policy: LevelPolicy,
}

/// The verdict for one mutant over one (registry, evidence) input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutantVerdict {
    /// The mutant's stable name.
    pub name: String,
    /// The one-line mutation description.
    pub description: String,
    /// True when the mutant changed at least one feature's outcome.
    pub killed: bool,
    /// How many features' levels differ from baseline.
    pub changed_features: usize,
    /// First differing feature as `ID: baseline -> mutant` (sorted by ID),
    /// for review logs; `None` for a killed-by-reasons-only mutant.
    pub sample: Option<String>,
}

/// Result of a full `policy-mutation` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationOutcome {
    /// Load diagnostics from the registry, evidence, and policy passes
    /// (sorted and de-duplicated; errors fail the command like `compute`).
    pub diagnostics: Vec<Diagnostic>,
    /// Features the baseline computation covered.
    pub features: usize,
    /// Verdicts, in catalogue order (the catalogue order is part of the
    /// JSON contract).
    pub verdicts: Vec<MutantVerdict>,
}

impl MutationOutcome {
    /// Number of killed mutants.
    #[must_use]
    pub fn killed(&self) -> usize {
        self.verdicts.iter().filter(|v| v.killed).count()
    }

    /// Number of surviving mutants (the gate-failing count).
    #[must_use]
    pub fn survived(&self) -> usize {
        self.verdicts.iter().filter(|v| !v.killed).count()
    }

    /// True when no error-severity diagnostic was produced and every
    /// mutant was killed. This is the M1.13 policy-mutation gate
    /// condition.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error)
            && self.survived() == 0
    }
}

// -- catalogue ---------------------------------------------------------------

/// Builds the deterministic mutant catalogue over `baseline`.
///
/// The catalogue is a fixed list of named mutations (the order is part of
/// the JSON contract); every mutant deep-clones the baseline policy and
/// applies exactly one mutation, so each verdict attributes the kill to
/// one rule surface.
#[must_use]
pub fn policy_mutants(baseline: &LevelPolicy) -> Vec<PolicyMutant> {
    let mut mutants = Vec::new();
    let mut push = |name: &'static str, description: String, mutate: &dyn Fn(&mut LevelPolicy)| {
        let mut policy = baseline.clone();
        mutate(&mut policy);
        mutants.push(PolicyMutant {
            name,
            description,
            policy,
        });
    };

    // L1 (parses): presence, count, and outcome mutations.
    push(
        "drop-l1-rules",
        "the L1 rule list is emptied — no parse evidence can ever count".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L1) {
                rules.clear();
            }
        },
    );
    push(
        "inflate-l1-count",
        "the L1 compile-pass count is inflated 1 -> 999 — no store can satisfy it".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L1) {
                for rule in rules.iter_mut() {
                    if rule.mode == "compile-pass" {
                        rule.count = 999;
                    }
                }
            }
        },
    );
    push(
        "flip-l1-outcome",
        "the L1 compile-pass outcome is flipped passed -> failed".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L1) {
                for rule in rules.iter_mut() {
                    if rule.mode == "compile-pass" {
                        rule.outcome = "failed".to_string();
                    }
                }
            }
        },
    );

    // L2 (checks): the negative-fixture rule's presence and outcome —
    // the plan's L2 definition is "positive **and** negative" evidence,
    // so each half gets its own mutant.
    push(
        "drop-l2-negative-rule",
        "the L2 compile-fail rule is dropped — positive evidence alone would reach L2".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L2) {
                rules.retain(|rule| rule.mode != "compile-fail");
            }
        },
    );
    push(
        "flip-l2-outcome",
        "the L2 compile-fail outcome is flipped passed -> failed".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L2) {
                for rule in rules.iter_mut() {
                    if rule.mode == "compile-fail" {
                        rule.outcome = "failed".to_string();
                    }
                }
            }
        },
    );

    // L3 (interpreted): presence and outcome of the GAM run-pass rule.
    push(
        "drop-l3-rules",
        "the L3 rule list is emptied — interpreted evidence can never count".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L3) {
                rules.clear();
            }
        },
    );
    push(
        "flip-l3-outcome",
        "the L3 run-pass outcome is flipped passed -> failed".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L3) {
                for rule in rules.iter_mut() {
                    rule.outcome = "failed".to_string();
                }
            }
        },
    );

    // L4 (native-verified): presence and outcome of the LLVM run-pass rule.
    push(
        "drop-l4-rules",
        "the L4 rule list is emptied — native evidence can never count".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L4) {
                rules.clear();
            }
        },
    );
    push(
        "flip-l4-outcome",
        "the L4 run-pass outcome is flipped passed -> failed".to_string(),
        &|p| {
            if let Some(rules) = p.levels.get_mut(&Level::L4) {
                for rule in rules.iter_mut() {
                    rule.outcome = "failed".to_string();
                }
            }
        },
    );

    mutants
}

// -- runner ------------------------------------------------------------------

/// Runs the policy-mutation gate over the project at `root` (the
/// directory that contains `ledger/`): one baseline compute pass, then
/// one pass per catalogue mutant over the same registry and evidence.
///
/// # Errors
///
/// None — every failure mode (unreadable registry, invalid evidence,
/// missing policy) becomes a sorted error-severity diagnostic and the
/// `is_ok()` gate condition fails, matching the `compute`/`verify`
/// contract.
pub fn run_policy_mutation(root: &Path, evidence: Option<&Path>) -> MutationOutcome {
    let span = tracing::info_span!("policy_mutation", root = %root.display());
    let _enter = span.enter();

    let loaded = load_project_registry(root);
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());

    let batch = ProjectEvidenceSource::new(root, evidence).load();
    diagnostics.extend(batch.diagnostics);

    let (policy, policy_diagnostics) = ledger_schema::load_policy(root);
    diagnostics.extend(policy_diagnostics);

    let mut outcome = MutationOutcome {
        diagnostics: Vec::new(),
        features: 0,
        verdicts: Vec::new(),
    };
    let Some(baseline_policy) = policy else {
        diagnostics.push(diagnostic(
            ledger_schema::CODE_POLICY_LOAD,
            Severity::Error,
            "policy-mutation needs ledger/policy.toml; the baseline policy failed to load"
                .to_string(),
        ));
        outcome.diagnostics = sort_diagnostics(diagnostics);
        return outcome;
    };

    // The engine dedupes and counts set-wise itself; the raw loaded
    // records are the same input the production compute pass feeds it.
    let evidence: &[LedgerEvidence] = &batch.records;
    let baseline = compute_levels(&loaded.registry, &baseline_policy, evidence);
    outcome.features = baseline.len();

    for mutant in policy_mutants(&baseline_policy) {
        let mutant_levels = compute_levels(&loaded.registry, &mutant.policy, evidence);
        outcome
            .verdicts
            .push(verdict_for(&baseline, &mutant_levels, &mutant));
    }

    outcome.diagnostics = sort_diagnostics(diagnostics);
    outcome
}

/// Compares a mutant's level vector against the baseline and builds the
/// verdict (the kill criterion is any per-feature difference).
fn verdict_for(
    baseline: &[FeatureLevel],
    mutant_levels: &[FeatureLevel],
    mutant: &PolicyMutant,
) -> MutantVerdict {
    let mut changed = 0usize;
    let mut sample = None;
    for (base, mutated) in baseline.iter().zip(mutant_levels.iter()) {
        if base != mutated {
            changed += 1;
            if sample.is_none() {
                let level_move = if base.computed_level == mutated.computed_level {
                    // Killed by the missing/caps/unmeasured text rather
                    // than the level number itself.
                    format!(
                        "{} (same level, reasons/caps differ)",
                        base.computed_level.as_str()
                    )
                } else {
                    format!(
                        "{} -> {}",
                        base.computed_level.as_str(),
                        mutated.computed_level.as_str()
                    )
                };
                sample = Some(format!("{}: {level_move}", base.id));
            }
        }
    }
    MutantVerdict {
        name: mutant.name.to_string(),
        description: mutant.description.clone(),
        killed: changed > 0,
        changed_features: changed,
        sample,
    }
}

/// Sorts and de-duplicates diagnostics the same way every other ledger
/// face does (code, then message).
fn sort_diagnostics(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    diagnostics
}

// -- renderers ---------------------------------------------------------------

/// JSON document rendered by [`format_mutation_json`].
#[derive(Serialize)]
struct MutationJson<'a> {
    schema: &'static str,
    features: usize,
    killed: usize,
    survived: usize,
    ok: bool,
    verdicts: &'a [MutantVerdict],
}

/// Renders the `glyim.ledger-mutation/1` JSON contract (pretty-printed).
#[must_use]
pub fn format_mutation_json(outcome: &MutationOutcome) -> String {
    let document = MutationJson {
        schema: MUTATION_SCHEMA,
        features: outcome.features,
        killed: outcome.killed(),
        survived: outcome.survived(),
        ok: outcome.is_ok(),
        verdicts: &outcome.verdicts,
    };
    let mut text = serde_json::to_string_pretty(&document)
        .unwrap_or_else(|err| format!("{{\"error\":\"{err}\"}}"));
    text.push('\n');
    text
}

/// Renders the human-readable mutation report (one row per mutant, in
/// catalogue order, survivors first-class and loud).
#[must_use]
pub fn format_mutation_text(outcome: &MutationOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Policy mutation report (plan M1.13): {} features, {} mutants, {} killed, {} survived",
        outcome.features,
        outcome.verdicts.len(),
        outcome.killed(),
        outcome.survived()
    );
    for verdict in &outcome.verdicts {
        let _ = writeln!(
            text,
            "  [{}] {} — {}",
            if verdict.killed { "KILLED" } else { "SURVIVED" },
            verdict.name,
            verdict.description
        );
        if let Some(sample) = &verdict.sample {
            let _ = writeln!(text, "      sample: {sample}");
        }
    }
    let _ = writeln!(
        text,
        "Gate: {}",
        if outcome.is_ok() {
            "every shipped policy mutant is killed — the level evaluation is load-bearing"
        } else {
            "surviving mutants found — policy surfaces the evaluation ignores (fix the engine or the catalogue)"
        }
    );
    text
}

#[cfg(test)]
mod tests {
    use super::{MUTATION_SCHEMA, format_mutation_json, policy_mutants, run_policy_mutation};
    use crate::compute::run_compute;
    use std::path::PathBuf;

    /// The repo root (ledger crate → `<repo>/tools/glyim-ledger`), so the
    /// gate tests run over the same committed registry/evidence/policy the
    /// production compute pass sees.
    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("ledger crate lives under <repo>/tools/")
            .to_path_buf()
    }

    #[test]
    fn catalogue_is_fixed_ordered_and_single_mutation() {
        let baseline = ledger_schema::load_policy(&repo_root())
            .0
            .expect("repo policy loads");
        let mutants = policy_mutants(&baseline);
        let names: Vec<_> = mutants.iter().map(|m| m.name).collect();
        assert_eq!(
            names,
            [
                "drop-l1-rules",
                "inflate-l1-count",
                "flip-l1-outcome",
                "drop-l2-negative-rule",
                "flip-l2-outcome",
                "drop-l3-rules",
                "flip-l3-outcome",
                "drop-l4-rules",
                "flip-l4-outcome",
            ],
            "the catalogue order is part of the JSON contract"
        );
        // Exactly one mutation per mutant: each differs from the baseline,
        // and no two mutants are identical.
        for mutant in &mutants {
            assert_ne!(mutant.policy, baseline, "{} must mutate", mutant.name);
        }
        for (i, a) in mutants.iter().enumerate() {
            for b in mutants.iter().skip(i + 1) {
                assert_ne!(a.policy, b.policy, "{} vs {} must differ", a.name, b.name);
            }
        }
    }

    /// The plan M1.13 acceptance criterion, as a gate: every shipped
    /// policy mutant is killed by the committed store — the level
    /// evaluation consumes every rule surface the catalogue mutates.
    #[test]
    fn every_catalogue_mutant_is_killed_over_the_real_store() {
        let outcome = run_policy_mutation(&repo_root(), None);
        assert!(
            outcome
                .diagnostics
                .iter()
                .all(|d| d.severity != ledger_schema::Severity::Error),
            "the repo registry/evidence/policy must load cleanly: {:?}",
            outcome.diagnostics
        );
        assert!(outcome.features > 0, "the repo registry has features");
        let survivors: Vec<_> = outcome.verdicts.iter().filter(|v| !v.killed).collect();
        assert!(
            survivors.is_empty(),
            "surviving policy mutants (surfaces the evaluation ignores): {survivors:?}"
        );
        // Every kill is visible in the computed level of at least one
        // feature — the same drift the committed STATUS golden catches.
        for verdict in &outcome.verdicts {
            assert!(
                verdict.changed_features > 0,
                "{} killed without a change",
                verdict.name
            );
        }
    }

    #[test]
    fn mutation_report_is_deterministic_and_total() {
        let outcome = run_policy_mutation(&repo_root(), None);
        let json = format_mutation_json(&outcome);
        assert!(json.contains(&format!("\"schema\": \"{MUTATION_SCHEMA}\"")));
        assert_eq!(
            json,
            format_mutation_json(&run_policy_mutation(&repo_root(), None)),
            "the report is a pure function of the committed store"
        );
        assert!(
            json.contains("\"killed\": 9"),
            "all nine catalogue mutants killed"
        );
    }

    /// The baseline the mutants are measured against is the production
    /// compute outcome: same registry, same evidence, same policy.
    #[test]
    fn baseline_matches_the_production_compute_pass() {
        let compute = run_compute(&repo_root(), None);
        let mutation = run_policy_mutation(&repo_root(), None);
        assert_eq!(compute.features, mutation.features);
        assert_eq!(compute.entries.len(), mutation.features);
    }
}
