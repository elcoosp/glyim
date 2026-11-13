//! CLI library for the Readiness Ledger tool (plan M1.1).
//!
//! Twelve subcommands:
//!
//! - `glyim-ledger check [ROOT]` — load `ROOT/ledger`, run the schema and
//!   semantic validators, cross-check the M1.3 inventory extractors, print
//!   every diagnostic sorted, exit 1 on any error. This is the CI registry
//!   gate.
//! - `glyim-ledger gaps [ROOT]` — the M1.7 gap-registry report: witness
//!   coverage per `[[gap]]` entry, with explicit witness debt rendered
//!   instead of failed. Error-severity diagnostics still fail the exit
//!   code.
//! - `glyim-ledger next [ROOT] [--lane L] [--n N]` — the plan §3.9
//!   deterministic work queue: every open gap and feature ranked into the
//!   `glyim.ledger-next/1` JSON contract consumed by `glyim-pilot`. See
//!   [`next`] for the ordering contract.
//! - `glyim-ledger explain <ID> [ROOT]` — the plan §3.7 per-entry
//!   explanation: resolve one gap or feature ID and render why it is
//!   queued and what evidence moves it (the same reason lines the `next`
//!   queue emits; see [`explain`]).
//! - `glyim-ledger compute [ROOT] [--evidence PATH] [--format text|json]
//!   [--snapshot PATH]` — the plan §6 M1.8 level-computation engine:
//!   per-feature readiness levels L0–L4 computed as a pure function of the
//!   registry, the `glyim.evidence/1` records, and `ledger/policy.toml`
//!   (L5 hardening is reported as unmeasured); see [`compute`] for the
//!   determinism and monotonicity contracts. `--snapshot PATH` also
//!   persists the `glyim.ledger-snapshot/1` document of the run.
//! - `glyim-ledger status [ROOT] [--evidence PATH] [--format md|json]` —
//!   the plan §6 M1.9 renderers over the compute outcome (first slice).
//! - `glyim-ledger diff <BASE> <CURRENT> [ROOT] [--format text|json]` —
//!   the plan §6 M1.10 readiness-regression gate (first slice): per-feature
//!   level movement between two snapshot documents, with regressions
//!   covered only by unexpired `ledger/waivers.toml` waivers (constitution
//!   C9); see [`diff`] for the gate contract.
//! - `glyim-ledger gates [ROOT] [--evidence PATH] [--format text|json]` —
//!   the plan §6 M1.12 release-gate report: every `RG-NN` gate declared in
//!   `ledger/gates.toml` with the verdict (`pass` / `fail` / `unmeasured`)
//!   the current evidence supports; `unmeasured` is displayed, never
//!   silently passed, and never failed by itself (see [`gates`]).
//! - `glyim-ledger verify [ROOT] [--evidence PATH] [--format text|json]` —
//!   the plan §6 M1.13 determinism self-check (first slice): every ledger
//!   pipeline runs twice from independent load passes and the rendered
//!   bytes must match; any drift fails the command (see [`verify`]).
//! - `glyim-ledger render-gaps [ROOT] [--format md|json] [--out PATH]
//!   [--check]` — the plan §6 M1.9 remainder: generate `KNOWN_GAPS.md`
//!   (or the `glyim.known-gaps/1` JSON face) as a pure function of the gap
//!   registry; `--out PATH` writes it, `--check` byte-compares it against
//!   the committed document and fails on any drift (see [`render`]).
//! - `glyim-ledger schema [OUT]` — write the JSON-Schema export document
//!   for both entry types to `OUT` (pretty-printed), or to stdout.
//! - `glyim-ledger inventory [ROOT]` — enumerate the source-derived
//!   inventory items (plan §3.5) with their claim/ignore status.
//!
//! Argument handling mirrors `glyim-metrics`: hand-rolled, no `clap`, so
//! the tool's own supply chain stays dependency-light.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use ledger_schema::{
    Diagnostic, GapEntry, GapStatus, Severity, Sourced, extract_inventory, inventory_diagnostics,
    load_inventory_ignore, load_project_registry, registry_schema, unmapped_item_keys,
};

pub mod compute;
pub mod diff;
pub mod explain;
pub mod gates;
pub mod next;
pub mod render;
pub mod status;
pub mod verify;

pub use compute::{
    CODE_EVIDENCE_INVALID, CODE_EVIDENCE_IO, CODE_EVIDENCE_SCHEMA, CODE_EVIDENCE_UNKNOWN_FEATURE,
    COMPUTE_SCHEMA, ComputeOutcome, EVIDENCE_WIRE_SCHEMA, EvidenceBatch, EvidenceOutcome,
    EvidenceSource, FeatureLevel, HARDENING_FUZZ_HOURS, HARDENING_MUTATION_RATE,
    InMemoryEvidenceSource, JsonlEvidenceSource, LedgerEvidence, ProjectEvidenceSource,
    compute_levels, format_compute_json, format_compute_report, run_compute,
};
pub use diff::{
    CODE_SNAPSHOT_IO, CODE_SNAPSHOT_SCHEMA, CODE_WAIVER_DUP, CODE_WAIVER_EXPIRED,
    CODE_WAIVER_HORIZON, CODE_WAIVER_SCHEMA, CODE_WAIVER_UNUSED, ComputeSnapshot, DiffDocument,
    FeatureLevelDiff, FileSnapshotStore, InMemorySnapshotStore, LEDGER_DIFF_SCHEMA,
    LedgerDiffOutcome, LevelVerdict, NaiveDate, SNAPSHOT_SCHEMA, SnapshotBatch, SnapshotEntry,
    SnapshotStore, WAIVER_MAX_AGE_DAYS, WaiverEntry, WaiverSet, diff_levels, execute_diff,
    format_diff_json, format_diff_text, load_waivers, run_diff, today_iso,
};
pub use explain::{ExplainEntry, ExplainOutcome, format_explain_report, run_explain};
pub use gates::{
    GATES_REPORT_SCHEMA, GateVerdict, GateVerdictRow, GatesOutcome, evaluate_gates,
    format_gates_json, format_gates_text, run_gates,
};
pub use next::{
    FeatureSource, GapSource, NEXT_SCHEMA, NextOutcome, WorkItem, WorkItemSource, WorkKind,
    build_queue, feature_depths, filter, format_next_json, format_next_report, parse_lane,
    run_next,
};
pub use render::{
    JsonGapRenderer, KNOWN_GAPS_MD_END, KNOWN_GAPS_MD_START, KNOWN_GAPS_SCHEMA, KnownGapDoc,
    KnownGapsDocument, KnownGapsRenderer, MarkdownGapRenderer, RenderGapsFormat, RenderGapsOutcome,
    run_render_gaps,
};
pub use status::{
    STATUS_MD_END, STATUS_MD_START, STATUS_SCHEMA, StatusOutcome, format_status_json,
    format_status_markdown, run_status,
};
pub use verify::{
    CHECK_NAMES, VERIFY_SCHEMA, VerifyOutcome, format_verify_json, format_verify_text,
    verify_determinism,
};

