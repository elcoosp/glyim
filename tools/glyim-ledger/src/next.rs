//! `ledger next` — the plan §3.9 deterministic work queue for the agent.
//!
//! A stable, lexicographically ordered stream of [`WorkItem`]s built from
//! the open gap registry and the feature `requires` DAG. The output is the
//! `glyim.ledger-next/1` JSON contract consumed by `glyim-pilot`; a text
//! renderer of the same queue backs the `next_queue` example.
//!
//! # Priority (plan §3.9, grounded in the data the registry has today)
//!
//! The master plan's queue keys on evidence the level engine (`ledger
//! compute`, M1.8) does not produce yet — failing release gates, prevention
//! debt, missing-vs-implemented evidence. Those axes are *omitted*, not
//! faked; the ordering below is the defensible subset over open gaps and
//! the `requires` DAG, and it is stable under future extensions (new
//! sources slot in through [`WorkItemSource`] without touching the
//! comparator):
//!
//! 1. **class-b gaps** — silent wrong behaviour, always first
//!    ([`GapClass::B`]); witnessed entries before unwitnessed, then ID.
//! 2. **class-a gaps** — deliberate hard errors with a diagnostic.
//! 3. **class-c witness debt** — deferred gaps without a strict-xfail
//!    witness (plan §3.3 witness debt is its own priority).
//! 4. **class-c witnessed gaps** — deferred work with a documented
//!    contract.
//! 5. **features** — blocked only by implementation or missing evidence:
//!    shallowest `requires` depth first (cheapest foundations), then most
//!    direct dependants (unblocks the most), then ID.
//!
//! Within every rank the tie-break is the registry ID, so the queue is a
//! total order over the registry — byte-identical across runs and
//! machines for the same registry state (the same determinism contract
//! the `gaps` and `check` renderers pin).
//!
//! # Seams
//!
//! [`WorkItemSource`] decouples the ordering engine from the registry:
//! [`GapSource`] and [`FeatureSource`] adapt today's `ledger-schema`
//! entries into work items, and the future evidence engine (M1.8) adds
//! further sources without modifying this module's comparator. The depth
//! computation is a bounded fixpoint, so a (registry-rejected) cyclic
//! `requires` graph terminates instead of panicking.

use std::collections::HashMap;
use std::path::Path;

use ledger_schema::{
    Diagnostic, FeatureEntry, GapClass, GapEntry, GapStatus, Lane, Severity, Sourced,
};
use serde::Serialize;

/// Schema tag of the JSON contract (pinned by `next.rs` tests; bump the
/// minor when adding fields, the major when changing or removing them).
pub const NEXT_SCHEMA: &str = "glyim.ledger-next/1";

/// Kind of a [`WorkItem`]: where it came from and the gap metadata that
/// drives the priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkKind {
    /// An open `[[gap]]` registry entry.
    Gap {
        /// Gap class (`a` diagnosed, `b` silent, `c` deferred).
        class: GapClass,
        /// Whether a strict-xfail witness fixture reproduces the gap.
        witnessed: bool,
    },
    /// A `[[feature]]` registry entry.
    Feature,
}

/// One prioritised item of the work queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkItem {
    /// 1-based position in the full (unfiltered) queue; preserved by
    /// [`filter`] so a lane-filtered queue shows honest, non-contiguous
    /// ranks.
    pub rank: usize,
    /// Registry ID (`GAP-NNNN` or a feature ID).
    pub id: String,
    /// Human-readable entry title.
    pub title: String,
    /// Gap or feature provenance (drives the priority).
    pub kind: WorkKind,
    /// Deterministic one-line reason the item is queued.
    pub why: String,
    /// What evidence or change moves the item out of the queue.
    pub evidence_needed: String,
    /// Registry file (and witness fixture) the item lives in.
    pub suggested_files: Vec<String>,
    /// Suggested owner lane: the gap's first affected feature's lane, or
    /// the feature's own `owner_lane`; `None` when unresolvable.
    pub lane: Option<Lane>,
    /// Gap → affected feature IDs; feature → direct dependant IDs.
    pub related: Vec<String>,
    /// Feature-only ordering metadata: `requires`-DAG depth (gap items
    /// carry 0). Not part of the JSON contract.
    #[serde(skip)]
    pub depth: u32,
}

