//! `ledger/gates.toml` — the M1.12 release-gate registry schema.
//!
//! The master plan (§6 M1.12) declares 24 release gates (`RG-01`…`RG-24`)
//! in `ledger/gates.toml`; `ledger gates` prints all 24 with their current
//! verdict, and `unmeasured` is allowed but displayed (never silently
//! dropped, never treated as a pass). This module owns the declaration
//! face of that seam: a strict (`deny_unknown_fields`) TOML schema
//! mirroring [`crate::policy`]'s role for `ledger compute`.
//!
//! A gate declaration has three fields:
//!
//! - `id` — the plan's stable `RG-NN` spelling;
//! - `title` — the plan's gate name (one line);
//! - `criterion` — the measurable pass condition, copied from the plan's
//!   gate table so the registry can be re-checked against it;
//! - `evidence` — which evidence source the verdict engine may consult
//!   today: `none` (no source records this gate yet → the honest
//!   `unmeasured` verdict), `levels` (the M1.8 compute outcome), or
//!   `gaps` (the M1.7 gap registry). The enum is deliberately closed:
//!   wiring a new source is a code change that lands with the source, so
//!   a declaration can never claim an input the engine does not have.
//!
//! Constitution C1 (status is computed, never typed) applies to the gate
//! *registry* too: the file declares what is measured and from where, but
//! never a verdict — verdicts are the engine's output
//! (`tools/glyim-ledger/src/gates.rs`), recomputed from evidence on every
//! run.
//!
//! Loading is deterministic and diagnostic-bearing like every other loader
//! in this crate: failures are sorted [`Diagnostic`]s, never panics, and
//! any error-severity diagnostic refuses the whole registry (a half-loaded
//! gate table would print a half-honest release report).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Diagnostic, Severity};

/// Schema tag pinned for the gates file shape.
pub const GATES_SCHEMA: &str = "glyim.ledger-gates-registry/1";

// -- diagnostics -----------------------------------------------------------

/// `ledger/gates.toml` is missing or unreadable.
pub const CODE_GATES_LOAD: &str = "LEDGER-GATES-LOAD";
/// `ledger/gates.toml` does not match the gates schema.
pub const CODE_GATES_SCHEMA: &str = "LEDGER-GATES-SCHEMA";
/// A `[[gate]]` entry is malformed (id/title/criterion/evidence).
pub const CODE_GATES_ENTRY: &str = "LEDGER-GATES-ENTRY";
/// A `[[gate]]` ID is declared twice.
pub const CODE_GATES_DUP: &str = "LEDGER-GATES-DUP";

fn diagnostic(code: &'static str, message: String) -> Diagnostic {
    Diagnostic {
        code,
        message,
        severity: Severity::Error,
    }
}

// -- schema types -----------------------------------------------------------

/// The evidence source a gate's verdict engine may consult today.
///
/// The set is closed on purpose (see the module docs): `none` is the
/// honest "no source records this gate yet" spelling that renders
/// `unmeasured`, and every other variant names a source the ledger
/// already loads for another subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateEvidence {
    /// No evidence source records this gate yet; the verdict is
    /// `unmeasured` (displayed, never silently passed or failed).
    None,
    /// The M1.8 compute outcome (per-feature readiness levels) — used by
    /// the feature-closure gate (plan RG-09).
    Levels,
    /// The M1.7 gap registry (open gaps, classes, witness debt) — used by
    /// the gap-closure gate (plan RG-10).
    Gaps,
}

impl GateEvidence {
    /// Canonical registry spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Levels => "levels",
            Self::Gaps => "gaps",
        }
    }
}

/// One `[[gate]]` registry entry: a plan release gate's stable identity
/// and the evidence source its verdict consumes. No verdict field exists
/// (plan C1); the engine computes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateEntry {
    /// Stable gate ID, the plan's `RG-NN` spelling (validated).
    pub id: String,
    /// The plan's gate name (one line, rendered as-is).
    pub title: String,
    /// The measurable pass condition, copied from the plan's gate table.
    pub criterion: String,
    /// The evidence source the verdict engine may consult today.
    pub evidence: GateEvidence,
}

/// On-disk shape of `ledger/gates.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GatesFile {
    /// Schema tag; must equal [`GATES_SCHEMA`].
    schema: String,
    /// The declared release gates.
    #[serde(default)]
    gate: Vec<GateEntry>,
}

/// The validated, engine-facing gate registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRegistry {
    /// The declared gates, in file order (the plan's RG-01…RG-24 order);
    /// duplicate-ID and malformed-ID diagnostics refuse the whole
    /// registry, so the vector is presentation order only.
    pub gates: Vec<GateEntry>,
}