/// Typed error for the CLI seams (`parse_args`, `write_schema`).
///
/// Replaces the former stringly-typed `Result<_, String>` returns: variants
/// are matchable (tests assert on constructors, not message substrings) and
/// every failure carries a `miette` `help()` with the remedy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerCliError {
    /// `-h` / `--help` was requested; the usage text travels through the
    /// error channel so `main` prints it and exits 2, exactly as before.
    HelpRequested {
        /// The usage text.
        usage: String,
    },
    /// An unrecognized flag was passed.
    UnknownFlag {
        /// The offending flag.
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// An unrecognized subcommand was passed.
    UnknownSubcommand {
        /// The offending subcommand.
        subcommand: String,
        /// The usage text.
        usage: String,
    },
    /// A subcommand received more positional arguments than it accepts.
    ExtraArgument {
        /// The offending argument.
        arg: String,
        /// The usage text.
        usage: String,
    },
    /// The JSON-Schema document could not be serialized.
    SchemaSerialize {
        /// The serialization error message.
        detail: String,
    },
    /// The JSON-Schema document could not be written to its output file.
    SchemaWrite {
        /// The output path.
        path: String,
        /// The IO error message.
        detail: String,
    },
    /// `next --n` received a value that is not a positive integer.
    NextBadCount {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `next --lane` received a letter outside the six registry lanes.
    NextBadLane {
        /// The offending lane spelling.
        lane: String,
        /// The usage text.
        usage: String,
    },
    /// `explain` was called without its `<ID>` positional.
    ExplainMissingId {
        /// The usage text.
        usage: String,
    },
    /// `next --lane`/`next --n` was passed without its value.
    NextMissingValue {
        /// The flag that was missing its value (`--lane` or `--n`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `compute --format` received a value outside `text|json`.
    ComputeBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `compute --evidence`/`compute --format` was passed without its value.
    ComputeMissingValue {
        /// The flag that was missing its value (`--evidence` or `--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `status --format` received a value outside `md|json`.
    StatusBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `status --evidence`/`status --format` was passed without its value.
    StatusMissingValue {
        /// The flag that was missing its value (`--evidence` or `--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `diff` was called without its two snapshot paths.
    DiffMissingPath {
        /// The usage text.
        usage: String,
    },
    /// `diff --format` received a value outside `text|json`.
    DiffBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `diff --format` was passed without its value.
    DiffMissingValue {
        /// The flag that was missing its value (`--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `gates --format` received a value outside `text|json`.
    GatesBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `gates --evidence`/`gates --format` was passed without its value.
    GatesMissingValue {
        /// The flag that was missing its value (`--evidence` or `--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `verify --format` received a value outside `text|json`.
    VerifyBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `verify --evidence`/`verify --format` was passed without its value.
    VerifyMissingValue {
        /// The flag that was missing its value (`--evidence` or `--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `compute --snapshot` could not persist the readiness snapshot.
    SnapshotWrite {
        /// The output path.
        path: String,
        /// The diagnostic detail (IO or serialization failure).
        detail: String,
    },
    /// `render-gaps --format` received a value outside `md|json`.
    RenderBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `render-gaps --out`/`render-gaps --format` was passed without its
    /// value.
    RenderMissingValue {
        /// The flag that was missing its value (`--out` or `--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `render-gaps --check` found the committed document drifted from the
    /// registry render (or unreadable): the file and
    /// `ledger/gaps/known/*.toml` disagree (plan M1.9 determinism gate).
    RenderDrift {
        /// The checked document path.
        path: String,
        /// What the comparison (or read) found.
        detail: String,
    },
    /// `render-gaps --out` could not write the rendered document.
    RenderWrite {
        /// The output path.
        path: String,
        /// The IO error message.
        detail: String,
    },
}

impl fmt::Display for LedgerCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelpRequested { usage } => f.write_str(usage),
            Self::UnknownFlag { flag, usage } => write!(f, "unknown flag {flag}\n{usage}"),
            Self::UnknownSubcommand { subcommand, usage } => {
                write!(f, "unknown subcommand {subcommand}\n{usage}")
            }
            Self::ExtraArgument { arg, usage } => {
                write!(f, "unexpected extra argument {arg}\n{usage}")
            }
            Self::SchemaSerialize { detail } => {
                write!(f, "cannot serialize ledger schema: {detail}")
            }
            Self::SchemaWrite { path, detail } => write!(f, "cannot write {path}: {detail}"),
            Self::NextBadCount { value, usage } => {
                write!(f, "--n expects a positive integer, got {value}\n{usage}")
            }
            Self::NextBadLane { lane, usage } => {
                write!(
                    f,
                    "--lane expects one of L, F, T, M, B, Q; got {lane}\n{usage}"
                )
            }
            Self::NextMissingValue { flag, usage } => {
                write!(
                    f,
                    "{flag} expects a value (lane letter or item count)\n{usage}"
                )
            }
            Self::ComputeBadFormat { value, usage } => {
                write!(f, "--format expects text or json, got {value}\n{usage}")
            }
            Self::ComputeMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (a path, or text|json)\n{usage}")
            }
            Self::StatusBadFormat { value, usage } => {
                write!(f, "--format expects md or json, got {value}\n{usage}")
            }
            Self::StatusMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (a path, or md|json)\n{usage}")
            }
            Self::DiffMissingPath { usage } => {
                write!(
                    f,
                    "diff expects two snapshot paths (BASE CURRENT) and an \
                     optional ROOT\n{usage}"
                )
            }
            Self::DiffBadFormat { value, usage } => {
                write!(f, "--format expects text or json, got {value}\n{usage}")
            }
            Self::DiffMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (text|json)\n{usage}")
            }
            Self::GatesBadFormat { value, usage } => {
                write!(f, "--format expects text or json, got {value}\n{usage}")
            }
            Self::GatesMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (a path, or text|json)\n{usage}")
            }
            Self::VerifyBadFormat { value, usage } => {
                write!(f, "--format expects text or json, got {value}\n{usage}")
            }
            Self::VerifyMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (a path, or text|json)\n{usage}")
            }
            Self::SnapshotWrite { path, detail } => {
                write!(f, "cannot write compute snapshot {path}: {detail}")
            }
            Self::ExplainMissingId { usage } => {
                write!(f, "explain expects a registry ID (gap or feature)\n{usage}")
            }
            Self::RenderBadFormat { value, usage } => {
                write!(f, "--format expects md or json, got {value}\n{usage}")
            }
            Self::RenderMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (a path, or md|json)\n{usage}")
            }
            Self::RenderDrift { path, detail } => {
                write!(
                    f,
                    "KNOWN_GAPS document drifted from the registry: {path}: {detail}"
                )
            }
            Self::RenderWrite { path, detail } => write!(f, "cannot write {path}: {detail}"),
        }
    }
}

impl std::error::Error for LedgerCliError {}

impl miette::Diagnostic for LedgerCliError {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(match self {
            Self::HelpRequested { .. } => "glyim-ledger::help",
            Self::UnknownFlag { .. } => "glyim-ledger::unknown_flag",
            Self::UnknownSubcommand { .. } => "glyim-ledger::unknown_subcommand",
            Self::ExtraArgument { .. } => "glyim-ledger::extra_argument",
            Self::SchemaSerialize { .. } => "glyim-ledger::schema_serialize",
            Self::SchemaWrite { .. } => "glyim-ledger::schema_write",
            Self::NextBadCount { .. } => "glyim-ledger::next_bad_count",
            Self::NextBadLane { .. } => "glyim-ledger::next_bad_lane",
            Self::NextMissingValue { .. } => "glyim-ledger::next_missing_value",
            Self::ComputeBadFormat { .. } => "glyim-ledger::compute_bad_format",
            Self::ComputeMissingValue { .. } => "glyim-ledger::compute_missing_value",
            Self::StatusBadFormat { .. } => "glyim-ledger::status_bad_format",
            Self::StatusMissingValue { .. } => "glyim-ledger::status_missing_value",
            Self::DiffMissingPath { .. } => "glyim-ledger::diff_missing_path",
            Self::DiffBadFormat { .. } => "glyim-ledger::diff_bad_format",
            Self::DiffMissingValue { .. } => "glyim-ledger::diff_missing_value",
            Self::GatesBadFormat { .. } => "glyim-ledger::gates_bad_format",
            Self::GatesMissingValue { .. } => "glyim-ledger::gates_missing_value",
            Self::VerifyBadFormat { .. } => "glyim-ledger::verify_bad_format",
            Self::VerifyMissingValue { .. } => "glyim-ledger::verify_missing_value",
            Self::SnapshotWrite { .. } => "glyim-ledger::snapshot_write",
            Self::ExplainMissingId { .. } => "glyim-ledger::explain_missing_id",
            Self::RenderBadFormat { .. } => "glyim-ledger::render_bad_format",
            Self::RenderMissingValue { .. } => "glyim-ledger::render_missing_value",
            Self::RenderDrift { .. } => "glyim-ledger::render_drift",
            Self::RenderWrite { .. } => "glyim-ledger::render_write",
        }))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        match self {
            Self::HelpRequested { .. } => None,
            Self::UnknownFlag { .. } => Some(Box::new(
                "the only flag is -h/--help; subcommands are `check [ROOT]`, \
                 `gaps [ROOT]`, `schema [OUT]`, `inventory [ROOT]`"
                    .to_string(),
            )),
            Self::UnknownSubcommand { subcommand, .. } => Some(Box::new(format!(
                "`{subcommand}` is not a subcommand; try `check`, `gaps`, `schema`, or `inventory`"
            ))),
            Self::ExtraArgument { .. } => Some(Box::new(
                "each subcommand takes at most one positional argument; quote \
                 paths that contain spaces"
                    .to_string(),
            )),
            Self::SchemaSerialize { .. } => Some(Box::new(
                "serializing the built-in schema document cannot fail by \
                 construction — this is an internal bug, please report it"
                    .to_string(),
            )),
            Self::SchemaWrite { .. } => Some(Box::new(
                "check that the parent directory of OUT exists and is writable".to_string(),
            )),
            Self::NextBadCount { .. } => Some(Box::new(
                "pass the maximum number of items to print, e.g. `--n 10`; omit the flag \
                 for the full queue"
                    .to_string(),
            )),
            Self::NextBadLane { .. } => Some(Box::new(
                "lanes are L (ledger/infra), F (frontend), T (types), M (middle), \
                 B (backend), Q (quality) — see docs/roadmaps/glyim-master-plan.md §6.0"
                    .to_string(),
            )),
            Self::NextMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. `--lane M` \
                 or `--n 10`"
                    .to_string(),
            )),
            Self::ComputeBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.ledger-compute/1 contract; \
                 plain text is the default"
                    .to_string(),
            )),
            Self::ComputeMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--evidence run.jsonl` or `--format json`"
                    .to_string(),
            )),
            Self::StatusBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.status/1 contract; the \
                 marker-delimited Markdown block is the default"
                    .to_string(),
            )),
            Self::StatusMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--evidence run.jsonl` or `--format json`"
                    .to_string(),
            )),
            Self::DiffMissingPath { .. } => Some(Box::new(
                "produce the two snapshot documents first, e.g. `compute \
                 --snapshot baseline.json` on main and on this branch, then \
                 `diff baseline.json head.json`"
                    .to_string(),
            )),
            Self::DiffBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.ledger-diff/1 contract; \
                 plain text is the default"
                    .to_string(),
            )),
            Self::DiffMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--format json`"
                    .to_string(),
            )),
            Self::GatesBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.ledger-gates/1 contract; \
                 plain text is the default"
                    .to_string(),
            )),
            Self::GatesMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--evidence run.jsonl` or `--format json`"
                    .to_string(),
            )),
            Self::VerifyBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.ledger-verify/1 contract; \
                 plain text is the default"
                    .to_string(),
            )),
            Self::VerifyMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--evidence run.jsonl` or `--format json`"
                    .to_string(),
            )),
            Self::SnapshotWrite { .. } => Some(Box::new(
                "check that the --snapshot path is writable; the document is \
                 the glyim.ledger-snapshot/1 face that `diff` consumes"
                    .to_string(),
            )),
            Self::ExplainMissingId { .. } => Some(Box::new(
                "pass the registry ID to explain, e.g. `explain GAP-0002` or \
                 `explain LANG-FOR-VEC-INTOITER`"
                    .to_string(),
            )),
            Self::RenderBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.known-gaps/1 contract; \
                 the Markdown document is the default"
                    .to_string(),
            )),
            Self::RenderMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--out KNOWN_GAPS.md` or `--format json`"
                    .to_string(),
            )),
            Self::RenderDrift { .. } => Some(Box::new(
                "run `cargo run -p glyim-ledger -- render-gaps --out KNOWN_GAPS.md` \
                 to regenerate the document from the registry"
                    .to_string(),
            )),
            Self::RenderWrite { .. } => Some(Box::new(
                "check that the --out path is writable; the document is the \
                 generated KNOWN_GAPS.md face (plan M1.9)"
                    .to_string(),
            )),
        }
    }
}

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `check [ROOT]` — registry gate.
    Check {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
    },
    /// `gaps [ROOT]` — M1.7 gap-registry witness report.
    Gaps {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
    },
    /// `schema [OUT]` — JSON-Schema export.
    Schema {
        /// File to write the pretty-printed document to; `None` writes to
        /// stdout.
        out: Option<PathBuf>,
    },
    /// `inventory [ROOT]` — source inventory report (plan §3.5).
    Inventory {
        /// Project directory to extract from (`.` by default).
        root: PathBuf,
    },
    /// `next [ROOT] [--lane L] [--n N]` — the plan §3.9 work queue.
    Next {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Only items whose suggested lane matches this lane.
        lane: Option<ledger_schema::Lane>,
        /// Print at most this many items (of the filtered queue).
        limit: Option<usize>,
    },
    /// `explain <ID> [ROOT]` — the plan §3.7 per-entry explanation.
    Explain {
        /// The registry ID to explain (gap or feature).
        id: String,
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
    },
    /// `compute [ROOT] [--evidence PATH] [--format text|json] [--snapshot
    /// PATH]` — the plan §6 M1.8 level-computation engine.
    Compute {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Explicit evidence JSONL path (overrides `ledger/evidence/*.jsonl`).
        evidence: Option<PathBuf>,
        /// Output format; [`ComputeFormat::Text`] by default.
        format: ComputeFormat,
        /// When set, also persist the `glyim.ledger-snapshot/1` document of
        /// this run to `PATH` (the input face of the M1.10 `diff` gate).
        snapshot: Option<PathBuf>,
    },
    /// `status [ROOT] [--evidence PATH] [--format md|json]` — the plan
    /// §6 M1.9 status renderers (first slice).
    Status {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Explicit evidence JSONL path (overrides `ledger/evidence/*.jsonl`).
        evidence: Option<PathBuf>,
        /// Output format; [`StatusFormat::Markdown`] by default.
        format: StatusFormat,
    },
    /// `diff <BASE> <CURRENT> [ROOT] [--format text|json]` — the plan §6
    /// M1.10 readiness-regression gate: compares two `glyim.ledger-snapshot/1`
    /// documents, reports per-feature level movement, and fails on any
    /// regression not covered by an unexpired `ledger/waivers.toml` waiver.
    Diff {
        /// Baseline snapshot path (the committed readiness state).
        baseline: PathBuf,
        /// Current snapshot path (the state under review).
        current: PathBuf,
        /// Project directory containing `ledger/waivers.toml` (`.` by
        /// default).
        root: PathBuf,
        /// Output format; [`DiffFormat::Text`] by default.
        format: DiffFormat,
    },
    /// `gates [ROOT] [--evidence PATH] [--format text|json]` — the plan §6
    /// M1.12 release-gate report over `ledger/gates.toml`.
    Gates {
        /// Project directory containing `ledger/gates.toml` (`.` by
        /// default).
        root: PathBuf,
        /// Explicit evidence JSONL path (overrides `ledger/evidence/*.jsonl`).
        evidence: Option<PathBuf>,
        /// Output format; [`GatesFormat::Text`] by default.
        format: GatesFormat,
    },
    /// `verify [ROOT] [--evidence PATH] [--format text|json]` — the plan
    /// §6 M1.13 determinism self-check: every ledger pipeline runs twice
    /// from independent load passes and the rendered bytes must match.
    Verify {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Explicit evidence JSONL path (overrides `ledger/evidence/*.jsonl`).
        evidence: Option<PathBuf>,
        /// Output format; [`VerifyFormat::Text`] by default.
        format: VerifyFormat,
    },
    /// `render-gaps [ROOT] [--format md|json] [--out PATH] [--check]` —
    /// the plan §6 M1.9 remainder: generate `KNOWN_GAPS.md` (or the
    /// `glyim.known-gaps/1` JSON face) from the gap registry; `--check`
    /// fails on any drift between the committed document and the render.
    RenderGaps {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Output face; [`RenderGapsFormat::Markdown`] by default.
        format: RenderGapsFormat,
        /// File to write the rendered document to; `None` prints to
        /// stdout. `--check` compares against this file (default
        /// `<ROOT>/KNOWN_GAPS.md`).
        out: Option<PathBuf>,
        /// Compare the rendered bytes against the target document instead
        /// of writing it (the determinism gate).
        check: bool,
    },
}