/// Source of work items for [`build_queue`].
///
/// The seam that keeps the comparator independent of where items come
/// from: the two adapters below cover the registry as it exists today,
/// and the M1.8 evidence engine can contribute further sources without
/// this module's ordering changing shape.
pub trait WorkItemSource {
    /// The (unsorted) work items contributed by this source.
    fn collect(&self) -> Vec<WorkItem>;
}

/// Adapts open `[[gap]]` registry entries into work items.
pub struct GapSource<'a> {
    gaps: &'a [Sourced<GapEntry>],
    lane_of: &'a dyn Fn(&str) -> Option<Lane>,
}

impl<'a> GapSource<'a> {
    /// Build a source over `gaps`; `lane_of` resolves the suggested lane
    /// from a feature ID (wired by the caller from the feature registry).
    pub fn new(gaps: &'a [Sourced<GapEntry>], lane_of: &'a dyn Fn(&str) -> Option<Lane>) -> Self {
        Self { gaps, lane_of }
    }
}

impl WorkItemSource for GapSource<'_> {
    fn collect(&self) -> Vec<WorkItem> {
        self.gaps
            .iter()
            .map(|sourced| {
                let gap = &sourced.entry;
                let witnessed = matches!(gap.status, GapStatus::Witnessed);
                let (why, evidence_needed) = match (gap.class, witnessed) {
                    (GapClass::B, true) => (
                        "class-b gap: silent wrong behaviour — the work queue's first priority \
                         (plan §3.9)"
                            .to_string(),
                        "closing change that retires the strict-xfail witness fixture".to_string(),
                    ),
                    (GapClass::B, false) => (
                        "class-b gap: silent wrong behaviour — the work queue's first priority \
                         (plan §3.9)"
                            .to_string(),
                        format!(
                            "strict-xfail witness fixture carrying `//@ known-gap: {}`",
                            gap.id
                        ),
                    ),
                    (GapClass::A, true) => (
                        "class-a gap: deliberate hard error with a diagnosed diagnostic"
                            .to_string(),
                        "closing change that retires the strict-xfail witness fixture".to_string(),
                    ),
                    (GapClass::A, false) => (
                        "class-a gap: deliberate hard error with a diagnosed diagnostic"
                            .to_string(),
                        format!(
                            "strict-xfail witness fixture carrying `//@ known-gap: {}`",
                            gap.id
                        ),
                    ),
                    (GapClass::C, true) => (
                        "class-c gap: deferred behaviour with no code path and no diagnostic \
                         needed"
                            .to_string(),
                        "closing change; the witness fixture documents today's contract"
                            .to_string(),
                    ),
                    (GapClass::C, false) => (
                        "class-c witness debt: registered gap without a strict-xfail witness \
                         (plan §3.3)"
                            .to_string(),
                        format!(
                            "strict-xfail witness fixture carrying `//@ known-gap: {}`",
                            gap.id
                        ),
                    ),
                };
                let mut suggested_files = vec![sourced.path.display().to_string()];
                if let Some(witness) = &gap.witness {
                    suggested_files.push(witness.clone());
                }
                WorkItem {
                    rank: 0,
                    id: gap.id.clone(),
                    title: gap.title.clone(),
                    kind: WorkKind::Gap {
                        class: gap.class,
                        witnessed,
                    },
                    why,
                    evidence_needed,
                    suggested_files,
                    lane: gap.affects.iter().find_map(|id| (self.lane_of)(id)),
                    related: gap.affects.clone(),
                    depth: 0,
                }
            })
            .collect()
    }
}

/// Adapts `[[feature]]` registry entries into work items, precomputing
/// the `requires`-DAG depth and direct-dependant sets.
pub struct FeatureSource<'a> {
    features: &'a [Sourced<FeatureEntry>],
    depths: HashMap<&'a str, u32>,
}

impl<'a> FeatureSource<'a> {
    /// Build a source over `features`.
    pub fn new(features: &'a [Sourced<FeatureEntry>]) -> Self {
        Self {
            features,
            depths: feature_depths(features),
        }
    }
}