/// Validates one gate ID against the plan's `RG-NN` spelling.
fn gate_id_is_canonical(id: &str) -> bool {
    let body = id.strip_prefix("RG-");
    match body {
        Some(digits) => {
            !digits.is_empty() && digits.len() <= 2 && digits.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

/// Loads and validates `ROOT/ledger/gates.toml`.
///
/// Returns the registry (`None` when loading or validation failed) plus
/// the sorted diagnostics; any error-severity diagnostic refuses the
/// whole registry so a partial acceptance can never print a half-honest
/// release report.
#[must_use]
pub fn load_gates(root: &Path) -> (Option<GateRegistry>, Vec<Diagnostic>) {
    let path = root.join("ledger").join("gates.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            return (
                None,
                vec![diagnostic(
                    CODE_GATES_LOAD,
                    format!("cannot read {}: {err}", path.display()),
                )],
            );
        }
    };
    let file: GatesFile = match toml::from_str(&text) {
        Ok(file) => file,
        Err(err) => {
            return (
                None,
                vec![diagnostic(
                    CODE_GATES_SCHEMA,
                    format!("{}: {err}", path.display()),
                )],
            );
        }
    };

    let mut diagnostics = Vec::new();
    if file.schema != GATES_SCHEMA {
        diagnostics.push(diagnostic(
            CODE_GATES_SCHEMA,
            format!(
                "{}: schema tag is {:?}, expected {GATES_SCHEMA:?}",
                path.display(),
                file.schema
            ),
        ));
    }
    if file.gate.is_empty() {
        diagnostics.push(diagnostic(
            CODE_GATES_ENTRY,
            format!("{}: declares no [[gate]] entries", path.display()),
        ));
    }

    let mut seen: Vec<&str> = Vec::new();
    for entry in &file.gate {
        if !gate_id_is_canonical(&entry.id) {
            diagnostics.push(diagnostic(
                CODE_GATES_ENTRY,
                format!(
                    "{}: gate id {:?} is not the plan's RG-NN spelling",
                    path.display(),
                    entry.id
                ),
            ));
        }
        if entry.title.trim().is_empty() {
            diagnostics.push(diagnostic(
                CODE_GATES_ENTRY,
                format!(
                    "{}: gate {} declares an empty title",
                    path.display(),
                    entry.id
                ),
            ));
        }
        if entry.criterion.trim().is_empty() {
            diagnostics.push(diagnostic(
                CODE_GATES_ENTRY,
                format!(
                    "{}: gate {} declares an empty criterion",
                    path.display(),
                    entry.id
                ),
            ));
        }
        if seen.contains(&entry.id.as_str()) {
            diagnostics.push(diagnostic(
                CODE_GATES_DUP,
                format!("{}: gate id {} is declared twice", path.display(), entry.id),
            ));
        }
        seen.push(&entry.id);
    }

    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (None, diagnostics);
    }

    let registry = GateRegistry { gates: file.gate };
    (Some(registry), diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> &'static str {
        // Two representative entries (one per wired evidence source plus
        // the honest `none`); the full 24-gate declaration lives in
        // `ledger/gates.toml` and is exercised by the CLI tests.
        r#"schema = "glyim.ledger-gates-registry/1"

[[gate]]
id = "RG-09"
title = "Feature closure"
criterion = "All core and std features >= L4; core >= L5"
evidence = "levels"

[[gate]]
id = "RG-10"
title = "Gap closure"
criterion = "0 open class-b gaps; every class-a gap has a pinned diagnostic; witness debt = 0"
evidence = "gaps"

[[gate]]
id = "RG-01"
title = "No ICE"
criterion = "0 ICEs over the full fuzz corpus; >= 1000 cumulative CPU-hours, last 100 clean"
evidence = "none"
"#
    }

    fn write_gates(root: &Path, text: &str) {
        let dir = root.join("ledger");
        fs::create_dir_all(&dir).expect("mkdir ledger");
        fs::write(dir.join("gates.toml"), text).expect("write gates.toml");
    }

    fn loaded(text: &str) -> (Option<GateRegistry>, Vec<Diagnostic>) {
        let dir = tempfile::tempdir().expect("tmp");
        write_gates(dir.path(), text);
        load_gates(dir.path())
    }

    #[test]
    fn seed_registry_parses_with_all_evidence_spellings() {
        let (registry, diagnostics) = loaded(seed());
        assert!(
            diagnostics.is_empty(),
            "seed registry must validate cleanly: {diagnostics:?}"
        );
        let registry = registry.expect("registry loaded");
        assert_eq!(registry.gates.len(), 3);
        assert_eq!(registry.gates[0].id, "RG-09");
        assert_eq!(registry.gates[0].evidence, GateEvidence::Levels);
        assert_eq!(registry.gates[1].evidence, GateEvidence::Gaps);
        assert_eq!(registry.gates[2].evidence, GateEvidence::None);
        assert_eq!(GateEvidence::None.as_str(), "none");
        assert_eq!(GateEvidence::Levels.as_str(), "levels");
        assert_eq!(GateEvidence::Gaps.as_str(), "gaps");
    }

    #[test]
    fn registry_preserves_file_order() {
        let (registry, _) = loaded(seed());
        let registry = registry.expect("registry loaded");
        let ids: Vec<&str> = registry.gates.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["RG-09", "RG-10", "RG-01"]);
    }

    #[test]
    fn missing_gates_file_is_a_load_error() {
        let dir = tempfile::tempdir().expect("tmp");
        let (registry, diagnostics) = load_gates(dir.path());
        assert!(registry.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_GATES_LOAD)
        ));
    }

    #[test]
    fn schema_tag_mismatch_refuses_the_registry() {
        let text = seed().replacen(
            "glyim.ledger-gates-registry/1",
            "glyim.ledger-gates-registry/2",
            1,
        );
        let (registry, diagnostics) = loaded(&text);
        assert!(registry.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_GATES_SCHEMA)
        ));
    }

    #[test]
    fn unknown_top_level_key_is_a_schema_error() {
        let text = format!("{}\n[extra]\nnope = true\n", seed());
        let (registry, diagnostics) = loaded(&text);
        assert!(registry.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_GATES_SCHEMA)
        ));
    }

    #[test]
    fn unknown_gate_field_is_a_schema_error() {
        let text = format!(
            "{}\n[[gate]]\nid = \"RG-25\"\ntitle = \"t\"\ncriterion = \"c\"\nevidence = \"none\"\nwaiver = true\n",
            seed()
        );
        let (registry, diagnostics) = loaded(&text);
        assert!(registry.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_GATES_SCHEMA)
        ));
    }

    #[test]
    fn empty_gate_table_is_an_entry_error() {
        let text = "schema = \"glyim.ledger-gates-registry/1\"\n";
        let (registry, diagnostics) = loaded(text);
        assert!(registry.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_GATES_ENTRY)
        ));
    }

    #[test]
    fn non_canonical_gate_id_is_an_entry_error() {
        for id in ["rg-01", "RG1", "RG-", "RG-100", "GAP-0001", "RG-1a"] {
            let text = seed().replacen("RG-09", id, 1);
            let (registry, diagnostics) = loaded(&text);
            assert!(registry.is_none(), "gate id {id:?} must be refused");
            assert!(
                diagnostics.iter().any(|d| d.code == CODE_GATES_ENTRY),
                "gate id {id:?} must name LEDGER-GATES-ENTRY"
            );
        }
    }

    #[test]
    fn duplicate_gate_id_is_a_dup_error() {
        let text = format!(
            "{}\n[[gate]]\nid = \"RG-09\"\ntitle = \"dup\"\ncriterion = \"c\"\nevidence = \"none\"\n",
            seed()
        );
        let (registry, diagnostics) = loaded(&text);
        assert!(registry.is_none());
        assert!(diagnostics.iter().any(|d| d.code == CODE_GATES_DUP));
    }

    #[test]
    fn empty_title_or_criterion_is_an_entry_error() {
        let blank_title = seed().replacen("title = \"Feature closure\"", "title = \"   \"", 1);
        let (registry, diagnostics) = loaded(&blank_title);
        assert!(registry.is_none());
        assert!(diagnostics.iter().any(|d| d.code == CODE_GATES_ENTRY));

        let blank_criterion = seed().replacen(
            "criterion = \"All core and std features >= L4; core >= L5\"",
            "criterion = \"\"",
            1,
        );
        let (registry, diagnostics) = loaded(&blank_criterion);
        assert!(registry.is_none());
        assert!(diagnostics.iter().any(|d| d.code == CODE_GATES_ENTRY));
    }

    #[test]
    fn unknown_evidence_spelling_is_a_schema_error() {
        let text = seed().replacen("evidence = \"levels\"", "evidence = \"fuzz\"", 1);
        let (registry, diagnostics) = loaded(&text);
        assert!(registry.is_none());
        assert!(matches!(
            diagnostics.first().map(|d| d.code),
            Some(CODE_GATES_SCHEMA)
        ));
    }
}