/// Output format of the `compute` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeFormat {
    /// The human-readable report ([`format_compute_report`]).
    Text,
    /// The `glyim.ledger-compute/1` JSON contract ([`format_compute_json`]).
    Json,
}

/// Output format of the `status` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFormat {
    /// The marker-delimited Markdown block ([`format_status_markdown`]).
    Markdown,
    /// The `glyim.status/1` JSON contract ([`format_status_json`]).
    Json,
}

/// Output format of the `diff` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFormat {
    /// The human-readable report ([`format_diff_text`]).
    Text,
    /// The `glyim.ledger-diff/1` JSON contract ([`format_diff_json`]).
    Json,
}

/// Output format of the `gates` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatesFormat {
    /// The human-readable table ([`format_gates_text`]).
    Text,
    /// The `glyim.ledger-gates/1` JSON contract ([`format_gates_json`]).
    Json,
}

/// Output format of the `verify` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyFormat {
    /// The human-readable self-check report ([`format_verify_text`]).
    Text,
    /// The `glyim.ledger-verify/1` JSON contract ([`format_verify_json`]).
    Json,
}

/// Usage text returned with argument errors.
#[must_use]
pub fn usage() -> String {
    "usage: glyim-ledger check [ROOT]\n   or: glyim-ledger gaps [ROOT]\n   or: glyim-ledger next [ROOT] [--lane L] [--n N]\n   or: glyim-ledger explain <ID> [ROOT]\n   or: glyim-ledger compute [ROOT] [--evidence PATH] [--format text|json] [--snapshot PATH]\n   or: glyim-ledger status [ROOT] [--evidence PATH] [--format md|json]\n   or: glyim-ledger diff <BASE> <CURRENT> [ROOT] [--format text|json]\n   or: glyim-ledger gates [ROOT] [--evidence PATH] [--format text|json]\n   or: glyim-ledger verify [ROOT] [--evidence PATH] [--format text|json]\n   or: glyim-ledger render-gaps [ROOT] [--format md|json] [--out PATH] [--check]\n   or: glyim-ledger schema [OUT]\n   or: glyim-ledger inventory [ROOT]\n\
     \x20 ROOT defaults to the current directory; with no subcommand `check`\n\
     \x20 is assumed. `schema` writes to OUT or stdout; `next` filters by\n\
     \x20 lane (L|F|T|M|B|Q) and prints the glyim.ledger-next/1 JSON queue;\n\
     \x20 `explain` resolves one registry ID against the same queue;\n\
     \x20 `compute` prints per-feature readiness levels from evidence\n\
     \x20 (`ledger/evidence/*.jsonl` by default, `--evidence` to override;\n\
     \x20 `--snapshot PATH` also persists the glyim.ledger-snapshot/1\n\
     \x20 document); `status` renders the computed status as the\n\
     \x20 marker-delimited Markdown block (default) or the glyim.status/1\n\
     \x20 JSON contract; `diff` compares two snapshot documents and fails\n\
     \x20 on any regression not covered by an unexpired waiver in\n\
     \x20 `ledger/waivers.toml` (plan C9); `gates` prints the RG-01..RG-24\n\
     \x20 release-gate verdicts declared in `ledger/gates.toml` —\n\
     \x20 unmeasured gates are displayed, never silently passed; `verify`\n\
     \x20 runs every ledger pipeline twice from independent load passes\n\
     \x20 and fails on any rendered-byte drift (plan M1.13);\n\
     \x20 `render-gaps` generates KNOWN_GAPS.md (or the glyim.known-gaps/1\n\
     \x20 JSON face) from the gap registry — `--out PATH` writes it,\n\
     \x20 `--check` fails on drift (plan M1.9)."
        .into()
}