impl WorkItemSource for FeatureSource<'_> {
    fn collect(&self) -> Vec<WorkItem> {
        let mut dependants: HashMap<&str, Vec<&str>> = HashMap::new();
        for sourced in self.features {
            for required in &sourced.entry.requires {
                dependants
                    .entry(required.as_str())
                    .or_default()
                    .push(sourced.id());
            }
        }
        self.features
            .iter()
            .map(|sourced| {
                let feature = &sourced.entry;
                let depth = self.depths.get(feature.id.as_str()).copied().unwrap_or(0);
                let mut related: Vec<String> = dependants
                    .get(feature.id.as_str())
                    .map(|deps| deps.iter().map(|d| (*d).to_string()).collect())
                    .unwrap_or_default();
                related.sort();
                WorkItem {
                    rank: 0,
                    id: feature.id.clone(),
                    title: feature.title.clone(),
                    kind: WorkKind::Feature,
                    why: format!(
                        "feature at depth {depth} of the `requires` DAG with {} direct \
                         dependant(s), blocked only by missing evidence or implementation \
                         (plan §3.9 priorities 4–5)",
                        related.len()
                    ),
                    evidence_needed: format!(
                        "fixture evidence tagged `//@ feature: {}` feeding `ledger compute` \
                         (plan M1.5/M1.8)",
                        feature.id
                    ),
                    suggested_files: vec![sourced.path.display().to_string()],
                    lane: Some(feature.owner_lane),
                    related,
                    depth,
                }
            })
            .collect()
    }
}

