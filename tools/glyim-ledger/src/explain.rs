//! `ledger explain` — the plan §3.7 per-entry explanation.
//!
//! Resolves one registry ID (gap or feature) and renders why it is
//! queued and what evidence or change moves it: the same `why` and
//! `evidence_needed` lines the §3.9 [`crate::next`] queue emits, plus the
//! entry's own metadata (caps/witness for gaps, area/tier/target for
//! features). The engine reuses the [`crate::next`] `WorkItemSource`
//! seam rather than re-deriving priorities, so the two surfaces can
//! never drift apart — pinned by the `explain_why_matches_the_next_queue`
//! test.

use std::path::Path;

use ledger_schema::{Diagnostic, Lane, Level, Tier};

use crate::next::{FeatureSource, GapSource, WorkKind, build_queue};

// ---------------------------------------------------------------------------
// resolution contract
// ---------------------------------------------------------------------------

/// One resolved registry entry: the queue item (why/evidence/files/lane)
/// merged with the entry's own registry metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainEntry {
    /// Registry ID (`GAP-NNNN` or a feature ID).
    pub id: String,
    /// Human-readable entry title.
    pub title: String,
    /// Gap or feature provenance (same shape the `next` queue emits).
    pub kind: WorkKind,
    /// Queue reason — byte-identical to the `next` queue's item.
    pub why: String,
    /// What evidence or change moves the item out of the queue.
    pub evidence_needed: String,
    /// Registry file (and witness fixture) the entry lives in.
    pub suggested_files: Vec<String>,
    /// Suggested owner lane (gap -> first affected feature's lane;
    /// feature -> `owner_lane`).
    pub lane: Option<Lane>,
    /// Gap -> affected feature IDs; feature -> direct dependant IDs.
    pub related: Vec<String>,
    /// Gap only: the level ceiling the gap imposes on affected features.
    pub caps: Option<Level>,
    /// Gap only: the witness fixture path, when one exists.
    pub witness: Option<String>,
    /// Feature only: registry area.
    pub area: Option<String>,
    /// Feature only: maturity tier.
    pub tier: Option<Tier>,
    /// Feature only: the level the feature must eventually reach.
    pub target_level: Option<Level>,
}

/// Result of an `explain` run: the resolved entry (if the ID exists)
/// plus any load/validation diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainOutcome {
    /// Load + validation diagnostics, sorted and de-duplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// The resolved entry; `None` when the ID matches no registry entry.
    pub entry: Option<ExplainEntry>,
}

impl ExplainOutcome {
    /// Number of error-severity diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == ledger_schema::Severity::Error)
            .count()
    }

    /// True when no error-severity diagnostic was produced. An unknown
    /// ID is a report ("no registry entry"), not a load failure.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }
}

/// Run the §3.7 explanation for `id` over the registry under `root`.
/// The queue item is produced through the same [`GapSource`]/[`FeatureSource`]
/// adapters the `next` queue uses, so the reason and evidence lines match
/// word for word.
#[must_use]
pub fn run_explain(root: &Path, id: &str) -> ExplainOutcome {
    let span = tracing::info_span!("explain", root = %root.display(), id = %id);
    let _enter = span.enter();

    let loaded = ledger_schema::load_project_registry(root);
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());

    let features = loaded.registry.features().to_vec();
    let lane_of = |fid: &str| {
        features
            .iter()
            .find(|f| f.id() == fid)
            .map(|f| f.entry.owner_lane)
    };
    let queue = build_queue(&[
        &GapSource::new(loaded.registry.gaps(), &lane_of),
        &FeatureSource::new(&features),
    ]);

    let item = queue.iter().find(|queued| queued.id == id);
    let entry = item.and_then(|queued| {
        let gap = loaded
            .registry
            .gaps()
            .iter()
            .find(|g| g.id() == id)
            .map(|g| &g.entry);
        let feature = features.iter().find(|f| f.id() == id).map(|f| &f.entry);
        match (gap, feature) {
            (Some(gap), _) => Some(ExplainEntry {
                id: queued.id.clone(),
                title: queued.title.clone(),
                kind: queued.kind,
                why: queued.why.clone(),
                evidence_needed: queued.evidence_needed.clone(),
                suggested_files: queued.suggested_files.clone(),
                lane: queued.lane,
                related: queued.related.clone(),
                caps: Some(gap.caps),
                witness: gap.witness.clone(),
                area: None,
                tier: None,
                target_level: None,
            }),
            (None, Some(feature)) => Some(ExplainEntry {
                id: queued.id.clone(),
                title: queued.title.clone(),
                kind: queued.kind,
                why: queued.why.clone(),
                evidence_needed: queued.evidence_needed.clone(),
                suggested_files: queued.suggested_files.clone(),
                lane: queued.lane,
                related: queued.related.clone(),
                caps: None,
                witness: None,
                area: Some(feature.area.clone()),
                tier: Some(feature.tier),
                target_level: Some(feature.target_level),
            }),
            (None, None) => None,
        }
    });

    tracing::debug!(found = entry.is_some(), "explain resolved");

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    ExplainOutcome { diagnostics, entry }
}