/// Parse CLI arguments (excluding argv[0]).
///
/// # Errors
///
/// Returns a typed [`LedgerCliError`] (including the usage text) for the
/// help request, unknown subcommands, unknown flags, or extra positional
/// arguments.
pub fn parse_args<I>(args: I) -> Result<Command, LedgerCliError>
where
    I: Iterator<Item = String>,
{
    let items: Vec<String> = args.collect();
    match items.first().map(String::as_str) {
        None => Ok(Command::Check {
            root: PathBuf::from("."),
        }),
        Some(
            "check" | "gaps" | "schema" | "inventory" | "next" | "explain" | "compute" | "status"
            | "diff" | "gates" | "verify" | "render-gaps",
        ) => {
            let rest = &items[1..];
            if items[0] == "next" {
                return parse_next(rest);
            }
            if items[0] == "explain" {
                return parse_explain(rest);
            }
            if items[0] == "compute" {
                return parse_compute(rest);
            }
            if items[0] == "status" {
                return parse_status(rest);
            }
            if items[0] == "diff" {
                return parse_diff(rest);
            }
            if items[0] == "gates" {
                return parse_gates(rest);
            }
            if items[0] == "verify" {
                return parse_verify(rest);
            }
            if items[0] == "render-gaps" {
                return parse_render_gaps(rest);
            }
            if rest.len() > 1 {
                return Err(LedgerCliError::ExtraArgument {
                    arg: rest[1].clone(),
                    usage: usage(),
                });
            }
            match items[0].as_str() {
                "check" => {
                    let root = rest
                        .first()
                        .map_or_else(|| PathBuf::from("."), PathBuf::from);
                    Ok(Command::Check { root })
                }
                "gaps" => {
                    let root = rest
                        .first()
                        .map_or_else(|| PathBuf::from("."), PathBuf::from);
                    Ok(Command::Gaps { root })
                }
                "inventory" => {
                    let root = rest
                        .first()
                        .map_or_else(|| PathBuf::from("."), PathBuf::from);
                    Ok(Command::Inventory { root })
                }
                _ => {
                    let out = rest.first().map(|p| PathBuf::from(p));
                    Ok(Command::Schema { out })
                }
            }
        }
        Some("-h" | "--help") => Err(LedgerCliError::HelpRequested { usage: usage() }),
        Some(flag) if flag.starts_with('-') => Err(LedgerCliError::UnknownFlag {
            flag: flag.to_string(),
            usage: usage(),
        }),
        Some(subcommand) => Err(LedgerCliError::UnknownSubcommand {
            subcommand: subcommand.to_string(),
            usage: usage(),
        }),
    }
}

/// Parse the `next` subcommand's arguments: one optional ROOT positional,
/// then `--lane <L|F|T|M|B|Q>` and `--n <count>` in any order.
fn parse_next(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut lane: Option<ledger_schema::Lane> = None;
    let mut limit: Option<usize> = None;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--lane" | "--n" => {
                let flag = rest[index].as_str();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::NextMissingValue {
                        flag: flag.to_string(),
                        usage: usage(),
                    });
                };
                if flag == "--lane" {
                    let Some(parsed) = next::parse_lane(value) else {
                        return Err(LedgerCliError::NextBadLane {
                            lane: value.clone(),
                            usage: usage(),
                        });
                    };
                    lane = Some(parsed);
                } else {
                    let Ok(count) = value.parse::<usize>() else {
                        return Err(LedgerCliError::NextBadCount {
                            value: value.clone(),
                            usage: usage(),
                        });
                    };
                    limit = Some(count);
                }
                index += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(LedgerCliError::UnknownFlag {
                    flag: flag.to_string(),
                    usage: usage(),
                });
            }
            positional => {
                if root.is_some() {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                }
                root = Some(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    Ok(Command::Next {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        lane,
        limit,
    })
}

/// Parse the `explain` subcommand's arguments: one required `<ID>`
/// positional, then one optional ROOT positional.
fn parse_explain(rest: &[String]) -> Result<Command, LedgerCliError> {
    // `explain` takes no flags: a leading dash is a usage error, not an
    // ID, so `explain --wat` reports the flag instead of explaining it.
    if let Some(flag) = rest.iter().find(|arg| arg.starts_with('-')) {
        return Err(LedgerCliError::UnknownFlag {
            flag: flag.clone(),
            usage: usage(),
        });
    }
    match rest {
        [] => Err(LedgerCliError::ExplainMissingId { usage: usage() }),
        [id] => Ok(Command::Explain {
            id: id.clone(),
            root: PathBuf::from("."),
        }),
        [id, root] => Ok(Command::Explain {
            id: id.clone(),
            root: PathBuf::from(root),
        }),
        [_, _, extra, ..] => Err(LedgerCliError::ExtraArgument {
            arg: extra.clone(),
            usage: usage(),
        }),
    }
}

/// Parse the `compute` subcommand's arguments: one optional ROOT positional,
/// then `--evidence <PATH>`, `--format <text|json>`, and `--snapshot <PATH>`
/// in any order.
fn parse_compute(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut evidence: Option<PathBuf> = None;
    let mut format = ComputeFormat::Text;
    let mut snapshot: Option<PathBuf> = None;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--evidence" | "--format" | "--snapshot" => {
                let flag = rest[index].as_str();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::ComputeMissingValue {
                        flag: flag.to_string(),
                        usage: usage(),
                    });
                };
                match flag {
                    "--evidence" => evidence = Some(PathBuf::from(value)),
                    "--snapshot" => snapshot = Some(PathBuf::from(value)),
                    _ => {
                        format = match value.as_str() {
                            "text" => ComputeFormat::Text,
                            "json" => ComputeFormat::Json,
                            other => {
                                return Err(LedgerCliError::ComputeBadFormat {
                                    value: other.to_string(),
                                    usage: usage(),
                                });
                            }
                        };
                    }
                }
                index += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(LedgerCliError::UnknownFlag {
                    flag: flag.to_string(),
                    usage: usage(),
                });
            }
            positional => {
                if root.is_some() {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                }
                root = Some(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    Ok(Command::Compute {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        evidence,
        format,
        snapshot,
    })
}

/// Parse the `status` subcommand's arguments: one optional ROOT positional,
/// then `--evidence <PATH>` and `--format <md|json>` in any order.
fn parse_status(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut evidence: Option<PathBuf> = None;
    let mut format = StatusFormat::Markdown;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--evidence" | "--format" => {
                let flag = rest[index].as_str();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::StatusMissingValue {
                        flag: flag.to_string(),
                        usage: usage(),
                    });
                };
                if flag == "--evidence" {
                    evidence = Some(PathBuf::from(value));
                } else {
                    format = match value.as_str() {
                        "md" => StatusFormat::Markdown,
                        "json" => StatusFormat::Json,
                        other => {
                            return Err(LedgerCliError::StatusBadFormat {
                                value: other.to_string(),
                                usage: usage(),
                            });
                        }
                    };
                }
                index += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(LedgerCliError::UnknownFlag {
                    flag: flag.to_string(),
                    usage: usage(),
                });
            }
            positional => {
                if root.is_some() {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                }
                root = Some(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    Ok(Command::Status {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        evidence,
        format,
    })
}

/// Parse the `diff` subcommand's arguments: two required snapshot paths
/// (`<BASE> <CURRENT>`), then an optional ROOT positional, and
/// `--format <text|json>` in any order.
fn parse_diff(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut positionals: Vec<PathBuf> = Vec::with_capacity(3);
    let mut format = DiffFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--format" => {
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::DiffMissingValue {
                        flag: "--format".to_string(),
                        usage: usage(),
                    });
                };
                format = match value.as_str() {
                    "text" => DiffFormat::Text,
                    "json" => DiffFormat::Json,
                    other => {
                        return Err(LedgerCliError::DiffBadFormat {
                            value: other.to_string(),
                            usage: usage(),
                        });
                    }
                };
                index += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(LedgerCliError::UnknownFlag {
                    flag: flag.to_string(),
                    usage: usage(),
                });
            }
            positional => {
                if positionals.len() == 3 {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                }
                positionals.push(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    if positionals.len() < 2 {
        return Err(LedgerCliError::DiffMissingPath { usage: usage() });
    }
    let baseline = positionals[0].clone();
    let current = positionals[1].clone();
    let root = positionals
        .get(2)
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Command::Diff {
        baseline,
        current,
        root,
        format,
    })
}

/// Parse the `gates` subcommand's arguments: one optional ROOT positional,
/// then `--evidence <PATH>` and `--format <text|json>` in any order.
fn parse_gates(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut evidence: Option<PathBuf> = None;
    let mut format = GatesFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--evidence" | "--format" => {
                let flag = rest[index].as_str();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::GatesMissingValue {
                        flag: flag.to_string(),
                        usage: usage(),
                    });
                };
                if flag == "--evidence" {
                    evidence = Some(PathBuf::from(value));
                } else {
                    format = match value.as_str() {
                        "text" => GatesFormat::Text,
                        "json" => GatesFormat::Json,
                        other => {
                            return Err(LedgerCliError::GatesBadFormat {
                                value: other.to_string(),
                                usage: usage(),
                            });
                        }
                    };
                }
                index += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(LedgerCliError::UnknownFlag {
                    flag: flag.to_string(),
                    usage: usage(),
                });
            }
            positional => {
                if root.is_some() {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                }
                root = Some(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    Ok(Command::Gates {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        evidence,
        format,
    })
}

/// Parse the `verify` subcommand's arguments: one optional ROOT positional,
/// then `--evidence <PATH>` and `--format <text|json>` in any order.
fn parse_verify(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut evidence: Option<PathBuf> = None;
    let mut format = VerifyFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--evidence" | "--format" => {
                let flag = rest[index].as_str();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::VerifyMissingValue {
                        flag: flag.to_string(),
                        usage: usage(),
                    });
                };
                if flag == "--evidence" {
                    evidence = Some(PathBuf::from(value));
                } else {
                    format = match value.as_str() {
                        "text" => VerifyFormat::Text,
                        "json" => VerifyFormat::Json,
                        other => {
                            return Err(LedgerCliError::VerifyBadFormat {
                                value: other.to_string(),
                                usage: usage(),
                            });
                        }
                    };
                }
                index += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(LedgerCliError::UnknownFlag {
                    flag: flag.to_string(),
                    usage: usage(),
                });
            }
            positional => {
                if root.is_some() {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                }
                root = Some(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    Ok(Command::Verify {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        evidence,
        format,
    })
}

/// Parse the `render-gaps` subcommand (plan M1.9 remainder).
fn parse_render_gaps(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut format = RenderGapsFormat::Markdown;
    let mut check = false;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--out" | "--format" => {
                let flag = rest[index].as_str();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::RenderMissingValue {
                        flag: flag.to_string(),
                        usage: usage(),
                    });
                };
                if flag == "--out" {
                    out = Some(PathBuf::from(value));
                } else {
                    format = RenderGapsFormat::parse(value).ok_or_else(|| {
                        LedgerCliError::RenderBadFormat {
                            value: value.clone(),
                            usage: usage(),
                        }
                    })?;
                }
                index += 2;
            }
            "--check" => {
                check = true;
                index += 1;
            }
            flag if flag.starts_with('-') => {
                return Err(LedgerCliError::UnknownFlag {
                    flag: flag.to_string(),
                    usage: usage(),
                });
            }
            positional => {
                if root.is_some() {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                }
                root = Some(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    Ok(Command::RenderGaps {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        format,
        out,
        check,
    })
}

/// Result of a full `ledger check` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    /// All diagnostics (schema + semantic + inventory), sorted and
    /// de-duplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// Feature entries loaded.
    pub features: usize,
    /// Gap entries loaded.
    pub gaps: usize,
    /// Registry files walked (including files that failed to parse).
    pub files: usize,
    /// Source-derived inventory items the §3.5 gate extracted and
    /// cross-checked.
    pub inventory: usize,
}