/// `requires`-DAG depth per feature ID: 0 for roots, otherwise
/// `1 + max(depth of requires)`.
///
/// Computed as a bounded fixpoint (`features.len()` rounds), so a cyclic
/// graph — rejected upstream by `registry.validate()` — terminates with
/// the converged-so-far depths instead of recursing forever.
pub fn feature_depths(features: &[Sourced<FeatureEntry>]) -> HashMap<&str, u32> {
    let requires_of: HashMap<&str, &[String]> = features
        .iter()
        .map(|s| (s.id(), s.entry.requires.as_slice()))
        .collect();
    let mut depths: HashMap<&str, u32> = features.iter().map(|s| (s.id(), 0)).collect();
    for _ in 0..features.len() {
        let mut changed = false;
        // Iterate in registry order (not map order) so the converged
        // depths — and the values a (registry-rejected) cycle freezes at —
        // are deterministic for a given registry state.
        for sourced in features {
            let id = sourced.id();
            // Unknown require IDs (rejected by validation) count as roots.
            let candidate = requires_of
                .get(id)
                .expect("keyed by feature id")
                .iter()
                .map(|r| depths.get(r.as_str()).copied().unwrap_or(0) + 1)
                .max()
                .unwrap_or(0);
            let slot = depths.get_mut(id).expect("keyed by feature id");
            if candidate > *slot {
                *slot = candidate;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    depths
}

/// Priority rank of a work item (1 = first; see the module docs).
fn rank_of(kind: &WorkKind) -> u8 {
    match kind {
        WorkKind::Gap {
            class: GapClass::B, ..
        } => 1,
        WorkKind::Gap {
            class: GapClass::A, ..
        } => 2,
        WorkKind::Gap {
            class: GapClass::C,
            witnessed,
        } => {
            if *witnessed {
                4
            } else {
                3
            }
        }
        WorkKind::Feature => 5,
    }
}

/// Within-rank sub-order for gaps: witnessed entries first (their repro
/// exists); class-c ranks already split on witnessing, so they compare
/// equal here.
fn sub_order(kind: &WorkKind) -> u8 {
    match kind {
        WorkKind::Gap {
            class: GapClass::B | GapClass::A,
            witnessed,
        } => u8::from(!*witnessed),
        _ => 0,
    }
}

/// Build the deterministic work queue: collect from every source, order
/// by the module-doc priority (class-b → class-a → class-c debt →
/// class-c deferred → features by depth/dependants/ID), de-duplicate by
/// ID, and assign dense 1-based ranks.
pub fn build_queue(sources: &[&dyn WorkItemSource]) -> Vec<WorkItem> {
    let mut items: Vec<WorkItem> = sources.iter().flat_map(|s| s.collect()).collect();
    items.sort_by(|a, b| {
        rank_of(&a.kind)
            .cmp(&rank_of(&b.kind))
            .then_with(|| match (&a.kind, &b.kind) {
                (WorkKind::Feature, WorkKind::Feature) => a
                    .depth
                    .cmp(&b.depth)
                    .then_with(|| b.related.len().cmp(&a.related.len()))
                    .then_with(|| a.id.cmp(&b.id)),
                _ => sub_order(&a.kind)
                    .cmp(&sub_order(&b.kind))
                    .then_with(|| a.id.cmp(&b.id)),
            })
    });
    items.dedup_by(|a, b| a.id == b.id);
    for (idx, item) in items.iter_mut().enumerate() {
        item.rank = idx + 1;
    }
    items
}

/// Result of a `ledger next` run: the full ranked queue plus any
/// load/validation diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextOutcome {
    /// Load + validation diagnostics, sorted and de-duplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// Pre-filter queue length (lane/limit selection never renumbers).
    pub total: usize,
    /// The ranked queue (after [`filter`] selection, if any).
    pub queue: Vec<WorkItem>,
}

impl NextOutcome {
    /// Number of error-severity diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// True when no error-severity diagnostic was produced.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }
}

/// Run the §3.9 work queue over the registry under `root` (the project
/// directory that contains `ledger/`): load + validate, then rank every
/// open gap and feature.
#[must_use]
pub fn run_next(root: &Path) -> NextOutcome {
    let span = tracing::info_span!("next_queue", root = %root.display());
    let _enter = span.enter();

    let loaded = ledger_schema::load_project_registry(root);
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());

    let features = loaded.registry.features().to_vec();
    let lane_of = |id: &str| {
        features
            .iter()
            .find(|f| f.id() == id)
            .map(|f| f.entry.owner_lane)
    };
    let queue = build_queue(&[
        &GapSource::new(loaded.registry.gaps(), &lane_of),
        &FeatureSource::new(&features),
    ]);

    tracing::debug!(
        total = queue.len(),
        diagnostics = diagnostics.len(),
        "next queue built"
    );

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    NextOutcome {
        diagnostics,
        total: queue.len(),
        queue,
    }
}

/// Select a subset of the queue: optional lane filter first, then an
/// optional prefix limit. Ranks and `total` are preserved so consumers
/// see where each item stands in the full queue.
#[must_use]
pub fn filter(outcome: &NextOutcome, lane: Option<Lane>, limit: Option<usize>) -> NextOutcome {
    let queue: Vec<WorkItem> = outcome
        .queue
        .iter()
        .filter(|item| lane.is_none_or(|l| item.lane == Some(l)))
        .take(limit.unwrap_or(usize::MAX))
        .cloned()
        .collect();
    NextOutcome {
        diagnostics: outcome.diagnostics.clone(),
        total: outcome.total,
        queue,
    }
}

/// Parse a lane spelling (`L`/`F`/`T`/`M`/`B`/`Q`); `None` for anything
/// else.
#[must_use]
pub fn parse_lane(spelling: &str) -> Option<Lane> {
    match spelling {
        "L" => Some(Lane::L),
        "F" => Some(Lane::F),
        "T" => Some(Lane::T),
        "M" => Some(Lane::M),
        "B" => Some(Lane::B),
        "Q" => Some(Lane::Q),
        _ => None,
    }
}

/// JSON wire format of the queue (`glyim.ledger-next/1`).
#[derive(Serialize)]
struct NextJson<'a> {
    schema: &'static str,
    /// Pre-filter queue size (what a lane/limit selection drew from).
    total: usize,
    /// Emitted item count.
    count: usize,
    items: &'a [WorkItem],
}

/// Render the queue as the `glyim.ledger-next/1` JSON contract
/// (pretty-printed, trailing newline) — the §3.9 output `glyim-pilot`
/// consumes.
#[must_use]
pub fn format_next_json(outcome: &NextOutcome) -> String {
    let document = NextJson {
        schema: NEXT_SCHEMA,
        total: outcome.total,
        count: outcome.queue.len(),
        items: &outcome.queue,
    };
    let mut text = serde_json::to_string_pretty(&document).unwrap_or_else(|err| {
        // Field sets are fixed and values are plain strings/numbers, so
        // serialization is infallible by construction; this arm keeps the
        // renderer total without introducing a Result into the format_*
        // family contract.
        format!("{{\"schema\":\"{NEXT_SCHEMA}\",\"error\":\"{err}\"}}")
    });
    text.push('\n');
    text
}