/// Render the stdout report for an explain run: a header line, the
/// entry's metadata block (kind-specific), the queue reason and evidence
/// lines, then any diagnostics. Unknown IDs render a "no registry entry"
/// line so a typo is a report, not a crash.
#[must_use]
pub fn format_explain_report(outcome: &ExplainOutcome) -> String {
    let mut lines = Vec::new();
    match &outcome.entry {
        Some(entry) => {
            let badge = match entry.kind {
                WorkKind::Gap {
                    class,
                    witnessed: false,
                } => format!("gap {}, witness debt", class.as_str()),
                WorkKind::Gap {
                    class,
                    witnessed: true,
                } => format!("gap {}", class.as_str()),
                WorkKind::Feature => "feature".to_string(),
            };
            lines.push(format!("explain: {} [{badge}] — {}", entry.id, entry.title));
            match entry.kind {
                WorkKind::Gap { .. } => {
                    lines.push(format!(
                        "  caps: {}  witness: {}",
                        entry
                            .caps
                            .map_or_else(|| "-".to_string(), |l| l.to_string()),
                        entry
                            .witness
                            .clone()
                            .unwrap_or_else(|| "none — witness debt".to_string())
                    ));
                    lines.push(format!(
                        "  affects: {}",
                        if entry.related.is_empty() {
                            "-".to_string()
                        } else {
                            entry.related.join(", ")
                        }
                    ));
                }
                WorkKind::Feature => {
                    lines.push(format!(
                        "  area: {}  tier: {}  target: {}",
                        entry.area.clone().unwrap_or_else(|| "-".to_string()),
                        entry
                            .tier
                            .map_or_else(|| "-".to_string(), |t| t.as_str().to_string()),
                        entry
                            .target_level
                            .map_or_else(|| "-".to_string(), |l| l.to_string())
                    ));
                    lines.push(format!(
                        "  dependants: {}",
                        if entry.related.is_empty() {
                            "-".to_string()
                        } else {
                            entry.related.join(", ")
                        }
                    ));
                }
            }
            lines.push(format!(
                "  lane: {}",
                entry
                    .lane
                    .map_or_else(|| "-".to_string(), |l| l.to_string())
            ));
            lines.push(format!("  why: {}", entry.why));
            lines.push(format!("  evidence: {}", entry.evidence_needed));
            lines.push(format!("  files: {}", entry.suggested_files.join(", ")));
        }
        None => lines.push("explain: no registry entry matched the requested ID".to_string()),
    }
    for diagnostic in &outcome.diagnostics {
        lines.push(diagnostic.to_string());
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    use ledger_schema::{GapClass, Lane};

    // ---------------------------------------------------------------------------
    // fixtures
    // ---------------------------------------------------------------------------

    fn feature(id: &str, requires: &[&str], lane: Lane) -> Sourced<ledger_schema::FeatureEntry> {
        Sourced {
            path: std::path::PathBuf::from(format!("ledger/features/lang/{id}.toml")),
            entry: ledger_schema::FeatureEntry {
                id: id.to_string(),
                title: format!("feature {id}"),
                area: "lang.control-flow".into(),
                tier: ledger_schema::Tier::Std,
                spec: "docs/specs/v0.1.0.md".into(),
                target_level: ledger_schema::Level::L4,
                backends: vec![ledger_schema::Backend::Gam, ledger_schema::Backend::Llvm],
                requires: requires.iter().map(|r| (*r).to_string()).collect(),
                errors: Vec::new(),
                inventory: Vec::new(),
                owner_lane: lane,
            },
        }
    }

    use ledger_schema::Sourced;

    fn gap(
        id: &str,
        class: GapClass,
        witnessed: bool,
        affects: &[&str],
    ) -> Sourced<ledger_schema::GapEntry> {
        Sourced {
            path: std::path::PathBuf::from(format!("ledger/gaps/known/{id}.toml")),
            entry: ledger_schema::GapEntry {
                id: id.to_string(),
                class,
                title: format!("gap {id}"),
                affects: affects.iter().map(|a| (*a).to_string()).collect(),
                caps: ledger_schema::Level::L2,
                witness: witnessed.then(|| format!("witness-{id}.g")),
                expect: None,
                opened_in: "v0.3.0".into(),
                target_milestone: None,
                prevention: Vec::new(),
                status: if witnessed {
                    ledger_schema::GapStatus::Witnessed
                } else {
                    ledger_schema::GapStatus::Unwitnessed
                },
            },
        }
    }

    /// Write the fixture registry into a tempdir so `run_explain` loads it
    /// exactly like the CLI does.
    fn write_registry(
        dir: &Path,
        features: &[Sourced<ledger_schema::FeatureEntry>],
        gaps: &[Sourced<ledger_schema::GapEntry>],
    ) {
        std::fs::create_dir_all(dir.join("ledger/features/lang")).expect("mkdir features");
        std::fs::create_dir_all(dir.join("ledger/gaps/known")).expect("mkdir gaps");
        std::fs::create_dir_all(dir.join("docs/specs")).expect("mkdir specs");
        std::fs::write(dir.join("docs/specs/v0.1.0.md"), "# spec\n").expect("spec");
        let mut feature_toml = String::new();
        for sourced in features {
            use std::fmt::Write as _;
            let requires = sourced
                .entry
                .requires
                .iter()
                .map(|r| format!("\"{r}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                feature_toml,
                "[[feature]]\n\
                 id = \"{}\"\n\
                 title = \"{}\"\n\
                 area = \"{}\"\n\
                 tier = \"std\"\n\
                 spec = \"docs/specs/v0.1.0.md\"\n\
                 target_level = \"L4\"\n\
                 backends = [\"gam\", \"llvm\"]\n\
                 requires = [{requires}]\n\
                 owner_lane = \"{}\"\n\n",
                sourced.entry.id, sourced.entry.title, sourced.entry.area, sourced.entry.owner_lane,
            );
        }
        // Only write registry files that actually declare entries: the
        // loader rejects empty files (LEDGER-EMPTY-FILE), and fixtures
        // that exercise one side only must not trip it.
        if !feature_toml.is_empty() {
            std::fs::write(dir.join("ledger/features/lang/reg.toml"), feature_toml)
                .expect("features");
        }
        let mut gap_toml = String::new();
        for sourced in gaps {
            use std::fmt::Write as _;
            let affects = sourced
                .entry
                .affects
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let witness_line = sourced
                .entry
                .witness
                .as_ref()
                .map(|w| format!("witness = \"{w}\"\n"))
                .unwrap_or_default();
            let status_line = match sourced.entry.status {
                ledger_schema::GapStatus::Witnessed => "witnessed",
                ledger_schema::GapStatus::Unwitnessed => "unwitnessed",
            };
            let _ = writeln!(
                gap_toml,
                "[[gap]]\n\
                 id = \"{}\"\n\
                 class = \"{}\"\n\
                 title = \"{}\"\n\
                 affects = [{affects}]\n\
                 caps = \"L2\"\n\
                 opened_in = \"v0.3.0\"\n\
                 status = \"{status_line}\"\n\
                 {witness_line}\n",
                sourced.entry.id,
                sourced.entry.class.as_str(),
                sourced.entry.title,
            );
            if let Some(witness) = &sourced.entry.witness {
                std::fs::write(
                    dir.join(witness),
                    format!("//@ known-gap: {}\nfn main() {{}}\n", sourced.entry.id),
                )
                .expect("witness fixture");
            }
        }
        if !gap_toml.is_empty() {
            std::fs::write(dir.join("ledger/gaps/known/reg.toml"), gap_toml).expect("gaps");
        }
    }

    #[test]
    fn explain_finds_a_witnessed_gap_with_its_metadata() {
        let dir = tempfile::tempdir().expect("tmp");
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![gap("GAP-0001", GapClass::A, true, &["LANG-A"])];
        write_registry(dir.path(), &features, &gaps);

        let outcome = super::run_explain(dir.path(), "GAP-0001");
        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        let entry = outcome.entry.expect("found");
        assert_eq!(entry.id, "GAP-0001");
        assert!(matches!(
            entry.kind,
            crate::WorkKind::Gap {
                class: GapClass::A,
                witnessed: true
            }
        ));
        assert_eq!(entry.caps.expect("caps").as_str(), "L2");
        assert!(
            entry
                .suggested_files
                .iter()
                .any(|f| f.ends_with("witness-GAP-0001.g"))
        );
        assert_eq!(entry.lane, Some(Lane::M));
    }

    #[test]
    fn explain_flags_unwitnessed_gaps_as_witness_debt() {
        let dir = tempfile::tempdir().expect("tmp");
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![gap("GAP-0002", GapClass::B, false, &["LANG-A"])];
        write_registry(dir.path(), &features, &gaps);

        let outcome = super::run_explain(dir.path(), "GAP-0002");
        assert!(outcome.entry.is_some());
        let text = super::format_explain_report(&outcome);
        assert!(text.contains("witness debt"), "{text}");
        assert!(text.contains("`//@ known-gap: GAP-0002`"), "{text}");
    }

    #[test]
    fn explain_finds_a_feature_with_requires_and_dependants() {
        let dir = tempfile::tempdir().expect("tmp");
        let features = vec![
            feature("LANG-ROOT", &[], Lane::M),
            feature("LANG-MID", &["LANG-ROOT"], Lane::T),
        ];
        write_registry(dir.path(), &features, &[]);

        let outcome = super::run_explain(dir.path(), "LANG-ROOT");
        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        let entry = outcome.entry.clone().expect("found");
        assert!(matches!(entry.kind, crate::WorkKind::Feature));
        assert_eq!(entry.related, vec!["LANG-MID".to_string()]);
        let text = super::format_explain_report(&outcome);
        assert!(text.contains("LANG-MID"), "{text}");
        assert!(text.contains("target: L4"), "{text}");
    }

    #[test]
    fn unknown_ids_yield_a_not_found_outcome_with_diagnostics_ok() {
        let dir = tempfile::tempdir().expect("tmp");
        write_registry(dir.path(), &[], &[]);
        let outcome = super::run_explain(dir.path(), "GAP-9999");
        assert!(
            outcome.is_ok(),
            "unknown ID is a report, not a load failure"
        );
        assert!(outcome.entry.is_none());
        let text = super::format_explain_report(&outcome);
        assert!(text.contains("no registry entry"), "{text}");
    }

    #[test]
    fn explain_why_matches_the_next_queue_word_for_word() {
        // The seam-reuse contract: `explain` renders the same queue item the
        // `next` engine produces for the same registry state.
        let dir = tempfile::tempdir().expect("tmp");
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![gap("GAP-0001", GapClass::A, true, &["LANG-A"])];
        write_registry(dir.path(), &features, &gaps);

        let explained = super::run_explain(dir.path(), "GAP-0001")
            .entry
            .expect("found");
        let queue = crate::build_queue(&[
            &crate::GapSource::new(&gaps, &|id| {
                features
                    .iter()
                    .find(|f| f.id() == id)
                    .map(|f| f.entry.owner_lane)
            }),
            &crate::FeatureSource::new(&features),
        ]);
        let queued = queue.iter().find(|w| w.id == "GAP-0001").expect("queued");
        assert_eq!(explained.why, queued.why);
        assert_eq!(explained.evidence_needed, queued.evidence_needed);
    }

    #[test]
    fn explain_report_is_deterministic_byte_for_byte() {
        let dir = tempfile::tempdir().expect("tmp");
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![gap("GAP-0001", GapClass::A, true, &["LANG-A"])];
        write_registry(dir.path(), &features, &gaps);
        let render = || super::format_explain_report(&super::run_explain(dir.path(), "GAP-0001"));
        assert_eq!(render(), render());
    }

    // ---------------------------------------------------------------------------
    // CLI surface (arg parsing contract)
    // ---------------------------------------------------------------------------

    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items.iter().map(|s| (*s).to_string())
    }

    #[test]
    fn explain_takes_an_id_and_an_optional_root() {
        match crate::parse_args(args(&["explain", "GAP-0002", "/tmp/reg"])).expect("parse") {
            Command::Explain { id, root } => {
                assert_eq!(id, "GAP-0002");
                assert_eq!(root, Path::new("/tmp/reg"));
            }
            other => panic!("expected Command::Explain, got {other:?}"),
        }
    }

    #[test]
    fn explain_defaults_root_to_current_dir() {
        match crate::parse_args(args(&["explain", "LANG-A"])).expect("parse") {
            Command::Explain { id, root } => {
                assert_eq!(id, "LANG-A");
                assert_eq!(root, Path::new("."));
            }
            other => panic!("expected Command::Explain, got {other:?}"),
        }
    }

    #[test]
    fn explain_requires_an_id() {
        let err = crate::parse_args(args(&["explain"])).expect_err("reject");
        assert!(
            matches!(err, crate::LedgerCliError::ExplainMissingId { .. }),
            "{err}"
        );
        assert!(miette::Diagnostic::help(&err).is_some());
    }

    #[test]
    fn explain_rejects_extra_positionals_and_unknown_flags() {
        let err = crate::parse_args(args(&["explain", "A", "b", "c"])).expect_err("reject");
        assert!(matches!(err, crate::LedgerCliError::ExtraArgument { ref arg, .. } if arg == "c"));
        let err = crate::parse_args(args(&["explain", "--wat"])).expect_err("reject");
        assert!(
            matches!(err, crate::LedgerCliError::UnknownFlag { ref flag, .. } if flag == "--wat")
        );
    }
}