impl CheckOutcome {
    /// Number of error-severity diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// Number of warning-severity diagnostics.
    #[must_use]
    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    /// True when no error-severity diagnostic was produced.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }
}

/// Load + validate the registry under `root` (the project directory that
/// contains `ledger/`). Repo-relative references inside the registry
/// (`spec` paths, witness fixtures) resolve against `root`.
///
/// The M1.3 inventory gate runs as part of the check (plan §3.5/§3.7:
/// "every inventory item mapped" is registry consistency): source
/// extraction runs first, then every item must be claimed by a feature's
/// `inventory` list or ignored in `ledger/inventory-ignore.toml`.
#[must_use]
pub fn run_check(root: &Path) -> CheckOutcome {
    let loaded = load_project_registry(root);
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());

    let extraction = extract_inventory(root);
    diagnostics.extend(extraction.diagnostics);
    let ignores = load_inventory_ignore(root);
    diagnostics.extend(ignores.diagnostics);
    diagnostics.extend(inventory_diagnostics(
        &extraction.items,
        &ignores.ignores,
        loaded.registry.features(),
    ));

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    CheckOutcome {
        diagnostics,
        features: loaded.registry.feature_count(),
        gaps: loaded.registry.gap_count(),
        files: loaded.registry.file_count(),
        inventory: extraction.items.len(),
    }
}