/// Render the queue as the human-readable report used by the
/// `next_queue` example: a summary line, one block per item, then any
/// diagnostics.
#[must_use]
pub fn format_next_report(outcome: &NextOutcome) -> String {
    let mut lines = vec![format!(
        "next: {} of {} item(s) queued (schema {NEXT_SCHEMA})",
        outcome.queue.len(),
        outcome.total,
    )];
    for item in &outcome.queue {
        let kind = match item.kind {
            WorkKind::Gap {
                class,
                witnessed: false,
            } => format!("gap {} debt", class.as_str()),
            WorkKind::Gap {
                class,
                witnessed: true,
            } => format!("gap {}", class.as_str()),
            WorkKind::Feature => "feature".to_string(),
        };
        lines.push(format!(
            "  {:<8} [{:<12}] {} — {}",
            item.rank, kind, item.id, item.title
        ));
        lines.push(format!("         why: {}", item.why));
        lines.push(format!("         evidence: {}", item.evidence_needed));
        lines.push(format!(
            "         files: {}",
            if item.suggested_files.is_empty() {
                "-".to_string()
            } else {
                item.suggested_files.join(", ")
            }
        ));
        lines.push(format!(
            "         lane: {}  related: {}",
            item.lane.map_or_else(|| "-".to_string(), |l| l.to_string()),
            if item.related.is_empty() {
                "-".to_string()
            } else {
                item.related.join(", ")
            }
        ));
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
    use crate::{Command, LedgerCliError};
    use std::path::Path;

    // ------------------------------------------------------------------
    // fixtures
    // ------------------------------------------------------------------

    /// A minimal valid `[[feature]]` entry (`FeatureEntry` is a plain
    /// struct).
    fn feature(id: &str, requires: &[&str], lane: Lane) -> Sourced<FeatureEntry> {
        Sourced {
            path: std::path::PathBuf::from(format!("ledger/features/lang/{id}.toml")),
            entry: FeatureEntry {
                id: id.to_string(),
                title: format!("feature {id}"),
                area: "lang.control-flow".into(),
                tier: ledger_schema::Tier::Core,
                spec: "docs/specs/v0.1.0.md".into(),
                target_level: ledger_schema::Level::L5,
                backends: vec![ledger_schema::Backend::Gam, ledger_schema::Backend::Llvm],
                requires: requires.iter().map(|r| (*r).to_string()).collect(),
                errors: Vec::new(),
                inventory: Vec::new(),
                owner_lane: lane,
            },
        }
    }

    /// A minimal valid `[[gap]]` entry.
    fn gap(id: &str, class: GapClass, witnessed: bool, affects: &[&str]) -> Sourced<GapEntry> {
        Sourced {
            path: std::path::PathBuf::from(format!("ledger/gaps/known/{id}.toml")),
            entry: GapEntry {
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
                    GapStatus::Witnessed
                } else {
                    GapStatus::Unwitnessed
                },
            },
        }
    }

    /// Lane lookup that maps every feature ID to the lane it was built
    /// with.
    fn lane_of(features: &[Sourced<FeatureEntry>]) -> impl Fn(&str) -> Option<Lane> + '_ {
        move |id| {
            features
                .iter()
                .find(|f| f.id() == id)
                .map(|f| f.entry.owner_lane)
        }
    }

    // ------------------------------------------------------------------
    // ordering contract
    // ------------------------------------------------------------------

    #[test]
    fn class_b_gaps_rank_before_class_a_then_class_c() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![
            gap("GAP-0003", GapClass::C, true, &["LANG-A"]),
            gap("GAP-0001", GapClass::A, true, &["LANG-A"]),
            gap("GAP-0002", GapClass::B, true, &["LANG-A"]),
        ];
        let queue = build_queue(&[
            &GapSource::new(&gaps, &lane_of(&features)),
            &FeatureSource::new(&features),
        ]);
        let ids: Vec<&str> = queue.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["GAP-0002", "GAP-0001", "GAP-0003", "LANG-A"]);
    }

    #[test]
    fn within_a_rank_witnessed_gaps_sort_before_unwitnessed_then_by_id() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![
            gap("GAP-0009", GapClass::B, false, &["LANG-A"]),
            gap("GAP-0002", GapClass::B, false, &["LANG-A"]),
            gap("GAP-0007", GapClass::B, true, &["LANG-A"]),
        ];
        let queue = build_queue(&[&GapSource::new(&gaps, &lane_of(&features))]);
        let ids: Vec<&str> = queue.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["GAP-0007", "GAP-0002", "GAP-0009"]);
    }

    #[test]
    fn unwitnessed_class_c_gaps_rank_as_witness_debt_before_deferred_work() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![
            gap("GAP-0001", GapClass::C, true, &["LANG-A"]),
            gap("GAP-0005", GapClass::C, false, &["LANG-A"]),
        ];
        let queue = build_queue(&[&GapSource::new(&gaps, &lane_of(&features))]);
        let ids: Vec<&str> = queue.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["GAP-0005", "GAP-0001"]);
    }

    #[test]
    fn features_rank_by_requires_depth_then_dependants_then_id() {
        // ROOT: depth 0, 2 dependants. ALT: depth 1, 2 dependants
        // (LEAF, MID2). MID: depth 1, 1 dependant (LEAF). LEAF requires
        // ALT and MID: depth 2, 0 dependants. MID2: depth 2, 0 dependants.
        // So: ROOT first (depth 0); at depth 1 ALT outranks MID (2 vs 1
        // dependants); at depth 2 LEAF and MID2 tie on dependants and sort
        // by ID.
        let features = vec![
            feature("LANG-MID2", &["LANG-ALT"], Lane::M),
            feature("LANG-LEAF", &["LANG-ALT", "LANG-MID"], Lane::M),
            feature("LANG-MID", &["LANG-ROOT"], Lane::M),
            feature("LANG-ROOT", &[], Lane::M),
            feature("LANG-ALT", &["LANG-ROOT"], Lane::M),
        ];
        let queue = build_queue(&[&FeatureSource::new(&features)]);
        let ids: Vec<&str> = queue.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "LANG-ROOT",
                // depth 1: ALT has 2 dependants, MID has 1.
                "LANG-ALT",
                "LANG-MID",
                // depth 2: both have 0 dependants; ID tiebreak.
                "LANG-LEAF",
                "LANG-MID2",
            ]
        );
    }

    #[test]
    fn ranks_are_one_based_and_dense_for_the_full_queue() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![
            gap("GAP-0001", GapClass::B, true, &["LANG-A"]),
            gap("GAP-0002", GapClass::A, true, &["LANG-A"]),
        ];
        let queue = build_queue(&[
            &GapSource::new(&gaps, &lane_of(&features)),
            &FeatureSource::new(&features),
        ]);
        for (idx, item) in queue.iter().enumerate() {
            assert_eq!(item.rank, idx + 1);
        }
    }

    #[test]
    fn cyclic_requires_terminate_and_still_rank() {
        let features = vec![
            feature("LANG-X", &["LANG-Y"], Lane::M),
            feature("LANG-Y", &["LANG-X"], Lane::M),
        ];
        let queue = build_queue(&[&FeatureSource::new(&features)]);
        let ids: Vec<&str> = queue.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["LANG-X", "LANG-Y"]);
    }

    #[test]
    fn duplicate_ids_collapse_to_one_item() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![
            gap("GAP-0001", GapClass::B, true, &["LANG-A"]),
            gap("GAP-0001", GapClass::B, true, &["LANG-A"]),
        ];
        let queue = build_queue(&[&GapSource::new(&gaps, &lane_of(&features))]);
        assert_eq!(queue.len(), 1);
    }

    // ------------------------------------------------------------------
    // payload contract
    // ------------------------------------------------------------------

    #[test]
    fn gap_items_carry_lane_affected_features_and_registry_file() {
        let features = vec![feature("LANG-UNWIND", &[], Lane::B)];
        let gaps = vec![gap("GAP-0002", GapClass::B, false, &["LANG-UNWIND"])];
        let queue = build_queue(&[&GapSource::new(&gaps, &lane_of(&features))]);
        assert_eq!(queue.len(), 1);
        let item = &queue[0];
        assert_eq!(item.lane, Some(Lane::B));
        assert_eq!(item.related, vec!["LANG-UNWIND".to_string()]);
        assert!(
            item.suggested_files
                .iter()
                .any(|f| f.ends_with("ledger/gaps/known/GAP-0002.toml"))
        );
        assert!(matches!(item.kind, WorkKind::Gap { .. }));
    }

    #[test]
    fn unwitnessed_gap_items_ask_for_a_witness_fixture() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![gap("GAP-0005", GapClass::C, false, &["LANG-A"])];
        let queue = build_queue(&[&GapSource::new(&gaps, &lane_of(&features))]);
        assert!(
            queue[0]
                .evidence_needed
                .contains("`//@ known-gap: GAP-0005`"),
            "{}",
            queue[0].evidence_needed
        );
    }

    #[test]
    fn feature_items_carry_owner_lane_and_dependants() {
        let features = vec![
            feature("LANG-A", &[], Lane::M),
            feature("LANG-B", &["LANG-A"], Lane::T),
        ];
        let queue = build_queue(&[&FeatureSource::new(&features)]);
        let a = queue.iter().find(|w| w.id == "LANG-A").expect("item");
        assert_eq!(a.lane, Some(Lane::M));
        assert_eq!(a.related, vec!["LANG-B".to_string()]);
    }

    #[test]
    fn queue_is_deterministic_byte_for_byte() {
        let features = vec![
            feature("LANG-A", &["LANG-B"], Lane::M),
            feature("LANG-B", &[], Lane::T),
        ];
        let gaps = vec![
            gap("GAP-0002", GapClass::B, true, &["LANG-B"]),
            gap("GAP-0001", GapClass::C, false, &["LANG-A"]),
        ];
        let build = || {
            let queue = build_queue(&[
                &GapSource::new(&gaps, &lane_of(&features)),
                &FeatureSource::new(&features),
            ]);
            format_next_json(&NextOutcome {
                diagnostics: Vec::new(),
                total: queue.len(),
                queue,
            })
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn json_pins_schema_version_and_required_fields() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![gap("GAP-0002", GapClass::B, false, &["LANG-A"])];
        let queue = build_queue(&[&GapSource::new(&gaps, &lane_of(&features))]);
        let json = format_next_json(&NextOutcome {
            diagnostics: Vec::new(),
            total: queue.len(),
            queue,
        });
        assert!(json.contains("\"glyim.ledger-next/1\""), "{json}");
        for field in [
            "\"rank\"",
            "\"id\"",
            "\"kind\"",
            "\"why\"",
            "\"evidence_needed\"",
            "\"suggested_files\"",
            "\"lane\"",
            "\"related\"",
            "\"title\"",
        ] {
            assert!(json.contains(field), "missing {field} in {json}");
        }
        assert!(json.contains("\"kind\": \"gap\""), "{json}");
        assert!(json.contains("\"class\": \"b\""), "{json}");
        assert!(json.contains("\"witnessed\": false"), "{json}");
        // The internal ordering metadata must not leak into the contract.
        assert!(!json.contains("depth"), "{json}");
    }

    #[test]
    fn empty_inputs_yield_an_empty_but_valid_queue() {
        let queue = build_queue(&[&GapSource::new(&[], &|_| None), &FeatureSource::new(&[])]);
        assert!(queue.is_empty());
        let json = format_next_json(&NextOutcome {
            diagnostics: Vec::new(),
            total: 0,
            queue,
        });
        assert!(json.contains("\"glyim.ledger-next/1\""), "{json}");
    }

    // ------------------------------------------------------------------
    // selection (lane filter + limit)
    // ------------------------------------------------------------------

    #[test]
    fn lane_filter_keeps_rank_and_reports_the_total() {
        let features = vec![
            feature("LANG-A", &[], Lane::M),
            feature("LANG-B", &[], Lane::B),
        ];
        let gaps = vec![gap("GAP-0002", GapClass::B, true, &["LANG-B"])];
        let outcome = NextOutcome {
            diagnostics: Vec::new(),
            total: 3,
            queue: build_queue(&[
                &GapSource::new(&gaps, &lane_of(&features)),
                &FeatureSource::new(&features),
            ]),
        };
        let filtered = filter(&outcome, Some(Lane::M), None);
        assert_eq!(filtered.total, 3, "total stays the pre-filter count");
        assert_eq!(filtered.queue.len(), 1);
        assert!(filtered.queue.iter().all(|w| w.lane == Some(Lane::M)));
        // The gap item (rank 1, lane B) is filtered out; the surviving
        // feature keeps its original (non-contiguous) rank.
        assert_eq!(filtered.queue[0].rank, 2);
    }

    #[test]
    fn limit_slices_the_rank_prefix() {
        let features = vec![
            feature("LANG-A", &[], Lane::M),
            feature("LANG-B", &[], Lane::M),
        ];
        let outcome = NextOutcome {
            diagnostics: Vec::new(),
            total: 2,
            queue: build_queue(&[&FeatureSource::new(&features)]),
        };
        let limited = filter(&outcome, None, Some(1));
        assert_eq!(limited.queue.len(), 1);
        assert_eq!(limited.queue[0].id, "LANG-A");
    }

    // ------------------------------------------------------------------
    // text renderer
    // ------------------------------------------------------------------

    #[test]
    fn text_report_renders_summary_items_and_diagnostics() {
        let features = vec![feature("LANG-A", &[], Lane::M)];
        let gaps = vec![gap("GAP-0002", GapClass::B, false, &["LANG-A"])];
        let queue = build_queue(&[&GapSource::new(&gaps, &lane_of(&features))]);
        let text = format_next_report(&NextOutcome {
            diagnostics: vec![Diagnostic {
                code: ledger_schema::CODE_SCHEMA,
                message: "ledger/gaps/known/x.toml: bad entry".into(),
                severity: Severity::Warning,
            }],
            total: queue.len(),
            queue,
        });
        assert!(text.contains("next: 1 of 1 item(s)"), "{text}");
        assert!(text.contains("glyim.ledger-next/1"), "{text}");
        assert!(text.contains("GAP-0002"), "{text}");
        assert!(text.contains("warning[LEDGER-SCHEMA]"), "{text}");
    }

    // ------------------------------------------------------------------
    // lane parsing (typed CLI errors)
    // ------------------------------------------------------------------

    #[test]
    fn parse_lane_accepts_only_the_six_registry_lanes() {
        for spelling in ["L", "F", "T", "M", "B", "Q"] {
            assert!(parse_lane(spelling).is_some(), "{spelling}");
        }
        for spelling in ["X", "l", "LL", ""] {
            assert!(parse_lane(spelling).is_none(), "{spelling}");
        }
    }

    // ------------------------------------------------------------------
    // CLI surface (arg parsing contract)
    // ------------------------------------------------------------------

    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items.iter().map(|s| (*s).to_string())
    }

    #[test]
    fn next_defaults_to_current_dir_without_flags() {
        match crate::parse_args(args(&["next"])).expect("parse") {
            Command::Next { root, lane, limit } => {
                assert_eq!(root, Path::new("."));
                assert_eq!(lane, None);
                assert_eq!(limit, None);
            }
            other => panic!("expected Command::Next, got {other:?}"),
        }
    }

    #[test]
    fn next_takes_root_lane_and_limit_flags() {
        match crate::parse_args(args(&["next", "/tmp/reg", "--lane", "M", "--n", "10"]))
            .expect("parse")
        {
            Command::Next { root, lane, limit } => {
                assert_eq!(root, Path::new("/tmp/reg"));
                assert_eq!(lane, Some(Lane::M));
                assert_eq!(limit, Some(10));
            }
            other => panic!("expected Command::Next, got {other:?}"),
        }
    }

    #[test]
    fn next_rejects_a_non_numeric_limit_with_help() {
        let err = crate::parse_args(args(&["next", "--n", "many"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::NextBadCount { ref value, .. } if value == "many"),
            "{err}"
        );
        assert!(miette::Diagnostic::help(&err).is_some());
    }

    #[test]
    fn next_rejects_an_unknown_lane_with_help() {
        let err = crate::parse_args(args(&["next", "--lane", "X"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::NextBadLane { ref lane, .. } if lane == "X"),
            "{err}"
        );
        assert!(miette::Diagnostic::help(&err).is_some());
    }

    #[test]
    fn next_rejects_a_flag_without_its_value() {
        let err = crate::parse_args(args(&["next", "--lane"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::NextMissingValue { ref flag, .. } if flag == "--lane"),
            "{err}"
        );
    }

    #[test]
    fn next_rejects_unknown_flags_and_extra_positionals() {
        let err = crate::parse_args(args(&["next", "--wat"])).expect_err("reject");
        assert!(matches!(err, LedgerCliError::UnknownFlag { ref flag, .. } if flag == "--wat"));
        let err = crate::parse_args(args(&["next", "a", "b"])).expect_err("reject");
        assert!(matches!(err, LedgerCliError::ExtraArgument { ref arg, .. } if arg == "b"));
    }
}