/// Render the full stdout report for a check run.
#[must_use]
pub fn format_outcome(outcome: &CheckOutcome) -> String {
    let mut lines: Vec<String> = outcome
        .diagnostics
        .iter()
        .map(ToString::to_string)
        .collect();
    if outcome.is_ok() {
        lines.push(format!(
            "ledger check OK: {} features, {} gaps, {} files, {} inventory items, 0 errors, {} warning(s)",
            outcome.features,
            outcome.gaps,
            outcome.files,
            outcome.inventory,
            outcome.warnings()
        ));
    } else {
        lines.push(format!(
            "ledger check FAILED: {} error(s), {} warning(s)",
            outcome.errors(),
            outcome.warnings()
        ));
        lines.push(format!(
            "registry: {} features, {} gaps, {} files, {} inventory items",
            outcome.features, outcome.gaps, outcome.files, outcome.inventory
        ));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Result of a full `ledger inventory` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryOutcome {
    /// Inventory gate diagnostics (stale/duplicate/unmapped), sorted.
    pub diagnostics: Vec<Diagnostic>,
    /// Extracted items, sorted by `(extractor, key)`.
    pub items: Vec<ledger_schema::InventoryItem>,
    /// Keys with neither a claim nor an ignore (subset of `items`).
    pub unmapped_keys: Vec<String>,
    /// Ignore entries loaded from `ledger/inventory-ignore.toml`.
    pub ignores: usize,
    /// Feature entries loaded from the registry.
    pub features: usize,
}

impl InventoryOutcome {
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

/// Run the plan §3.5 inventory extraction + gate without the full
/// registry validation: extract from `root`'s sources, load the ignore
/// file, load the registry for claim resolution, and report every
/// violation. `ledger check` runs this same gate as part of its pass.
#[must_use]
pub fn run_inventory(root: &Path) -> InventoryOutcome {
    let loaded = load_project_registry(root);
    let extraction = extract_inventory(root);
    let ignores = load_inventory_ignore(root);
    let mut diagnostics = Vec::new();
    diagnostics.extend(loaded.diagnostics);
    diagnostics.extend(extraction.diagnostics);
    diagnostics.extend(ignores.diagnostics);
    diagnostics.extend(inventory_diagnostics(
        &extraction.items,
        &ignores.ignores,
        loaded.registry.features(),
    ));
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    let unmapped_keys = unmapped_item_keys(
        &extraction.items,
        &ignores.ignores,
        loaded.registry.features(),
    );
    InventoryOutcome {
        diagnostics,
        items: extraction.items,
        unmapped_keys,
        ignores: ignores.ignores.len(),
        features: loaded.registry.feature_count(),
    }
}

/// Render the full stdout report for an inventory run: a summary line,
/// every item (unmapped ones flagged), then the diagnostics.
#[must_use]
pub fn format_inventory(outcome: &InventoryOutcome) -> String {
    let plural = if outcome.items.len() == 1 {
        "item"
    } else {
        "items"
    };
    let mut lines = vec![format!(
        "inventory: {} {} ({} unmapped), {} ignore entries, {} features",
        outcome.items.len(),
        plural,
        outcome.unmapped_keys.len(),
        outcome.ignores,
        outcome.features
    )];
    for item in &outcome.items {
        let flag = if outcome.unmapped_keys.iter().any(|key| key == &item.key) {
            " UNMAPPED"
        } else {
            ""
        };
        lines.push(format!(
            "  {:<12} {:<44} {}{flag}",
            item.extractor.as_str(),
            item.key,
            item.source
        ));
    }
    for diagnostic in &outcome.diagnostics {
        lines.push(diagnostic.to_string());
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Result of a `ledger gaps` run (plan §3.3, M1.7): the loaded gap
/// registry with its witness coverage, plus any load/validation
/// diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapOutcome {
    /// Load + validation diagnostics, sorted and de-duplicated.
    pub diagnostics: Vec<Diagnostic>,
    /// Every gap entry with the file it was loaded from, sorted by ID.
    pub gaps: Vec<Sourced<GapEntry>>,
    /// Registry IDs whose `status` is `unwitnessed` — the explicit
    /// witness-debt list (plan §3.3: allowed, but never silent).
    pub unwitnessed: Vec<String>,
}

impl GapOutcome {
    /// Number of error-severity diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// True when no error-severity diagnostic was produced. Witness debt
    /// is a *warning*, so a registry with debt still runs ok — the report
    /// exists to render the debt, not to fail it (`check` is the gate).
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }
}

/// Run the M1.7 gap-registry report: load the registry under `root`
/// (the project directory that contains `ledger/`), validate it, and
/// summarise witness coverage. This is a *report*, not the `check` gate:
/// witness debt is expected and rendered, not failed.
#[must_use]
pub fn run_gaps(root: &Path) -> GapOutcome {
    let span = tracing::info_span!("gaps_report", root = %root.display());
    let _enter = span.enter();

    let loaded = load_project_registry(root);
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());

    let mut gaps: Vec<Sourced<GapEntry>> = loaded.registry.gaps().to_vec();
    gaps.sort_by(|a, b| a.id().cmp(b.id()));
    let unwitnessed: Vec<String> = gaps
        .iter()
        .filter(|g| matches!(g.entry.status, GapStatus::Unwitnessed))
        .map(|g| g.id().to_string())
        .collect();

    tracing::debug!(
        gaps = gaps.len(),
        unwitnessed = unwitnessed.len(),
        diagnostics = diagnostics.len(),
        "gap registry loaded for report"
    );

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    GapOutcome {
        diagnostics,
        gaps,
        unwitnessed,
    }
}

/// Render the stdout report for a gaps run: a coverage summary line, one
/// line per gap (class + status + title, with the witness path indented
/// underneath), the witness-debt reminder, then any diagnostics.
#[must_use]
pub fn format_gap_report(outcome: &GapOutcome) -> String {
    let witnessed = outcome.gaps.len() - outcome.unwitnessed.len();
    let mut lines = vec![format!(
        "gaps: {} registered, {} witnessed, {} unwitnessed (witness debt)",
        outcome.gaps.len(),
        witnessed,
        outcome.unwitnessed.len(),
    )];
    for gap in &outcome.gaps {
        let status = match &gap.entry.status {
            GapStatus::Witnessed => "witnessed",
            GapStatus::Unwitnessed => "debt",
        };
        lines.push(format!(
            "  {:<8} [{}] {:<9} {}",
            gap.id(),
            gap.entry.class.as_str(),
            status,
            gap.entry.title,
        ));
        if let Some(witness) = &gap.entry.witness {
            lines.push(format!("          witness: {witness}"));
        }
    }
    if !outcome.unwitnessed.is_empty() {
        lines.push(format!(
            "witness debt: {} gap(s) without a strict-xfail witness (plan §3.3); \
             per-entry reasons live in ledger/gaps/known/*.toml",
            outcome.unwitnessed.len()
        ));
    }
    for diagnostic in &outcome.diagnostics {
        lines.push(diagnostic.to_string());
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Write the combined JSON-Schema export to `out` (pretty-printed).
///
/// # Errors
///
/// Returns a typed [`LedgerCliError`] when serialization or the file write
/// fails.
pub fn write_schema(out: &Path) -> Result<String, LedgerCliError> {
    let document = registry_schema();
    let text =
        serde_json::to_string_pretty(&document).map_err(|err| LedgerCliError::SchemaSerialize {
            detail: err.to_string(),
        })? + "\n";
    fs::write(out, text).map_err(|err| LedgerCliError::SchemaWrite {
        path: out.display().to_string(),
        detail: err.to_string(),
    })?;
    Ok(format!("wrote ledger JSON schema to {}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_schema::CODE_DUP_FEATURE;
    use std::fmt::Write as _;

    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items.iter().map(|s| (*s).to_string())
    }

    // -- argument parsing ----------------------------------------------------

    #[test]
    fn empty_args_mean_check_with_default_root() {
        let cmd = parse_args(args(&[])).expect("parse");
        assert_eq!(
            cmd,
            Command::Check {
                root: PathBuf::from(".")
            }
        );
    }

    #[test]
    fn check_takes_optional_root() {
        let cmd = parse_args(args(&["check", "/tmp/reg"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Check {
                root: PathBuf::from("/tmp/reg")
            }
        );
    }

    #[test]
    fn inventory_takes_optional_root() {
        let cmd = parse_args(args(&["inventory", "/tmp/src"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Inventory {
                root: PathBuf::from("/tmp/src")
            }
        );
    }

    #[test]
    fn inventory_defaults_to_current_dir() {
        let cmd = parse_args(args(&["inventory"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Inventory {
                root: PathBuf::from(".")
            }
        );
    }

    #[test]
    fn inventory_rejects_extra_positionals() {
        let err = parse_args(args(&["inventory", "a", "b"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::ExtraArgument { ref arg, .. } if arg == "b"),
            "{err}"
        );
    }

    #[test]
    fn schema_defaults_to_stdout() {
        let cmd = parse_args(args(&["schema"])).expect("parse");
        assert_eq!(cmd, Command::Schema { out: None });
    }

    #[test]
    fn schema_takes_optional_output_path() {
        let cmd = parse_args(args(&["schema", "out.json"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Schema {
                out: Some(PathBuf::from("out.json"))
            }
        );
    }

    #[test]
    fn compute_defaults_to_text_format_over_the_project_root() {
        let cmd = parse_args(args(&["compute"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Compute {
                root: PathBuf::from("."),
                evidence: None,
                format: ComputeFormat::Text,
                snapshot: None,
            }
        );
    }

    #[test]
    fn compute_accepts_root_evidence_and_format_in_any_order() {
        let cmd = parse_args(args(&[
            "compute",
            "/tmp/reg",
            "--format",
            "json",
            "--evidence",
            "run.jsonl",
        ]))
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Compute {
                root: PathBuf::from("/tmp/reg"),
                evidence: Some(PathBuf::from("run.jsonl")),
                format: ComputeFormat::Json,
                snapshot: None,
            }
        );
    }

    #[test]
    fn compute_accepts_a_snapshot_output_path() {
        let cmd = parse_args(args(&["compute", "--snapshot", "out/snapshot.json"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Compute {
                root: PathBuf::from("."),
                evidence: None,
                format: ComputeFormat::Text,
                snapshot: Some(PathBuf::from("out/snapshot.json")),
            }
        );
    }

    #[test]
    fn diff_requires_two_snapshot_paths_and_defaults_the_root() {
        let err = parse_args(args(&["diff", "only-one.json"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::DiffMissingPath { .. }),
            "{err}"
        );
        let cmd = parse_args(args(&["diff", "base.json", "head.json"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Diff {
                baseline: PathBuf::from("base.json"),
                current: PathBuf::from("head.json"),
                root: PathBuf::from("."),
                format: DiffFormat::Text,
            }
        );
        let cmd = parse_args(args(&[
            "diff",
            "base.json",
            "head.json",
            "/tmp/reg",
            "--format",
            "json",
        ]))
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Diff {
                baseline: PathBuf::from("base.json"),
                current: PathBuf::from("head.json"),
                root: PathBuf::from("/tmp/reg"),
                format: DiffFormat::Json,
            }
        );
        let err = parse_args(args(&[
            "diff",
            "base.json",
            "head.json",
            "--format",
            "yaml",
        ]))
        .expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::DiffBadFormat { ref value, .. } if value == "yaml"),
            "{err}"
        );
    }

    #[test]
    fn compute_rejects_unknown_format_values() {
        let err = parse_args(args(&["compute", "--format", "yaml"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::ComputeBadFormat { ref value, .. } if value == "yaml"),
            "{err}"
        );
        assert!(err.to_string().contains("--format expects text or json"));
    }

    #[test]
    fn compute_rejects_flags_without_values() {
        for flag in ["--evidence", "--format"] {
            let err = parse_args(args(&["compute", flag])).expect_err("reject");
            assert!(
                matches!(err, LedgerCliError::ComputeMissingValue { flag: ref bound, .. } if *bound == flag),
                "{err}"
            );
        }
    }

    #[test]
    fn compute_rejects_extra_positionals() {
        let err = parse_args(args(&["compute", "a", "b"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::ExtraArgument { ref arg, .. } if arg == "b"),
            "{err}"
        );
    }

    #[test]
    fn compute_rejects_unknown_flags() {
        let err = parse_args(args(&["compute", "--json"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::UnknownFlag { ref flag, .. } if flag == "--json"),
            "{err}"
        );
    }

    #[test]
    fn status_defaults_to_markdown_over_the_project_root() {
        let cmd = parse_args(args(&["status"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Status {
                root: PathBuf::from("."),
                evidence: None,
                format: StatusFormat::Markdown,
            }
        );
    }

    #[test]
    fn status_accepts_root_evidence_and_format_in_any_order() {
        let cmd = parse_args(args(&[
            "status",
            "--format",
            "json",
            "s",
            "--evidence",
            "e.jsonl",
        ]))
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Status {
                root: PathBuf::from("s"),
                evidence: Some(PathBuf::from("e.jsonl")),
                format: StatusFormat::Json,
            }
        );
    }

    #[test]
    fn status_rejects_unknown_format_values() {
        let err = parse_args(args(&["status", "--format", "html"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::StatusBadFormat { ref value, .. } if value == "html"),
            "{err}"
        );
        assert!(err.to_string().contains("--format expects md or json"));
    }

    #[test]
    fn status_rejects_flags_without_values() {
        for flag in ["--evidence", "--format"] {
            let err = parse_args(args(&["status", flag])).expect_err("reject");
            assert!(
                matches!(err, LedgerCliError::StatusMissingValue { flag: ref bound, .. } if bound == flag),
                "{err}"
            );
        }
    }

    #[test]
    fn status_rejects_unknown_flags() {
        let err = parse_args(args(&["status", "--md"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::UnknownFlag { ref flag, .. } if flag == "--md"),
            "{err}"
        );
    }

    #[test]
    fn gates_defaults_to_text_over_the_project_root() {
        let cmd = parse_args(args(&["gates"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Gates {
                root: PathBuf::from("."),
                evidence: None,
                format: GatesFormat::Text,
            }
        );
    }

    #[test]
    fn gates_accepts_root_evidence_and_format_in_any_order() {
        let cmd = parse_args(args(&[
            "gates",
            "--format",
            "json",
            "s",
            "--evidence",
            "e.jsonl",
        ]))
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Gates {
                root: PathBuf::from("s"),
                evidence: Some(PathBuf::from("e.jsonl")),
                format: GatesFormat::Json,
            }
        );
    }

    #[test]
    fn gates_rejects_unknown_format_values() {
        let err = parse_args(args(&["gates", "--format", "yaml"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::GatesBadFormat { ref value, .. } if value == "yaml"),
            "{err}"
        );
        assert!(err.to_string().contains("--format expects text or json"));
    }

    #[test]
    fn gates_rejects_flags_without_values() {
        for flag in ["--evidence", "--format"] {
            let err = parse_args(args(&["gates", flag])).expect_err("reject");
            assert!(
                matches!(err, LedgerCliError::GatesMissingValue { flag: ref bound, .. } if bound == flag),
                "{err}"
            );
        }
    }

    #[test]
    fn gates_rejects_unknown_flags() {
        let err = parse_args(args(&["gates", "--json"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::UnknownFlag { ref flag, .. } if flag == "--json"),
            "{err}"
        );
    }

    #[test]
    fn verify_defaults_to_text_over_the_project_root() {
        let cmd = parse_args(args(&["verify"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Verify {
                root: PathBuf::from("."),
                evidence: None,
                format: VerifyFormat::Text,
            }
        );
    }

    #[test]
    fn verify_accepts_root_evidence_and_format_in_any_order() {
        let cmd = parse_args(args(&[
            "verify",
            "--evidence",
            "e.jsonl",
            "r",
            "--format",
            "json",
        ]))
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Verify {
                root: PathBuf::from("r"),
                evidence: Some(PathBuf::from("e.jsonl")),
                format: VerifyFormat::Json,
            }
        );
    }

    #[test]
    fn verify_rejects_unknown_format_values() {
        let err = parse_args(args(&["verify", "--format", "yaml"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::VerifyBadFormat { ref value, .. } if value == "yaml"),
            "{err}"
        );
        assert!(err.to_string().contains("--format expects text or json"));
    }

    #[test]
    fn verify_rejects_flags_without_values() {
        for flag in ["--evidence", "--format"] {
            let err = parse_args(args(&["verify", flag])).expect_err("reject");
            assert!(
                matches!(err, LedgerCliError::VerifyMissingValue { flag: ref bound, .. } if bound == flag),
                "{err}"
            );
        }
    }

    #[test]
    fn verify_rejects_unknown_flags() {
        let err = parse_args(args(&["verify", "--json"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::UnknownFlag { ref flag, .. } if flag == "--json"),
            "{err}"
        );
    }

    #[test]
    fn unknown_subcommand_is_rejected_with_usage() {
        let err = parse_args(args(&["frobnicate"])).expect_err("reject");
        assert!(matches!(
            err,
            LedgerCliError::UnknownSubcommand { ref subcommand, .. } if subcommand == "frobnicate"
        ));
        assert!(err.to_string().contains("usage:"), "{err}");
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let err = parse_args(args(&["--wat"])).expect_err("reject");
        assert!(
            matches!(err, LedgerCliError::UnknownFlag { ref flag, .. } if flag == "--wat"),
            "error names the flag: {err}"
        );
    }

    #[test]
    fn help_flag_is_a_typed_variant_with_usage() {
        let err = parse_args(args(&["-h"])).expect_err("help exits via usage");
        assert!(matches!(err, LedgerCliError::HelpRequested { .. }));
        assert!(err.to_string().contains("usage:"));
    }

    #[test]
    fn every_cli_error_variant_has_help_text() {
        let errors = [
            LedgerCliError::UnknownFlag {
                flag: "--wat".into(),
                usage: usage(),
            },
            LedgerCliError::UnknownSubcommand {
                subcommand: "frob".into(),
                usage: usage(),
            },
            LedgerCliError::ExtraArgument {
                arg: "b".into(),
                usage: usage(),
            },
            LedgerCliError::SchemaSerialize {
                detail: "boom".into(),
            },
            LedgerCliError::SchemaWrite {
                path: "out.json".into(),
                detail: "denied".into(),
            },
            LedgerCliError::NextBadCount {
                value: "many".into(),
                usage: usage(),
            },
            LedgerCliError::NextBadLane {
                lane: "X".into(),
                usage: usage(),
            },
            LedgerCliError::NextMissingValue {
                flag: "--lane".into(),
                usage: usage(),
            },
            LedgerCliError::ExplainMissingId { usage: usage() },
            LedgerCliError::GatesBadFormat {
                value: "yaml".into(),
                usage: usage(),
            },
            LedgerCliError::GatesMissingValue {
                flag: "--evidence".into(),
                usage: usage(),
            },
            LedgerCliError::VerifyBadFormat {
                value: "yaml".into(),
                usage: usage(),
            },
            LedgerCliError::VerifyMissingValue {
                flag: "--format".into(),
                usage: usage(),
            },
        ];
        for err in errors {
            let help =
                miette::Diagnostic::help(&err).expect("every failure variant carries help text");
            assert!(
                !help.to_string().is_empty(),
                "{err:?} help must be non-empty"
            );
        }
        // The help *request* is not a failure: it deliberately has no help.
        let requested = LedgerCliError::HelpRequested { usage: usage() };
        assert!(miette::Diagnostic::help(&requested).is_none());
    }

    #[test]
    fn extra_positional_is_rejected() {
        let err = parse_args(args(&["check", "a", "b"])).expect_err("reject");
        assert!(matches!(err, LedgerCliError::ExtraArgument { .. }), "{err}");
    }

    // -- check run -----------------------------------------------------------

    fn write_file(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, contents).expect("write");
    }

    fn feature_toml(id: &str) -> String {
        format!(
            "[[feature]]\n\
             id = \"{id}\"\n\
             title = \"t {id}\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n"
        )
    }

    /// Write the canonical spec file feature `spec` fields resolve against
    /// (M1.2 spec-path validation).
    fn write_spec(root: &Path) {
        write_file(&root.join("docs/specs"), "v0.1.0.md", "# spec fixture\n");
    }

    fn gap_toml(id: &str, affects: &str) -> String {
        format!(
            "[[gap]]\n\
             id = \"{id}\"\n\
             class = \"a\"\n\
             title = \"g {id}\"\n\
             affects = [\"{affects}\"]\n\
             caps = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status = \"unwitnessed\"\n"
        )
    }

    #[test]
    fn check_passes_on_a_valid_registry_with_witness_debt() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        write_spec(dir.path());
        write_file(&ledger, "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(&ledger, "gaps/x.toml", &gap_toml("GAP-1", "LANG-A"));
        let outcome = run_check(dir.path());
        assert!(
            outcome.is_ok(),
            "valid registry must pass: {:?}",
            outcome.diagnostics
        );
        assert_eq!(outcome.features, 1);
        assert_eq!(outcome.gaps, 1);
        assert_eq!(outcome.files, 2);
        assert_eq!(outcome.warnings(), 1, "one witness-debt warning");
    }

    #[test]
    fn check_fails_on_duplicate_feature_ids() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        write_spec(dir.path());
        write_file(&ledger, "features/lang/a.toml", &feature_toml("LANG-DUP"));
        write_file(&ledger, "features/lang/b.toml", &feature_toml("LANG-DUP"));
        let outcome = run_check(dir.path());
        assert_eq!(outcome.errors(), 1);
        assert_eq!(outcome.diagnostics[0].code, CODE_DUP_FEATURE);
    }

    #[test]
    fn check_fails_loudly_without_a_ledger_directory() {
        let dir = tempfile::tempdir().expect("tmp");
        let outcome = run_check(dir.path());
        assert!(!outcome.is_ok(), "missing registry must fail");
        assert_eq!(outcome.files, 0);
    }

    #[test]
    fn check_reports_missing_spec_paths() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        // No spec file under this root: the feature's `spec` reference
        // cannot resolve, which must surface as an error diagnostic.
        write_file(&ledger, "features/lang/a.toml", &feature_toml("LANG-A"));
        let outcome = run_check(dir.path());
        assert!(!outcome.is_ok(), "missing spec must fail the check");
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == ledger_schema::CODE_SPEC_MISSING)
        );
    }

    #[test]
    fn check_reports_unknown_taxonomy_areas() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        write_spec(dir.path());
        write_file(
            &ledger,
            "features/lang/a.toml",
            &feature_toml("LANG-A").replace("area = \"lang.control-flow\"", "area = \"lang.wat\""),
        );
        let outcome = run_check(dir.path());
        assert!(!outcome.is_ok(), "unknown area must fail the check");
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == ledger_schema::CODE_AREA_UNKNOWN)
        );
    }

    // -- inventory run -------------------------------------------------------

    /// A minimal `#[unsafe(no_mangle)]` FFI source tree fixture.
    fn write_runtime_symbol(root: &Path, symbol: &str) {
        write_file(
            &root.join("crates/glyim-runtime/src"),
            "lib.rs",
            &format!(
                "#[unsafe(no_mangle)]\npub extern \"C\" fn {symbol}(size: usize) -> usize {{ size }}\n"
            ),
        );
    }

    /// Minimal claim-bearing feature entry (area/tier/spec fields are
    /// taxonomy-shaped so a future `validate()` pass would also accept it).
    fn feature_toml_with_claims(id: &str, claims: &[&str]) -> String {
        let list = claims
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "[[feature]]\n\
             id = \"{id}\"\n\
             title = \"t {id}\"\n\
             area = \"runtime.ffi\"\n\
             tier = \"std\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L4\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             inventory = [{list}]\n\
             owner_lane = \"B\"\n"
        )
    }

    #[test]
    fn run_inventory_fails_on_unregistered_symbol() {
        let dir = tempfile::tempdir().expect("tmp");
        write_runtime_symbol(dir.path(), "glyim_orphan");
        let outcome = run_inventory(dir.path());
        assert!(
            !outcome.is_ok(),
            "unregistered symbol must fail: {:?}",
            outcome.diagnostics
        );
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(outcome.unmapped_keys, vec!["runtime.ffi.glyim_orphan"]);
        assert_eq!(outcome.ignores, 0);
    }

    #[test]
    fn run_inventory_passes_when_a_feature_claims_the_symbol() {
        let dir = tempfile::tempdir().expect("tmp");
        write_runtime_symbol(dir.path(), "glyim_claimed");
        write_file(
            &dir.path().join("ledger/features/runtime"),
            "ffi.toml",
            &feature_toml_with_claims("RUNTIME-FFI-CLAIMED", &["runtime.ffi.glyim_claimed"]),
        );
        let outcome = run_inventory(dir.path());
        assert!(
            outcome.is_ok(),
            "claimed symbol must pass: {:?}",
            outcome.diagnostics
        );
        assert_eq!(outcome.features, 1);
        assert!(outcome.unmapped_keys.is_empty());
    }

    #[test]
    fn format_inventory_renders_summary_items_and_diagnostics() {
        let outcome = InventoryOutcome {
            diagnostics: vec![Diagnostic {
                code: ledger_schema::CODE_INVENTORY_UNMAPPED,
                message: "inventory item `runtime.ffi.glyim_orphan` (runtime-ffi, src/lib.rs) is \
                          neither claimed by a feature's `inventory` list nor ignored"
                    .into(),
                severity: Severity::Error,
            }],
            items: vec![ledger_schema::InventoryItem {
                extractor: ledger_schema::InventoryExtractor::RuntimeFfi,
                key: "runtime.ffi.glyim_orphan".into(),
                source: "src/lib.rs".into(),
            }],
            unmapped_keys: vec!["runtime.ffi.glyim_orphan".into()],
            ignores: 0,
            features: 0,
        };
        let text = format_inventory(&outcome);
        assert!(text.contains("inventory: 1 item (1 unmapped)"), "{text}");
        assert!(text.contains("runtime.ffi.glyim_orphan"), "{text}");
        assert!(text.contains("UNMAPPED"), "{text}");
        assert!(text.contains("error[LEDGER-INVENTORY-UNMAPPED]"), "{text}");
    }

    // -- the M1.3 DoD: unregistered MIR variants fail `ledger check` ---------

    fn write_mir_enum(root: &Path, variants: &[&str]) {
        let mut body = String::new();
        for variant in variants {
            let _ = writeln!(body, "    {variant},");
        }
        write_file(
            &root.join("crates/glyim-mir/src"),
            "lib.rs",
            &format!("pub enum StatementKind {{\n{body}}}\n"),
        );
    }

    fn write_ignore(root: &Path, items: &[&str]) {
        let mut body = String::new();
        for item in items {
            let _ = writeln!(
                body,
                "[[ignore]]\nitem = \"{item}\"\nreason = \"seed: no per-variant feature yet\"\n"
            );
        }
        write_file(&root.join("ledger"), "inventory-ignore.toml", &body);
    }

    #[test]
    fn check_passes_with_registered_mir_variants() {
        let dir = tempfile::tempdir().expect("tmp");
        write_spec(dir.path());
        write_file(
            &dir.path().join("ledger/features/lang"),
            "a.toml",
            &feature_toml("LANG-A"),
        );
        write_mir_enum(dir.path(), &["Assign", "Nop"]);
        write_ignore(
            dir.path(),
            &["mir.StatementKind.Assign", "mir.StatementKind.Nop"],
        );
        let outcome = run_check(dir.path());
        assert!(
            outcome.is_ok(),
            "fully registered inventory must pass: {:?}",
            outcome.diagnostics
        );
        assert_eq!(outcome.inventory, 2);
    }

    #[test]
    fn check_fails_when_a_mir_variant_is_added_without_registration() {
        let dir = tempfile::tempdir().expect("tmp");
        write_spec(dir.path());
        write_file(
            &dir.path().join("ledger/features/lang"),
            "a.toml",
            &feature_toml("LANG-A"),
        );
        write_mir_enum(dir.path(), &["Assign", "Nop", "Ghost"]);
        write_ignore(
            dir.path(),
            &["mir.StatementKind.Assign", "mir.StatementKind.Nop"],
        );
        let outcome = run_check(dir.path());
        assert!(
            !outcome.is_ok(),
            "M1.3 DoD: the new variant must fail the gate"
        );
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == ledger_schema::CODE_INVENTORY_UNMAPPED
                    && d.message.contains("mir.StatementKind.Ghost")),
            "diagnostic names the unregistered variant: {:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn check_fails_on_stale_claims() {
        let dir = tempfile::tempdir().expect("tmp");
        write_spec(dir.path());
        write_file(
            &dir.path().join("ledger/features/runtime"),
            "ffi.toml",
            &feature_toml_with_claims("RUNTIME-FFI-GHOST", &["runtime.ffi.glyim_ghost"]),
        );
        let outcome = run_check(dir.path());
        assert!(!outcome.is_ok(), "stale claim must fail");
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == ledger_schema::CODE_INVENTORY_STALE_CLAIM),
            "{:?}",
            outcome.diagnostics
        );
    }

    // -- gaps run (M1.7) -----------------------------------------------------

    #[test]
    fn gaps_takes_optional_root() {
        let cmd = parse_args(args(&["gaps", "/tmp/reg"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Gaps {
                root: PathBuf::from("/tmp/reg")
            }
        );
    }

    #[test]
    fn gaps_defaults_to_current_dir() {
        let cmd = parse_args(args(&["gaps"])).expect("parse");
        assert_eq!(
            cmd,
            Command::Gaps {
                root: PathBuf::from(".")
            }
        );
    }

    #[test]
    fn usage_lists_every_subcommand() {
        // The usage text is the only discoverability surface for `gaps`;
        // drift here is how a subcommand goes missing.
        let text = usage();
        for sub in ["check", "gaps", "schema", "inventory"] {
            assert!(text.contains(sub), "usage must list `{sub}`: {text}");
        }
    }

    /// A witnessed `[[gap]]` entry pointing at a witness fixture that
    /// carries the `//@ known-gap:` marker (the strict-xfail contract).
    fn witnessed_gap_toml(id: &str, affects: &str, witness: &str) -> String {
        format!(
            "[[gap]]\n\
             id = \"{id}\"\n\
             class = \"c\"\n\
             title = \"g {id}\"\n\
             affects = [\"{affects}\"]\n\
             caps = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status = \"witnessed\"\n\
             witness = \"{witness}\"\n"
        )
    }

    #[test]
    fn run_gaps_reports_witness_coverage_and_debt() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        write_spec(dir.path());
        write_file(&ledger, "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(
            &ledger,
            "gaps/two.toml",
            &format!(
                "{}\n{}",
                witnessed_gap_toml("GAP-0001", "LANG-A", "witness-1.g"),
                gap_toml("GAP-0002", "LANG-A")
            ),
        );
        write_file(
            dir.path(),
            "witness-1.g",
            "//@ known-gap: GAP-0001\nfn main() {}\n",
        );

        let outcome = run_gaps(dir.path());
        assert!(
            outcome.is_ok(),
            "valid registry must pass: {:?}",
            outcome.diagnostics
        );
        assert_eq!(outcome.gaps.len(), 2);
        assert_eq!(outcome.unwitnessed, vec!["GAP-0002".to_string()]);
        // Entries render in ID order regardless of file order.
        assert_eq!(outcome.gaps[0].id(), "GAP-0001");
        assert_eq!(outcome.gaps[1].id(), "GAP-0002");
    }

    #[test]
    fn run_gaps_tolerates_a_missing_registry() {
        // A report, not a gate: no ledger dir yields an empty report with
        // the load diagnostic rendered, not a panic.
        let dir = tempfile::tempdir().expect("tmp");
        let outcome = run_gaps(dir.path());
        assert!(!outcome.is_ok());
        assert!(outcome.gaps.is_empty());
    }

    #[test]
    fn format_gap_report_renders_summary_entries_and_debt() {
        let dir = tempfile::tempdir().expect("tmp");
        let ledger = dir.path().join("ledger");
        write_spec(dir.path());
        write_file(&ledger, "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(
            &ledger,
            "gaps/two.toml",
            &format!(
                "{}\n{}",
                witnessed_gap_toml("GAP-0001", "LANG-A", "witness-1.g"),
                gap_toml("GAP-0002", "LANG-A")
            ),
        );
        write_file(
            dir.path(),
            "witness-1.g",
            "//@ known-gap: GAP-0001\nfn main() {}\n",
        );

        let text = format_gap_report(&run_gaps(dir.path()));
        assert!(
            text.contains("gaps: 2 registered, 1 witnessed, 1 unwitnessed"),
            "{text}"
        );
        assert!(text.contains("GAP-0001 [c] witnessed"), "{text}");
        assert!(text.contains("witness: witness-1.g"), "{text}");
        assert!(text.contains("GAP-0002 [a] debt"), "{text}");
        assert!(text.contains("witness debt: 1 gap(s)"), "{text}");
    }

    // -- rendering -----------------------------------------------------------

    #[test]
    fn format_outcome_renders_ok_summary() {
        let outcome = CheckOutcome {
            diagnostics: vec![],
            features: 6,
            gaps: 2,
            files: 8,
            inventory: 104,
        };
        let text = format_outcome(&outcome);
        assert_eq!(
            text,
            "ledger check OK: 6 features, 2 gaps, 8 files, 104 inventory items, 0 errors, 0 warning(s)\n"
        );
    }

    #[test]
    fn format_outcome_lists_diagnostics_then_failure_summary() {
        let outcome = CheckOutcome {
            diagnostics: vec![Diagnostic {
                code: CODE_DUP_FEATURE,
                message: "duplicate feature id LANG-DUP".into(),
                severity: Severity::Error,
            }],
            features: 2,
            gaps: 0,
            files: 2,
            inventory: 0,
        };
        let text = format_outcome(&outcome);
        assert!(text.contains("error[LEDGER-DUP-FEATURE]: duplicate feature id LANG-DUP"));
        assert!(text.contains("ledger check FAILED: 1 error(s), 0 warning(s)"));
        assert!(text.contains("2 features, 0 gaps, 2 files, 0 inventory items"));
    }

    // -- schema export -------------------------------------------------------

    #[test]
    fn write_schema_pretty_prints_to_a_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("ledger-schema.json");
        let summary = write_schema(&out).expect("write");
        assert!(summary.contains(&out.display().to_string()), "{summary}");
        let raw = std::fs::read_to_string(&out).expect("read back");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
        assert!(value.get("feature").is_some());
        assert!(value.get("gap").is_some());
        assert!(raw.contains("\n  \""), "pretty-printed, not single-line");
    }

    #[test]
    fn write_schema_reports_write_failures() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("no-such-dir").join("schema.json");
        assert!(write_schema(&out).is_err(), "missing parent dir must error");
    }
}
