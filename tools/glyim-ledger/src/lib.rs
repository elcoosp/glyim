//! CLI library for the Readiness Ledger tool (plan M1.1).
//!
//! Twenty-two subcommands:
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
//! - `glyim-ledger status [ROOT] [--evidence PATH] [--format md|json]
//!   [--out PATH] [--check]` — the plan §6 M1.9 renderers over the
//!   compute outcome, plus the M1.10 remainder: `--out PATH` writes the
//!   rendered document (default `STATUS.md` / `STATUS.json` by face) and
//!   `--check` byte-compares the committed document against the computed
//!   status ("STATUS is up to date").
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
//! - `glyim-ledger evidence record [--out PATH] [--format text|json]
//!   <SRC...>` — the plan §3.4 evidence-store curation face: validate
//!   harness-emitted `glyim.evidence/1` records against the registry and
//!   merge them into the committed store (whole-record dedupe, canonical
//!   order; default target `ledger/evidence/recorded.jsonl`), so the
//!   engine's `ledger/evidence/*.jsonl` discovery has a first-class
//!   writer (see [`evidence`]).
//! - `glyim-ledger evidence check [ROOT] [--format text|json]` — validate
//!   the committed evidence store against the registry and report its
//!   coverage; error diagnostics fail the exit code (see [`evidence`]).
//!   `glyim-ledger evidence audit [ROOT] [--format text|json]` is its
//!   filesystem counterpart: it recomputes every record's
//!   `fixture_sha256` through the [`crate::FixtureFingerprint`] seam and
//!   fails on missing, drifted, or unreadable fixtures — `check` proves
//!   what the store says is consistent, `audit` proves what it points at
//!   still exists.
//! - `glyim-ledger waivers [ROOT] [--format text|json]` — the
//!   constitution-C9 waiver-health report: every `ledger/waivers.toml`
//!   entry with its expiry verdict; expired waivers and load diagnostics
//!   fail the exit code (see [`waivers`]). `waivers grant [ROOT]
//!   --feature <ID> --reason <TEXT> --issue <URL> [--days N]` is the
//!   curated write face: it validates the request against the registry
//!   and the C9 horizon and appends the canonical `[[waiver]]` block
//!   (deterministic ID, never a silent overwrite). `waivers revoke
//!   [ROOT] --id <WVR-ID>` is the retirement write face: it removes
//!   exactly the one matching block and rewrites the remaining entries
//!   canonically through the [`crate::WaiverRewriter`] seam (nothing is
//!   written when the document does not load cleanly). `waivers extend
//!   [ROOT] --id <WVR-ID> [--days N]` is the re-pin write face: it moves
//!   exactly the one matching block's expiry to `today + days` (default
//!   7, horizon 14) through the [`crate::WaiverExtender`] seam, keeping
//!   the entry's identity, reason, and issue — the sanctioned counterpart
//!   the duplicate-grant help line points to (nothing is written when the
//!   document does not load cleanly). `waivers prune [ROOT]` is the
//!   batch-retirement write face: it removes every expired block and
//!   rewrites the remaining entries canonically through the
//!   [`crate::WaiverPruner`] seam (a non-clean document is refused; a
//!   clean document is never rewritten).
//! - `glyim-ledger handoff [ROOT] [--items N] [--format md|json]
//!   [--out PATH] [--check]` — the plan §3.7 session-handoff generator:
//!   it composes the compute, `next`, and `waivers` face outcomes into
//!   one short document (readiness histogram, top-N work queue, waiver
//!   health, witness debt) as a pure function of the state;
//!   `--items N` caps the queue slice, `--out PATH` writes the rendered
//!   document (default `HANDOFF.md` / `HANDOFF.json` by face) and
//!   `--check` byte-compares the committed document against the render
//!   (see [`handoff`]).
//! - `glyim-ledger release-check <VERSION> [ROOT] [--format text|json]` —
//!   the plan §10 final release gate: it composes the M1.12 gate report,
//!   the M1.13 determinism self-check, and the C9 waiver health into one
//!   verdict for the named release (every declared gate green —
//!   `unmeasured` blocks, being displayed, never silently passed —
//!   evidence reproducible, no waiver active or expired, and no error
//!   diagnostic across the composed faces); see [`release`].
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
pub mod evidence;
pub mod explain;
pub mod gaps;
pub mod gates;
pub mod handoff;
pub mod next;
pub mod release;
pub mod render;
pub mod status;
pub mod verify;
pub mod waivers;

pub use compute::{
    CODE_EVIDENCE_INVALID, CODE_EVIDENCE_IO, CODE_EVIDENCE_SCHEMA, CODE_EVIDENCE_UNKNOWN_FEATURE,
    COMPUTE_SCHEMA, ComputeOutcome, EVIDENCE_WIRE_SCHEMA, EvidenceBatch, EvidenceOutcome,
    EvidenceSource, FeatureLevel, HARDENING_FUZZ_HOURS, HARDENING_MUTATION_RATE,
    InMemoryEvidenceSource, JsonlEvidenceSource, LedgerEvidence, ProjectEvidenceSource,
    compute_levels, format_compute_json, format_compute_report, run_compute,
};
pub use diff::DEFAULT_WAIVER_DAYS;
pub use diff::{
    CODE_SNAPSHOT_IO, CODE_SNAPSHOT_SCHEMA, CODE_WAIVER_DUP, CODE_WAIVER_EXPIRED,
    CODE_WAIVER_HORIZON, CODE_WAIVER_SCHEMA, CODE_WAIVER_UNUSED, ComputeSnapshot, DiffDocument,
    FeatureLevelDiff, FileSnapshotStore, InMemorySnapshotStore, LEDGER_DIFF_SCHEMA,
    LedgerDiffOutcome, LevelVerdict, NaiveDate, SNAPSHOT_SCHEMA, SnapshotBatch, SnapshotEntry,
    SnapshotStore, WAIVER_MAX_AGE_DAYS, WaiverEntry, WaiverSet, diff_levels, execute_diff,
    format_diff_json, format_diff_text, load_waivers, run_diff, today_iso,
};
pub use evidence::{
    CODE_EVIDENCE_STORE_IO, CheckReport, DEFAULT_RECORD_TARGET, EVIDENCE_AUDIT_SCHEMA,
    EVIDENCE_CHECK_SCHEMA, EVIDENCE_RECORD_SCHEMA, EvidenceAuditOutcome, EvidenceStore,
    FileEvidenceStore, FileFixtureFingerprint, FingerprintError, FixtureFingerprint, FixtureRow,
    FixtureVerdict, InMemoryEvidenceStore, InMemoryFixtureFingerprint, OutcomeHistogram,
    RecordOutcome, RecordStats, audit_records, canonical_line, format_evidence_audit_json,
    format_evidence_audit_text, format_evidence_check_json, format_evidence_check_text,
    format_evidence_record_json, format_evidence_record_text, merge_records, resolve_fixture,
    run_evidence_audit, run_evidence_check, run_evidence_record,
};
pub use explain::{ExplainEntry, ExplainOutcome, format_explain_report, run_explain};
pub use gaps::{
    FileGapRetirer, FileGapWitnessRewriter, GAPS_RETIRE_SCHEMA, GAPS_WITNESS_SCHEMA, GapRetirer,
    GapWitnessRewriter, GapsFormat, GapsRetireOutcome, GapsRetireRequest, GapsWitnessOutcome,
    GapsWitnessRequest, InMemoryGapRetirer, InMemoryGapWitnessRewriter, format_gaps_retire_json,
    format_gaps_retire_text, format_gaps_witness_json, format_gaps_witness_text, run_gaps_retire,
    run_gaps_witness,
};
pub use gates::{
    GATES_REPORT_SCHEMA, GateVerdict, GateVerdictRow, GatesOutcome, evaluate_gates,
    format_gates_json, format_gates_text, run_gates,
};
pub use handoff::{
    FileHandoffSource, HANDOFF_DEFAULT_ITEMS, HANDOFF_JSON_TARGET, HANDOFF_MAX_ITEMS,
    HANDOFF_MD_END, HANDOFF_MD_START, HANDOFF_MD_TARGET, HANDOFF_SCHEMA, HandoffDocument,
    HandoffExpiry, HandoffFormat, HandoffItem, HandoffLevels, HandoffOutcome, HandoffRenderer,
    HandoffSource, HandoffState, InMemoryHandoffSource, JsonHandoffRenderer, TextHandoffRenderer,
    format_handoff_json, format_handoff_text, render_handoff, run_handoff, run_handoff_out,
};
pub use next::{
    FeatureSource, GapSource, NEXT_SCHEMA, NextOutcome, WorkItem, WorkItemSource, WorkKind,
    build_queue, feature_depths, filter, format_next_json, format_next_report, parse_lane,
    run_next,
};
pub use release::{
    FileReleaseCheckSource, InMemoryReleaseCheckSource, JsonReleaseCheckRenderer,
    RELEASE_CHECK_SCHEMA, ReleaseCheckDocument, ReleaseCheckOutcome, ReleaseCheckRenderer,
    ReleaseCheckSource, ReleaseCheckState, ReleaseVersion, TextReleaseCheckRenderer,
    format_release_check_json, format_release_check_text, render_release_check, run_release_check,
};
pub use render::{
    JsonGapRenderer, KNOWN_GAPS_MD_END, KNOWN_GAPS_MD_START, KNOWN_GAPS_MD_TARGET,
    KNOWN_GAPS_SCHEMA, KnownGapDoc, KnownGapsDocument, KnownGapsRenderer, MarkdownGapRenderer,
    RenderGapsFormat, RenderGapsOutcome, run_render_gaps,
};
pub use status::{
    STATUS_JSON_TARGET, STATUS_MD_END, STATUS_MD_START, STATUS_MD_TARGET, STATUS_SCHEMA,
    StatusOutcome, StatusRendered, format_status_json, format_status_markdown, run_status,
    run_status_out,
};
pub use verify::{
    CHECK_NAMES, VERIFY_SCHEMA, VerifyOutcome, format_verify_json, format_verify_text,
    verify_determinism,
};
pub use waivers::{
    FileWaiverExtender, FileWaiverPruner, FileWaiverRewriter, FileWaiverSink, FileWaiverSource,
    InMemoryWaiverExtender, InMemoryWaiverPruner, InMemoryWaiverRewriter, InMemoryWaiverSink,
    InMemoryWaiverSource, JsonWaiversRenderer, TextWaiversRenderer, WAIVERS_EXTEND_SCHEMA,
    WAIVERS_GRANT_SCHEMA, WAIVERS_PRUNE_SCHEMA, WAIVERS_REVOKE_SCHEMA, WAIVERS_SCHEMA,
    WaiverExtender, WaiverPruner, WaiverRewriter, WaiverRow, WaiverSink, WaiverSource,
    WaiverVerdict, WaiversDocument, WaiversExtendOutcome, WaiversExtendRequest,
    WaiversGrantOutcome, WaiversGrantRequest, WaiversOutcome, WaiversPruneOutcome, WaiversPruneRow,
    WaiversRenderer, WaiversRevokeOutcome, WaiversRevokeRequest, assess_waivers,
    format_waivers_extend_json, format_waivers_extend_text, format_waivers_grant_json,
    format_waivers_grant_text, format_waivers_json, format_waivers_prune_json,
    format_waivers_prune_text, format_waivers_revoke_json, format_waivers_revoke_text,
    format_waivers_text, run_waivers, run_waivers_extend, run_waivers_grant, run_waivers_prune,
    run_waivers_revoke,
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
    /// `evidence record` was called without at least one SRC path.
    EvidenceMissingSources {
        /// The usage text.
        usage: String,
    },
    /// `evidence record --out`/`--format` was passed without its value.
    EvidenceMissingValue {
        /// The flag that was missing its value (`--out` or `--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `evidence --format` received a value outside `text|json`.
    EvidenceBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `evidence` was followed by something other than `record`/`check`.
    EvidenceSubcommand {
        /// The offending subcommand.
        subcommand: String,
        /// The usage text.
        usage: String,
    },
    /// `evidence record` could not persist the merged store.
    EvidenceWrite {
        /// The store path.
        path: String,
        /// The diagnostic detail (the store's IO failure message).
        detail: String,
    },
    /// `status --check` found the committed document drifted from the
    /// computed status (or unreadable): the file and the
    /// `(registry, evidence, policy)` render disagree (the M1.10
    /// "STATUS is up to date" gate).
    StatusDrift {
        /// The checked document path.
        path: String,
        /// What the comparison (or read) found.
        detail: String,
    },
    /// `status --out` could not write the rendered document.
    StatusWrite {
        /// The output path.
        path: String,
        /// The IO error message.
        detail: String,
    },
    /// `waivers --format` received a value outside `text|json`.
    WaiversBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `waivers --format` was passed without its value.
    WaiversMissingValue {
        /// The flag that was missing its value (`--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `waivers grant` was called without one of its required flags
    /// (`--feature`, `--reason`, `--issue`) or with an unusable reason.
    WaiversGrantMissingField {
        /// The missing (or unusable) flag.
        field: String,
        /// The usage text.
        usage: String,
    },
    /// `waivers grant --days` received a value outside the C9 horizon
    /// (`1..=WAIVER_MAX_AGE_DAYS`) or not an integer.
    WaiversGrantBadDays {
        /// The offending value.
        days: String,
        /// The horizon the value must stay inside.
        horizon: i64,
    },
    /// `waivers grant --issue` did not carry an `https://` link (the plan
    /// requires an issue link per waiver).
    WaiversGrantBadIssue {
        /// The offending link.
        issue: String,
    },
    /// `waivers grant --feature` did not name a registry feature.
    WaiversGrantUnknownFeature {
        /// The offending feature ID.
        feature: String,
        /// A few declared feature IDs (up to three) for the help line.
        candidates: Vec<String>,
    },
    /// The registry under ROOT did not load cleanly, so the grant cannot
    /// validate the feature ID (no waiver is written).
    WaiversGrantRegistry {
        /// The joined load diagnostics.
        detail: String,
    },
    /// A waiver with the same deterministic ID already exists in
    /// `ledger/waivers.toml` (a same-day re-grant is a duplicate, never a
    /// silent overwrite).
    WaiversGrantDuplicate {
        /// The colliding ID.
        id: String,
        /// The document that already carries it.
        path: String,
    },
    /// `waivers grant` could not append the canonical block to the
    /// committed document.
    WaiversGrantWrite {
        /// The document path.
        path: String,
        /// The IO (or serialization) failure message.
        detail: String,
    },
    /// `handoff --format` received a value outside `md|json`.
    HandoffBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `handoff --items`/`--out`/`--format` was passed without its value.
    HandoffMissingValue {
        /// The flag that was missing its value.
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// `handoff --items` received a value outside `1..=HANDOFF_MAX_ITEMS`
    /// or not an integer.
    HandoffBadItems {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `handoff --check` found the committed document drifted from the
    /// rendered handoff (or unreadable): the file and the composed
    /// `(compute, next, waivers)` render disagree (the plan §3.7
    /// determinism gate).
    HandoffDrift {
        /// The checked document path.
        path: String,
        /// What the comparison (or read) found.
        detail: String,
    },
    /// `handoff --out` could not write the rendered document.
    HandoffWrite {
        /// The output path.
        path: String,
        /// The IO error message.
        detail: String,
    },
    /// `waivers revoke` was called without its required `--id` flag (or
    /// with a blank/control-character ID).
    WaiversRevokeMissingField {
        /// The missing flag.
        field: String,
        /// The usage text.
        usage: String,
    },
    /// `waivers revoke --id` was blank or carried control characters (an
    /// ID can never match a canonical `WVR-...` entry).
    WaiversRevokeBadId {
        /// The offending ID.
        id: String,
    },
    /// `waivers revoke --id` named a waiver the committed document does
    /// not carry (nothing was written); `candidates` lists a few existing
    /// IDs for the help line.
    WaiversRevokeUnknownId {
        /// The offending ID.
        id: String,
        /// A few existing waiver IDs (up to three).
        candidates: Vec<String>,
    },
    /// The committed `ledger/waivers.toml` did not load cleanly (schema,
    /// duplicate ID, horizon violation), so the rewriter refuses to
    /// rewrite it (no waiver is removed).
    WaiversRevokeLoad {
        /// The joined load diagnostics.
        detail: String,
    },
    /// `waivers revoke` could not rewrite the committed document.
    WaiversRevokeWrite {
        /// The document path.
        path: String,
        /// The IO (or serialization) failure message.
        detail: String,
    },
    /// `waivers extend` was called without its required `--id` flag (or
    /// with a non-numeric `--days` value, reported for whichever flag is
    /// missing or unusable).
    WaiversExtendMissingField {
        /// The missing (or unusable) flag.
        field: String,
        /// The usage text.
        usage: String,
    },
    /// `waivers extend --days` received a value outside the C9 horizon
    /// (or a non-numeric one at parse time); the new expiry is pinned
    /// relative to today, so it must sit in `1..=horizon`.
    WaiversExtendBadDays {
        /// The offending `--days` value.
        days: String,
        /// The C9 horizon (`WAIVER_MAX_AGE_DAYS`).
        horizon: i64,
    },
    /// `waivers extend --id` was blank or carried control characters (an
    /// ID can never match a canonical `WVR-...` entry).
    WaiversExtendBadId {
        /// The offending ID.
        id: String,
    },
    /// `waivers extend --id` named a waiver the committed document does
    /// not carry (nothing was written); `candidates` lists a few existing
    /// IDs for the help line.
    WaiversExtendUnknownId {
        /// The offending ID.
        id: String,
        /// A few existing waiver IDs (up to three).
        candidates: Vec<String>,
    },
    /// The committed `ledger/waivers.toml` did not load cleanly (schema,
    /// duplicate ID, horizon violation), so the extender refuses to
    /// rewrite it (no expiry moves).
    WaiversExtendLoad {
        /// The joined load diagnostics.
        detail: String,
    },
    /// `waivers extend` could not rewrite the committed document.
    WaiversExtendWrite {
        /// The document path.
        path: String,
        /// The IO (or serialization) failure message.
        detail: String,
    },
    /// `release-check` was called without its required `<VERSION>`
    /// positional.
    ReleaseCheckMissingVersion {
        /// The usage text.
        usage: String,
    },
    /// `release-check <VERSION>` did not parse as `vMAJOR.MINOR.PATCH`
    /// (or the unprefixed `MAJOR.MINOR.PATCH`).
    ReleaseCheckBadVersion {
        /// The offending value.
        value: String,
    },
    /// `release-check --format` received a value outside `text|json`.
    ReleaseCheckBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `release-check --format` was passed without its value.
    ReleaseCheckMissingValue {
        /// The flag that was missing its value (`--format`).
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// The committed `ledger/waivers.toml` did not load cleanly (schema,
    /// duplicate ID, horizon violation), so the pruner refuses to
    /// rewrite it (no waiver is removed).
    WaiversPruneLoad {
        /// The joined load diagnostics.
        detail: String,
    },
    /// `waivers prune` could not rewrite the committed document.
    WaiversPruneWrite {
        /// The document path.
        path: String,
        /// The IO (or serialization) failure message.
        detail: String,
    },
    /// `gaps witness` was called without its required `--id`/`--witness`
    /// flags (or with an unusable one).
    GapsWitnessMissingField {
        /// The missing (or unusable) flag.
        field: String,
        /// The usage text.
        usage: String,
    },
    /// `gaps witness --id` was blank or carried control characters (an ID
    /// can never match a canonical `GAP-NNNN` entry).
    GapsWitnessBadId {
        /// The offending ID.
        id: String,
    },
    /// `gaps witness --witness` was not a usable repo-relative fixture
    /// path (blank, absolute, backslash, or a `..` escape).
    GapsWitnessBadWitness {
        /// The offending path.
        witness: String,
    },
    /// `gaps witness --id` named a gap the registry does not carry
    /// (nothing was written); `candidates` lists a few existing IDs for
    /// the help line.
    GapsWitnessUnknownId {
        /// The offending ID.
        id: String,
        /// A few existing gap IDs (up to three).
        candidates: Vec<String>,
    },
    /// The registry under ROOT did not load cleanly, so the witness
    /// cannot validate the gap ID (nothing is written).
    GapsWitnessRegistry {
        /// The joined load diagnostics.
        detail: String,
    },
    /// The named gap is already witnessed (a second witness would be a
    /// silent overwrite — retire or hand-edit instead).
    GapsWitnessAlreadyWitnessed {
        /// The already-witnessed gap ID.
        id: String,
        /// The witness the entry already carries (or its status spelling
        /// when the entry carries no path).
        witness: String,
        /// The registry file that carries the witnessed entry.
        path: String,
    },
    /// `gaps witness --witness` named a file that does not exist under
    /// ROOT (a phantom witness would be silent debt — plan §3.3).
    GapsWitnessMissingFixture {
        /// The offending path (as given).
        witness: String,
        /// The resolved path that does not exist.
        path: String,
    },
    /// `gaps witness` could not rewrite the registry file that carries
    /// the matched block.
    GapsWitnessWrite {
        /// The registry file path.
        path: String,
        /// The IO (or shape) failure message.
        detail: String,
    },
    /// A `gaps` write face received a `--format` outside `text|json`.
    GapsBadFormat {
        /// The offending value.
        value: String,
        /// The usage text.
        usage: String,
    },
    /// `gaps retire` was called without its required `--id` flag.
    GapsRetireMissingField {
        /// The missing flag.
        field: String,
        /// The usage text.
        usage: String,
    },
    /// `gaps retire --id` was blank or carried control characters (an ID
    /// can never match a canonical `GAP-NNNN` entry).
    GapsRetireBadId {
        /// The offending ID.
        id: String,
    },
    /// `gaps retire --id` named a gap the registry does not carry
    /// (nothing was written); `candidates` lists a few existing IDs for
    /// the help line.
    GapsRetireUnknownId {
        /// The offending ID.
        id: String,
        /// A few existing gap IDs (up to three).
        candidates: Vec<String>,
    },
    /// The registry under ROOT did not load cleanly, so the retirer
    /// refuses to rewrite it (no gap is removed).
    GapsRetireLoad {
        /// The joined load diagnostics.
        detail: String,
    },
    /// `gaps retire` could not rewrite the registry file that carries
    /// the matched block.
    GapsRetireWrite {
        /// The registry file path.
        path: String,
        /// The IO (or shape) failure message.
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
            Self::EvidenceMissingSources { usage } => {
                write!(f, "evidence record requires at least one SRC path\n{usage}")
            }
            Self::EvidenceMissingValue { flag, usage } => {
                write!(f, "missing value for {flag}\n{usage}")
            }
            Self::EvidenceBadFormat { value, usage } => {
                write!(
                    f,
                    "invalid --format {value:?} (expected text|json)\n{usage}"
                )
            }
            Self::EvidenceSubcommand { subcommand, usage } => {
                write!(f, "unknown evidence subcommand {subcommand:?}\n{usage}")
            }
            Self::EvidenceWrite { path, detail } => {
                write!(f, "cannot write evidence store {path}: {detail}")
            }
            Self::StatusDrift { path, detail } => {
                write!(f, "status drift at {path}: {detail}")
            }
            Self::StatusWrite { path, detail } => write!(f, "cannot write {path}: {detail}"),
            Self::WaiversBadFormat { value, usage } => {
                write!(f, "--format expects text or json, got {value}\n{usage}")
            }
            Self::WaiversMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (text|json)\n{usage}")
            }
            Self::WaiversGrantMissingField { field, usage } => {
                write!(
                    f,
                    "waivers grant expects {field} (a registry feature ID, a \
                     usable reason, and an https:// issue link)\n{usage}"
                )
            }
            Self::WaiversGrantBadDays { days, horizon } => {
                write!(
                    f,
                    "--days expects an integer between 1 and {horizon} (the C9 \
                     horizon), got {days}"
                )
            }
            Self::WaiversGrantBadIssue { issue } => {
                write!(f, "--issue must be an https:// link, got {issue:?}")
            }
            Self::WaiversGrantUnknownFeature { feature, .. } => {
                write!(f, "--feature {feature:?} is not a registry feature")
            }
            Self::WaiversGrantRegistry { detail } => {
                write!(f, "the registry under ROOT did not load: {detail}")
            }
            Self::WaiversGrantDuplicate { id, path } => {
                write!(f, "waiver {id} already exists in {path}")
            }
            Self::WaiversGrantWrite { path, detail } => {
                write!(f, "cannot append to {path}: {detail}")
            }
            Self::HandoffBadFormat { value, usage } => {
                write!(f, "--format expects md or json, got {value}\n{usage}")
            }
            Self::HandoffMissingValue { flag, usage } => {
                write!(
                    f,
                    "{flag} expects a value (a count, a path, or md|json)\n{usage}"
                )
            }
            Self::HandoffBadItems { value, usage } => {
                write!(
                    f,
                    "--items expects an integer in 1..={HANDOFF_MAX_ITEMS}, got {value}\n{usage}"
                )
            }
            Self::HandoffDrift { path, detail } => {
                write!(
                    f,
                    "handoff document drifted from the composed state: {path}: {detail}"
                )
            }
            Self::HandoffWrite { path, detail } => write!(f, "cannot write {path}: {detail}"),
            Self::WaiversRevokeMissingField { field, usage } => {
                write!(f, "{field} expects a value (the exact WVR-... ID)\n{usage}")
            }
            Self::WaiversRevokeBadId { id } => {
                write!(f, "--id expects a usable waiver ID, got {id:?}")
            }
            Self::WaiversRevokeUnknownId { id, .. } => {
                write!(f, "no waiver carries the ID {id:?} in ledger/waivers.toml")
            }
            Self::WaiversRevokeLoad { detail } => {
                write!(f, "ledger/waivers.toml does not load cleanly: {detail}")
            }
            Self::WaiversRevokeWrite { path, detail } => {
                write!(f, "cannot rewrite {path}: {detail}")
            }
            Self::WaiversExtendMissingField { field, usage } => {
                write!(
                    f,
                    "{field} expects a value (the exact WVR-... ID, or a whole-day \
                     count for --days)\n{usage}"
                )
            }
            Self::WaiversExtendBadDays { days, horizon } => {
                write!(
                    f,
                    "--days expects a whole-day count in 1..={horizon} (constitution C9), \
                     got {days:?}"
                )
            }
            Self::WaiversExtendBadId { id } => {
                write!(f, "--id expects a usable waiver ID, got {id:?}")
            }
            Self::WaiversExtendUnknownId { id, .. } => {
                write!(f, "no waiver carries the ID {id:?} in ledger/waivers.toml")
            }
            Self::WaiversExtendLoad { detail } => {
                write!(f, "ledger/waivers.toml does not load cleanly: {detail}")
            }
            Self::WaiversExtendWrite { path, detail } => {
                write!(f, "cannot rewrite {path}: {detail}")
            }
            Self::ReleaseCheckMissingVersion { usage } => {
                write!(
                    f,
                    "release-check expects a release version (vMAJOR.MINOR.PATCH)\n{usage}"
                )
            }
            Self::ReleaseCheckBadVersion { value } => write!(
                f,
                "<VERSION> expects three dot-separated numbers (v1.0.0), got {value:?}"
            ),
            Self::ReleaseCheckBadFormat { value, usage } => {
                write!(f, "--format expects text or json, got {value}\n{usage}")
            }
            Self::ReleaseCheckMissingValue { flag, usage } => {
                write!(f, "{flag} expects a value (text|json)\n{usage}")
            }
            Self::WaiversPruneLoad { detail } => {
                write!(f, "ledger/waivers.toml does not load cleanly: {detail}")
            }
            Self::WaiversPruneWrite { path, detail } => {
                write!(f, "cannot rewrite {path}: {detail}")
            }
            Self::GapsWitnessMissingField { field, usage } => {
                write!(
                    f,
                    "{field} expects a value (the exact GAP-NNNN ID and the \
                     repo-relative witness fixture path)\n{usage}"
                )
            }
            Self::GapsWitnessBadId { id } => {
                write!(f, "--id expects a usable gap ID, got {id:?}")
            }
            Self::GapsWitnessBadWitness { witness } => {
                write!(
                    f,
                    "--witness expects a repo-relative fixture path (no \
                     absolute, backslash, or .. escape), got {witness:?}"
                )
            }
            Self::GapsWitnessUnknownId { id, .. } => {
                write!(f, "no gap carries the ID {id:?} in ledger/gaps/known/")
            }
            Self::GapsWitnessRegistry { detail } => {
                write!(f, "the registry under ROOT did not load: {detail}")
            }
            Self::GapsWitnessAlreadyWitnessed { id, witness, path } => {
                write!(f, "gap {id} is already witnessed in {path} ({witness})")
            }
            Self::GapsWitnessMissingFixture { witness, path } => {
                write!(
                    f,
                    "the witness fixture {witness:?} does not exist at {path}"
                )
            }
            Self::GapsWitnessWrite { path, detail } => {
                write!(f, "cannot rewrite {path}: {detail}")
            }
            Self::GapsBadFormat { value, usage } => {
                write!(f, "--format expects text or json, got {value}\n{usage}")
            }
            Self::GapsRetireMissingField { field, usage } => {
                write!(
                    f,
                    "{field} expects a value (the exact GAP-NNNN ID to remove)\n{usage}"
                )
            }
            Self::GapsRetireBadId { id } => {
                write!(f, "--id expects a usable gap ID, got {id:?}")
            }
            Self::GapsRetireUnknownId { id, .. } => {
                write!(f, "no gap carries the ID {id:?} in ledger/gaps/known/")
            }
            Self::GapsRetireLoad { detail } => {
                write!(f, "the registry under ROOT does not load cleanly: {detail}")
            }
            Self::GapsRetireWrite { path, detail } => {
                write!(f, "cannot rewrite {path}: {detail}")
            }
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
            Self::EvidenceMissingSources { .. } => "glyim-ledger::evidence_missing_sources",
            Self::EvidenceMissingValue { .. } => "glyim-ledger::evidence_missing_value",
            Self::EvidenceBadFormat { .. } => "glyim-ledger::evidence_bad_format",
            Self::EvidenceSubcommand { .. } => "glyim-ledger::evidence_subcommand",
            Self::EvidenceWrite { .. } => "glyim-ledger::evidence_write",
            Self::StatusDrift { .. } => "glyim-ledger::status_drift",
            Self::StatusWrite { .. } => "glyim-ledger::status_write",
            Self::WaiversBadFormat { .. } => "glyim-ledger::waivers_bad_format",
            Self::WaiversMissingValue { .. } => "glyim-ledger::waivers_missing_value",
            Self::WaiversGrantMissingField { .. } => "glyim-ledger::waivers_grant_missing_field",
            Self::WaiversGrantBadDays { .. } => "glyim-ledger::waivers_grant_bad_days",
            Self::WaiversGrantBadIssue { .. } => "glyim-ledger::waivers_grant_bad_issue",
            Self::WaiversGrantUnknownFeature { .. } => {
                "glyim-ledger::waivers_grant_unknown_feature"
            }
            Self::WaiversGrantRegistry { .. } => "glyim-ledger::waivers_grant_registry",
            Self::WaiversGrantDuplicate { .. } => "glyim-ledger::waivers_grant_duplicate",
            Self::WaiversGrantWrite { .. } => "glyim-ledger::waivers_grant_write",
            Self::HandoffBadFormat { .. } => "glyim-ledger::handoff_bad_format",
            Self::HandoffMissingValue { .. } => "glyim-ledger::handoff_missing_value",
            Self::HandoffBadItems { .. } => "glyim-ledger::handoff_bad_items",
            Self::HandoffDrift { .. } => "glyim-ledger::handoff_drift",
            Self::HandoffWrite { .. } => "glyim-ledger::handoff_write",
            Self::WaiversRevokeMissingField { .. } => "glyim-ledger::waivers_revoke_missing_field",
            Self::WaiversRevokeBadId { .. } => "glyim-ledger::waivers_revoke_bad_id",
            Self::WaiversRevokeUnknownId { .. } => "glyim-ledger::waivers_revoke_unknown_id",
            Self::WaiversRevokeLoad { .. } => "glyim-ledger::waivers_revoke_load",
            Self::WaiversRevokeWrite { .. } => "glyim-ledger::waivers_revoke_write",
            Self::WaiversExtendMissingField { .. } => "glyim-ledger::waivers_extend_missing_field",
            Self::WaiversExtendBadDays { .. } => "glyim-ledger::waivers_extend_bad_days",
            Self::WaiversExtendBadId { .. } => "glyim-ledger::waivers_extend_bad_id",
            Self::WaiversExtendUnknownId { .. } => "glyim-ledger::waivers_extend_unknown_id",
            Self::WaiversExtendLoad { .. } => "glyim-ledger::waivers_extend_load",
            Self::WaiversExtendWrite { .. } => "glyim-ledger::waivers_extend_write",
            Self::ReleaseCheckMissingVersion { .. } => {
                "glyim-ledger::release_check_missing_version"
            }
            Self::ReleaseCheckBadVersion { .. } => "glyim-ledger::release_check_bad_version",
            Self::ReleaseCheckBadFormat { .. } => "glyim-ledger::release_check_bad_format",
            Self::ReleaseCheckMissingValue { .. } => "glyim-ledger::release_check_missing_value",
            Self::WaiversPruneLoad { .. } => "glyim-ledger::waivers_prune_load",
            Self::WaiversPruneWrite { .. } => "glyim-ledger::waivers_prune_write",
            Self::GapsWitnessMissingField { .. } => "glyim-ledger::gaps_witness_missing_field",
            Self::GapsWitnessBadId { .. } => "glyim-ledger::gaps_witness_bad_id",
            Self::GapsWitnessBadWitness { .. } => "glyim-ledger::gaps_witness_bad_witness",
            Self::GapsWitnessUnknownId { .. } => "glyim-ledger::gaps_witness_unknown_id",
            Self::GapsWitnessRegistry { .. } => "glyim-ledger::gaps_witness_registry",
            Self::GapsWitnessAlreadyWitnessed { .. } => {
                "glyim-ledger::gaps_witness_already_witnessed"
            }
            Self::GapsWitnessMissingFixture { .. } => "glyim-ledger::gaps_witness_missing_fixture",
            Self::GapsWitnessWrite { .. } => "glyim-ledger::gaps_witness_write",
            Self::GapsBadFormat { .. } => "glyim-ledger::gaps_bad_format",
            Self::GapsRetireMissingField { .. } => "glyim-ledger::gaps_retire_missing_field",
            Self::GapsRetireBadId { .. } => "glyim-ledger::gaps_retire_bad_id",
            Self::GapsRetireUnknownId { .. } => "glyim-ledger::gaps_retire_unknown_id",
            Self::GapsRetireLoad { .. } => "glyim-ledger::gaps_retire_load",
            Self::GapsRetireWrite { .. } => "glyim-ledger::gaps_retire_write",
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
            Self::EvidenceMissingSources { .. } => Some(Box::new(
                "name at least one harness-emitted JSONL file \
                 (set GLYIM_TEST_EVIDENCE on a corpus run to produce one)"
                    .to_string(),
            )),
            Self::EvidenceMissingValue { .. } => Some(Box::new(
                "`--out PATH` names the store file (default \
                 ledger/evidence/recorded.jsonl); `--format` is text|json"
                    .to_string(),
            )),
            Self::EvidenceBadFormat { .. } => Some(Box::new(
                "`--format text` renders the merge report, `--format json` \
                 the glyim.ledger-evidence-record/1 contract"
                    .to_string(),
            )),
            Self::EvidenceSubcommand { .. } => Some(Box::new(
                "`evidence record` curates harness records into the store, \
                 `evidence check` validates the committed store"
                    .to_string(),
            )),
            Self::EvidenceWrite { .. } => Some(Box::new(
                "the record face creates missing parent directories; check \
                 that the --out path's parent is writable (plan §3.4 store)"
                    .to_string(),
            )),
            Self::StatusDrift { .. } => Some(Box::new(
                "run `cargo run -p glyim-ledger -- status . --out STATUS.md` \
                 (and `--format json --out STATUS.json`) to regenerate the \
                 documents from the computed status"
                    .to_string(),
            )),
            Self::StatusWrite { .. } => Some(Box::new(
                "check that the --out path is writable; the document is the \
                 generated STATUS face (plan M1.10)"
                    .to_string(),
            )),
            Self::WaiversBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.ledger-waivers/1 contract; \
                 plain text is the default"
                    .to_string(),
            )),
            Self::WaiversMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--format json`"
                    .to_string(),
            )),
            Self::WaiversGrantMissingField { field, .. } => Some(Box::new(format!(
                "pass {field} on the same command line, e.g. `--feature \
                 LANG-FOR-VEC-INTOITER --reason \"fix in flight\" --issue \
                 https://github.com/elcoosp/glyim/issues/123`; --days 7 is \
                 the default horizon"
            ))),
            Self::WaiversGrantBadDays { horizon, .. } => Some(Box::new(format!(
                "constitution C9 caps every waiver at {horizon} days; grant \
                 a shorter horizon and re-grant while the fix lands"
            ))),
            Self::WaiversGrantBadIssue { .. } => Some(Box::new(
                "the plan requires an issue link per waiver; pass the \
                 tracking URL, e.g. https://github.com/elcoosp/glyim/issues/123"
                    .to_string(),
            )),
            Self::WaiversGrantUnknownFeature { candidates, .. } => Some(Box::new(format!(
                "the waiver covers a registry feature's regressions; declared \
                 ids include {} (see `glyim-ledger check` or ledger/features/*.toml)",
                candidates.join(", ")
            ))),
            Self::WaiversGrantRegistry { .. } => Some(Box::new(
                "run `glyim-ledger check` first: the registry must load \
                 cleanly before a waiver can be granted"
                    .to_string(),
            )),
            Self::WaiversGrantDuplicate { .. } => Some(Box::new(
                "one exception per feature per day; extend the existing \
                 entry's expiry instead of re-granting it"
                    .to_string(),
            )),
            Self::WaiversGrantWrite { .. } => Some(Box::new(
                "check that ledger/waivers.toml is writable; the appended \
                 block is the canonical [[waiver]] fragment"
                    .to_string(),
            )),
            Self::HandoffBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.ledger-handoff/1 contract; \
                 the marker-delimited Markdown document is the default"
                    .to_string(),
            )),
            Self::HandoffMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--items 10`, `--out HANDOFF.md`, or `--format json`"
                    .to_string(),
            )),
            Self::HandoffBadItems { .. } => Some(Box::new(
                "the handoff is a summary, not the queue: render 1 to 20 \
                 items (`--items 5` is the default); the full queue is one \
                 `next` call away"
                    .to_string(),
            )),
            Self::HandoffDrift { .. } => Some(Box::new(
                "run `cargo run -p glyim-ledger -- handoff . --out HANDOFF.md` \
                 to regenerate the document from the composed state"
                    .to_string(),
            )),
            Self::HandoffWrite { .. } => Some(Box::new(
                "check that the --out path is writable; the document is the \
                 generated HANDOFF face (plan §3.7)"
                    .to_string(),
            )),
            Self::WaiversRevokeMissingField { field, .. } => Some(Box::new(format!(
                "pass {field} on the same command line with the exact ID, e.g. \
                 `--id WVR-20261113-lang-grant-demo`; the health face \
                 (`glyim-ledger waivers`) prints every current ID"
            ))),
            Self::WaiversRevokeBadId { .. } => Some(Box::new(
                "a waiver ID is a canonical WVR-<date>-<slug> token; run \
                 `glyim-ledger waivers` to list the live inventory"
                    .to_string(),
            )),
            Self::WaiversRevokeUnknownId { candidates, .. } => Some(Box::new(format!(
                "revoke the exact ID of an existing waiver; current ids \
                 include {} (see `glyim-ledger waivers` for the full report)",
                candidates.join(", ")
            ))),
            Self::WaiversRevokeLoad { .. } => Some(Box::new(
                "the rewriter refuses to rewrite a document it cannot fully \
                 parse; fix the load diagnostics first (`glyim-ledger waivers` \
                 prints them)"
                    .to_string(),
            )),
            Self::WaiversRevokeWrite { .. } => Some(Box::new(
                "check that ledger/waivers.toml is writable; the rewrite \
                 replaces the document with the canonical remaining entries"
                    .to_string(),
            )),
            Self::WaiversExtendMissingField { field, .. } => Some(Box::new(format!(
                "pass {field} on the same command line, e.g. \
                 `--id WVR-20261113-lang-grant-demo --days 7`; the health \
                 face (`glyim-ledger waivers`) prints every current ID"
            ))),
            Self::WaiversExtendBadDays { .. } => Some(Box::new(
                "the new expiry is pinned relative to today, so the horizon \
                 is `1..=14` whole days (constitution C9); omit --days for \
                 the 7-day working default"
                    .to_string(),
            )),
            Self::WaiversExtendBadId { .. } => Some(Box::new(
                "a waiver ID is a canonical WVR-<date>-<slug> token; run \
                 `glyim-ledger waivers` to list the live inventory"
                    .to_string(),
            )),
            Self::WaiversExtendUnknownId { candidates, .. } => Some(Box::new(format!(
                "extend the exact ID of an existing waiver; current ids \
                 include {} (see `glyim-ledger waivers` for the full report)",
                candidates.join(", ")
            ))),
            Self::WaiversExtendLoad { .. } => Some(Box::new(
                "the extender refuses to rewrite a document it cannot fully \
                 parse; fix the load diagnostics first (`glyim-ledger waivers` \
                 prints them)"
                    .to_string(),
            )),
            Self::WaiversExtendWrite { .. } => Some(Box::new(
                "check that ledger/waivers.toml is writable; the rewrite \
                 replaces the document with the canonical entries, only the \
                 extended block's expiry moved"
                    .to_string(),
            )),
            Self::ReleaseCheckMissingVersion { .. } => Some(Box::new(
                "pass the version the verdict names, e.g. `release-check \
                 v1.0.0` (the `v` prefix is optional)"
                    .to_string(),
            )),
            Self::ReleaseCheckBadVersion { .. } => Some(Box::new(
                "a release triple is three dot-separated numbers, optionally \
                 v-prefixed — `v1.0.0` or `1.0.0`; no pre-release tags or \
                 build metadata on a release gate"
                    .to_string(),
            )),
            Self::ReleaseCheckBadFormat { .. } => Some(Box::new(
                "`--format json` renders the glyim.ledger-release-check/1 \
                 contract; plain text is the default"
                    .to_string(),
            )),
            Self::ReleaseCheckMissingValue { .. } => Some(Box::new(
                "the flag needs its value on the same command line, e.g. \
                 `--format json`"
                    .to_string(),
            )),
            Self::WaiversPruneLoad { .. } => Some(Box::new(
                "the pruner refuses to rewrite a document it cannot fully \
                 parse; fix the load diagnostics first (`glyim-ledger waivers` \
                 prints them)"
                    .to_string(),
            )),
            Self::WaiversPruneWrite { .. } => Some(Box::new(
                "check that ledger/waivers.toml is writable; the rewrite \
                 replaces the document with the canonical remaining entries, \
                 every expired block removed"
                    .to_string(),
            )),
            Self::GapsWitnessMissingField { field, .. } => Some(Box::new(format!(
                "the flag needs its value on the same command line, e.g. \
                 `gaps witness --id GAP-0002 --witness fixtures/known-gap.g` \
                 ({field})"
            ))),
            Self::GapsWitnessBadId { .. } => Some(Box::new(
                "the ID must match the registry's canonical GAP-NNNN \
                 spelling (`glyim-ledger gaps` prints every open ID)"
                    .to_string(),
            )),
            Self::GapsWitnessBadWitness { .. } => Some(Box::new(
                "use the same repo-relative spelling the committed entries \
                 carry, e.g. `crates/glyim-test/tests/compile-fail/x.g`"
                    .to_string(),
            )),
            Self::GapsWitnessUnknownId { candidates, .. } => Some(Box::new(format!(
                "the open gap IDs include {candidates:?} (`glyim-ledger \
                 gaps` prints the full report)"
            ))),
            Self::GapsWitnessRegistry { .. } => Some(Box::new(
                "fix the registry diagnostics first (`glyim-ledger check` \
                 prints every diagnostic); a registry that does not parse \
                 cleanly is never rewritten"
                    .to_string(),
            )),
            Self::GapsWitnessAlreadyWitnessed { .. } => Some(Box::new(
                "a witnessed gap is never silently re-witnessed; retire the \
                 entry when the gap closes (`gaps retire`) or edit the \
                 registry deliberately"
                    .to_string(),
            )),
            Self::GapsWitnessMissingFixture { .. } => Some(Box::new(
                "create and commit the fixture first — a witness that \
                 points at nothing would trade visible debt for silent debt"
                    .to_string(),
            )),
            Self::GapsWitnessWrite { .. } => Some(Box::new(
                "check that the registry file is writable; the rewriter \
                 edits only the matched [[gap]] block's status/witness lines"
                    .to_string(),
            )),
            Self::GapsBadFormat { .. } => Some(Box::new(
                "`--format json` renders the face's versioned contract; \
                 plain text is the default"
                    .to_string(),
            )),
            Self::GapsRetireMissingField { field, .. } => Some(Box::new(format!(
                "the flag needs its value on the same command line, e.g. \
                 `gaps retire --id GAP-0002` ({field})"
            ))),
            Self::GapsRetireBadId { .. } => Some(Box::new(
                "the ID must match the registry's canonical GAP-NNNN \
                 spelling (`glyim-ledger gaps` prints every open ID)"
                    .to_string(),
            )),
            Self::GapsRetireUnknownId { candidates, .. } => Some(Box::new(format!(
                "the open gap IDs include {candidates:?} (`glyim-ledger \
                 gaps` prints the full report)"
            ))),
            Self::GapsRetireLoad { .. } => Some(Box::new(
                "the retirer refuses to rewrite a registry it cannot fully \
                 parse; fix the diagnostics first (`glyim-ledger check` \
                 prints every diagnostic)"
                    .to_string(),
            )),
            Self::GapsRetireWrite { .. } => Some(Box::new(
                "check that the registry file is writable; the retirer \
                 drops only the matched [[gap]] block's lines"
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
    /// `gaps witness [ROOT] --id <GAP-ID> --witness <PATH> [--format
    /// text|json]` — the gap-lifecycle write face: attach the witness
    /// fixture that reproduces one unwitnessed gap. Validates the request
    /// against the loaded registry and the fixture tree (a phantom or
    /// escaping path is a rejection), edits exactly the matched `[[gap]]`
    /// block (`status` flipped, the aligned `witness` line appended, the
    /// curated header prose preserved byte-for-byte), and never leaves a
    /// partial write behind. The dispatch layer refreshes the generated
    /// `KNOWN_GAPS.md` after a successful rewrite so the `render-gaps
    /// --check` gate stays green.
    GapsWitness {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// The exact gap ID (`GAP-NNNN`) to witness.
        id: String,
        /// The witness fixture path, relative to the project root.
        witness: String,
        /// Output format; [`gaps::GapsFormat::Text`] by default.
        format: GapsFormat,
    },
    /// `gaps retire [ROOT] --id <GAP-ID> [--format text|json]` — the
    /// closure half of the gap-lifecycle write face: removes one gap's
    /// `[[gap]]` block from its registry file entirely (the rendered
    /// `KNOWN_GAPS.md` says closed entries retire to the git history of
    /// the file and of the registry — until this face the removal had no
    /// sanctioned command). Validates the request, refuses a registry
    /// that does not load cleanly, edits exactly the matched block (the
    /// curated header prose preserved byte-for-byte), and never leaves a
    /// partial write behind. The dispatch layer refreshes the generated
    /// `KNOWN_GAPS.md` after a successful removal so the `render-gaps
    /// --check` gate stays green.
    GapsRetire {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// The exact gap ID (`GAP-NNNN`) to remove.
        id: String,
        /// Output format; [`gaps::GapsFormat::Text`] by default.
        format: GapsFormat,
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
    /// `status [ROOT] [--evidence PATH] [--format md|json] [--out PATH]
    /// [--check]` — the plan §6 M1.9 status renderers, plus the M1.10
    /// remainder: `--out PATH` writes the rendered document (default
    /// `STATUS.md` / `STATUS.json` by face under ROOT) and `--check`
    /// byte-compares the committed document against the computed status.
    Status {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Explicit evidence JSONL path (overrides `ledger/evidence/*.jsonl`).
        evidence: Option<PathBuf>,
        /// Output format; [`StatusFormat::Markdown`] by default.
        format: StatusFormat,
        /// File to write the rendered document to; `None` prints to
        /// stdout. `--check` compares against this file (default
        /// `<ROOT>/STATUS.md` or `<ROOT>/STATUS.json` by face).
        out: Option<PathBuf>,
        /// Compare the rendered bytes against the committed document
        /// instead of writing it (the "STATUS is up to date" gate).
        check: bool,
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
    /// `evidence record [--out PATH] [--format text|json] <SRC...>` — the
    /// plan §3.4 evidence-store curation face: validate harness-emitted
    /// records against the registry and merge them into the committed
    /// store (whole-record dedupe, canonical order).
    EvidenceRecord {
        /// Harness-emitted JSONL files to curate (at least one).
        sources: Vec<PathBuf>,
        /// Store file to merge into; defaults to
        /// `ledger/evidence/recorded.jsonl`.
        out: PathBuf,
        /// Output format; [`EvidenceFormat::Text`] by default.
        format: EvidenceFormat,
    },
    /// `evidence check [ROOT] [--format text|json]` — validate the
    /// committed `ledger/evidence/*.jsonl` store against the registry and
    /// report its coverage; error diagnostics fail the exit code (the CI
    /// gate face).
    EvidenceCheck {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Output format; [`EvidenceFormat::Text`] by default.
        format: EvidenceFormat,
    },
    /// `evidence audit [ROOT] [--format text|json]` — audit the committed
    /// store against the filesystem: recompute every record's
    /// `fixture_sha256` from the fixture bytes at its resolved path and
    /// report per-record verdicts; any missing, drifted, or unreadable
    /// fixture fails the exit code (the trust-loop closure the check
    /// face cannot express).
    EvidenceAudit {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Output format; [`EvidenceFormat::Text`] by default.
        format: EvidenceFormat,
    },
    /// `waivers [ROOT] [--format text|json]` — the constitution-C9
    /// waiver-health report: every waiver loaded from
    /// `ledger/waivers.toml` with its expiry verdict; expired waivers and
    /// load diagnostics fail the exit code (expired exceptions must fail
    /// CI even when no regression needs covering yet).
    Waivers {
        /// Project directory containing `ledger/waivers.toml` (`.` by
        /// default).
        root: PathBuf,
        /// Output format; [`WaiversFormat::Text`] by default.
        format: WaiversFormat,
    },
    /// `waivers grant [ROOT] --feature <ID> --reason <TEXT> --issue <URL>
    /// [--days N] [--format text|json]` — the curated write face for the
    /// C9 exception registry: validates the request against the registry
    /// and the C9 horizon, composes the deterministic ID
    /// (`WVR-<YYYYMMDD>-<feature slug>`), and appends the canonical
    /// `[[waiver]]` block to `ledger/waivers.toml` (never overwrites).
    WaiversGrant {
        /// Project directory containing `ledger/waivers.toml` (`.` by
        /// default).
        root: PathBuf,
        /// The registry feature ID whose regressions the waiver covers.
        feature: String,
        /// Why the exception exists.
        reason: String,
        /// The `https://` tracking link.
        issue: String,
        /// The horizon in days; `None` uses the 7-day CLI default.
        days: Option<i64>,
        /// Output format; [`WaiversFormat::Text`] by default.
        format: WaiversFormat,
    },
    /// `handoff [ROOT] [--items N] [--format md|json] [--out PATH]
    /// [--check]` — the plan §3.7 session-handoff generator: composes the
    /// compute, `next`, and `waivers` face outcomes into one short
    /// document (readiness histogram, top-N work queue, waiver health,
    /// witness debt). `--out PATH` writes the rendered document (default
    /// `HANDOFF.md` / `HANDOFF.json` by face under ROOT) and `--check`
    /// byte-compares the committed document against the render.
    Handoff {
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Render at most this many work-queue items; `None` uses the
        /// 5-item default (`HANDOFF_DEFAULT_ITEMS`).
        items: Option<usize>,
        /// Output format; [`HandoffFormat::Markdown`] by default.
        format: HandoffFormat,
        /// File to write the rendered document to; `None` prints to
        /// stdout. `--check` compares against this file (default
        /// `<ROOT>/HANDOFF.md` or `<ROOT>/HANDOFF.json` by face).
        out: Option<PathBuf>,
        /// Compare the rendered bytes against the committed document
        /// instead of writing it (the handoff drift gate).
        check: bool,
    },
    /// `waivers revoke [ROOT] --id <WVR-ID> [--format text|json]` — the
    /// curated write face retiring one C9 exception: validates the
    /// request, refuses a document that does not load cleanly, and
    /// removes exactly the one matching `[[waiver]]` block, rewriting
    /// the remaining entries canonically (never a silent partial write).
    WaiversRevoke {
        /// Project directory containing `ledger/waivers.toml` (`.` by
        /// default).
        root: PathBuf,
        /// The exact waiver ID (`WVR-...`) to remove.
        id: String,
        /// Output format; [`WaiversFormat::Text`] by default.
        format: WaiversFormat,
    },
    /// `waivers extend [ROOT] --id <WVR-ID> [--days <N>] [--format
    /// text|json]` — the curated write face re-pinning one C9 exception's
    /// expiry: validates the request, refuses a document that does not
    /// load cleanly, and moves exactly the one matching `[[waiver]]`
    /// block's expiry (identity, reason, and issue are never rewritten;
    /// the counterpart the duplicate-grant help line already points to).
    WaiversExtend {
        /// Project directory containing `ledger/waivers.toml` (`.` by
        /// default).
        root: PathBuf,
        /// The exact waiver ID (`WVR-...`) whose expiry moves.
        id: String,
        /// New horizon in whole days; [`diff::DEFAULT_WAIVER_DAYS`] when
        /// `--days` is omitted (resolved by the dispatch layer).
        days: Option<i64>,
        /// Output format; [`WaiversFormat::Text`] by default.
        format: WaiversFormat,
    },
    /// `release-check <VERSION> [ROOT] [--format text|json]` — the plan
    /// §10 final release gate: every declared gate green (`unmeasured`
    /// blocks — it is displayed, never silently passed), evidence
    /// reproducible (the M1.13 determinism self-check), no waiver active
    /// or expired, and no error diagnostic across the composed faces.
    ReleaseCheck {
        /// The release the verdict names (`vMAJOR.MINOR.PATCH`).
        version: ReleaseVersion,
        /// Project directory containing the `ledger/` registry (`.` by
        /// default).
        root: PathBuf,
        /// Output format; [`ReleaseCheckFormat::Text`] by default.
        format: ReleaseCheckFormat,
    },
    /// `waivers prune [ROOT] [--format text|json]` — the batch-retirement
    /// write face: it removes every expired `[[waiver]]` block from
    /// `ledger/waivers.toml` and rewrites the remaining entries
    /// canonically (a document that does not load cleanly is refused; a
    /// document with no expired waiver is never rewritten).
    WaiversPrune {
        /// Project directory containing `ledger/waivers.toml` (`.` by
        /// default).
        root: PathBuf,
        /// Output format; [`WaiversFormat::Text`] by default.
        format: WaiversFormat,
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

impl StatusFormat {
    /// Canonical flag spelling (used in tracing spans).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }
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

/// Output format of the `release-check` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseCheckFormat {
    /// The human-readable report ([`format_release_check_text`]).
    Text,
    /// The `glyim.ledger-release-check/1` JSON contract
    /// ([`format_release_check_json`]).
    Json,
}

impl ReleaseCheckFormat {
    /// Canonical flag spelling (used in tracing spans).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

/// Output format of the `verify` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyFormat {
    /// The human-readable self-check report ([`format_verify_text`]).
    Text,
    /// The `glyim.ledger-verify/1` JSON contract ([`format_verify_json`]).
    Json,
}

/// Output format of the `evidence record` / `evidence check` subcommands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceFormat {
    /// The human-readable report
    /// ([`format_evidence_record_text`](crate::evidence::format_evidence_record_text) /
    /// [`format_evidence_check_text`](crate::evidence::format_evidence_check_text)).
    Text,
    /// The `glyim.ledger-evidence-record/1` /
    /// `glyim.ledger-evidence-check/1` JSON contracts
    /// ([`format_evidence_record_json`](crate::evidence::format_evidence_record_json) /
    /// [`format_evidence_check_json`](crate::evidence::format_evidence_check_json)).
    Json,
}

impl EvidenceFormat {
    /// Canonical wire spelling (used in tracing spans).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

/// Output format of the `waivers` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiversFormat {
    /// The human-readable report ([`format_waivers_text`]).
    Text,
    /// The `glyim.ledger-waivers/1` JSON contract ([`format_waivers_json`]).
    Json,
}

/// Usage text returned with argument errors.
#[must_use]
pub fn usage() -> String {
    "usage: glyim-ledger check [ROOT]\n   or: glyim-ledger gaps [ROOT]\n   or: glyim-ledger gaps witness [ROOT] --id <GAP-ID> --witness <PATH> [--format text|json]\n   or: glyim-ledger gaps retire [ROOT] --id <GAP-ID> [--format text|json]\n   or: glyim-ledger next [ROOT] [--lane L] [--n N]\n   or: glyim-ledger explain <ID> [ROOT]\n   or: glyim-ledger compute [ROOT] [--evidence PATH] [--format text|json] [--snapshot PATH]\n   or: glyim-ledger status [ROOT] [--evidence PATH] [--format md|json] [--out PATH] [--check]\n   or: glyim-ledger diff <BASE> <CURRENT> [ROOT] [--format text|json]\n   or: glyim-ledger gates [ROOT] [--evidence PATH] [--format text|json]\n   or: glyim-ledger verify [ROOT] [--evidence PATH] [--format text|json]\n   or: glyim-ledger render-gaps [ROOT] [--format md|json] [--out PATH] [--check]\n   or: glyim-ledger evidence record [--out PATH] [--format text|json] <SRC...>\n   or: glyim-ledger evidence check [ROOT] [--format text|json]\n   or: glyim-ledger evidence audit [ROOT] [--format text|json]\n   or: glyim-ledger waivers [ROOT] [--format text|json]\n   or: glyim-ledger waivers grant [ROOT] --feature <ID> --reason <TEXT> --issue <URL> [--days N] [--format text|json]\n   or: glyim-ledger waivers revoke [ROOT] --id <WVR-ID> [--format text|json]\n   or: glyim-ledger waivers extend [ROOT] --id <WVR-ID> [--days N] [--format text|json]\n   or: glyim-ledger waivers prune [ROOT] [--format text|json]\n   or: glyim-ledger handoff [ROOT] [--items N] [--format md|json] [--out PATH] [--check]\n   or: glyim-ledger release-check <VERSION> [ROOT] [--format text|json]\n   or: glyim-ledger schema [OUT]\n   or: glyim-ledger inventory [ROOT]\n\
     \x20 ROOT defaults to the current directory; with no subcommand `check`\n\
     \x20 is assumed. `schema` writes to OUT or stdout; `next` filters by\n\
     \x20 lane (L|F|T|M|B|Q) and prints the glyim.ledger-next/1 JSON queue;\n\
     \x20 `explain` resolves one registry ID against the same queue;\n\
     \x20 `compute` prints per-feature readiness levels from evidence\n\
     \x20 (`ledger/evidence/*.jsonl` by default, `--evidence` to override;\n\
     \x20 `--snapshot PATH` also persists the glyim.ledger-snapshot/1\n\
     \x20 document); `status` renders the computed status as the\n\
     \x20 marker-delimited Markdown block (default) or the glyim.status/1\n\
     \x20 JSON contract — `--out PATH` writes the rendered document\n\
     \x20 (default STATUS.md / STATUS.json by face) and `--check` fails\n\
     \x20 on drift (the plan M1.10 \"STATUS is up to date\" gate);\n\
     \x20 `diff` compares two snapshot documents and fails\n\
     \x20 on any regression not covered by an unexpired waiver in\n\
     \x20 `ledger/waivers.toml` (plan C9); `gates` prints the RG-01..RG-24\n\
     \x20 release-gate verdicts declared in `ledger/gates.toml` —\n\
     \x20 unmeasured gates are displayed, never silently passed; `verify`\n\
     \x20 runs every ledger pipeline twice from independent load passes\n\
     \x20 and fails on any rendered-byte drift (plan M1.13);\n\
     \x20 `render-gaps` generates KNOWN_GAPS.md (or the glyim.known-gaps/1\n\
     \x20 JSON face) from the gap registry — `--out PATH` writes it,\n\
     \x20 `--check` fails on drift (plan M1.9); `gaps witness` records\n\
     \x20 the witness fixture that reproduces one unwitnessed gap\n\
     \x20 (--id GAP-NNNN --witness PATH, fixture must exist, curated\n\
     \x20 registry prose preserved, the generated document refreshed\n\
     \x20 in the same run); `gaps retire` removes one closed gap's\n\
     \x20 [[gap]] block from its registry file (--id, curated prose\n\
     \x20 preserved, the generated document refreshed in the same\n\
     \x20 run — closed entries retire to git history); `evidence record` curates\n\
     \x20 harness-emitted glyim.evidence/1 records into the committed\n\
     \x20 store (whole-record dedupe, canonical order; `--out PATH`,\n\
     \x20 default ledger/evidence/recorded.jsonl), and `evidence check`\n\
     \x20 validates the committed store against the registry and reports\n\
     \x20 its coverage (plan §3.4 store face); `evidence audit` recomputes\n\
     \x20 every record's fixture_sha256 against the tree and fails on\n\
     \x20 missing, drifted, or unreadable fixtures; `waivers` loads\n\
     \x20 `ledger/waivers.toml` and reports every waiver's expiry verdict —\n\
     \x20 expired waivers and load diagnostics fail the exit code (C9);\n\
     \x20 `waivers grant` appends one validated `[[waiver]]` block\n\
     \x20 (--feature/--reason/--issue/--days, deterministic\n\
     \x20 WVR-<date>-<slug> id, never a silent overwrite);\n\
     \x20 `waivers revoke` retires one waiver by its exact ID\n\
     \x20 (--id, canonical rewrite of the remaining entries,\n\
     \x20 nothing written when the document does not load cleanly);\n\
     \x20 `waivers extend` re-pins one waiver's expiry to\n\
     \x20 today + --days (default 7, horizon 14) — the sanctioned\n\
     \x20 counterpart the duplicate-grant help line points to;\n\
     \x20 `waivers prune` retires every expired waiver at once\n\
     \x20 (canonical rewrite of the remaining entries, nothing\n\
     \x20 written when the document does not load cleanly and no\n\
     \x20 rewrite at all when nothing expired);\n\
     \x20 `handoff` composes the compute, next, and waivers face\n\
     \x20 outcomes into one short session document — the readiness\n\
     \x20 histogram, the top-N work queue (`--items N`, default 5,\n\
     \x20 max 20), waiver health, and witness debt — `--out PATH`\n\
     \x20 writes the rendered document (default HANDOFF.md /\n\
     \x20 HANDOFF.json by face) and `--check` fails on drift\n\
     \x20 (plan §3.7 session handoff); `release-check <VERSION>` is\n\
     \x20 the plan §10 final release gate: every declared gate\n\
     \x20 green (unmeasured blocks — displayed, never silently\n\
     \x20 passed), evidence reproducible (the M1.13 self-check),\n\
     \x20 no waiver active or expired, and no error diagnostic\n\
     \x20 across the composed faces."
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
            | "diff" | "gates" | "verify" | "render-gaps" | "evidence" | "waivers" | "handoff"
            | "release-check",
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
            if items[0] == "evidence" {
                return parse_evidence(rest);
            }
            if items[0] == "waivers" {
                return parse_waivers(rest);
            }
            if items[0] == "handoff" {
                return parse_handoff(rest);
            }
            if items[0] == "release-check" {
                return parse_release_check(rest);
            }
            if items[0] == "gaps" {
                return parse_gaps(rest);
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
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--evidence" | "--format" | "--out" => {
                let flag = rest[index].as_str();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::StatusMissingValue {
                        flag: flag.to_string(),
                        usage: usage(),
                    });
                };
                match flag {
                    "--evidence" => evidence = Some(PathBuf::from(value)),
                    "--out" => out = Some(PathBuf::from(value)),
                    _ => {
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
    Ok(Command::Status {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        evidence,
        format,
        out,
        check,
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

/// Parse the `gaps` subcommand's arguments: the `witness`/`retire`
/// sub-words route to the gap-lifecycle write faces (the `waivers grant`
/// dispatch shape); anything else is the plain M1.7 read face — one
/// optional ROOT positional, extra positionals ignored (the historical
/// shape).
fn parse_gaps(rest: &[String]) -> Result<Command, LedgerCliError> {
    match rest.first().map(String::as_str) {
        Some("witness") => return parse_gaps_witness(&rest[1..]),
        Some("retire") => return parse_gaps_retire(&rest[1..]),
        _ => {}
    }
    let root = rest
        .first()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    Ok(Command::Gaps { root })
}

/// Parse the `gaps witness` subcommand's arguments: one optional ROOT
/// positional, the two required flags (`--id`, `--witness`), and
/// `--format <text|json>` in any order (the `waivers grant` shape).
fn parse_gaps_witness(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut id: Option<String> = None;
    let mut witness: Option<String> = None;
    let mut format = GapsFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--id" | "--witness" | "--format" => {
                let flag = rest[index].clone();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::GapsWitnessMissingField {
                        field: flag,
                        usage: usage(),
                    });
                };
                // The outer match already narrowed the flag set, so the
                // final branch is exactly `--format` — no wildcard arm.
                if flag == "--id" {
                    id = Some(value.clone());
                } else if flag == "--witness" {
                    witness = Some(value.clone());
                } else {
                    format = match value.as_str() {
                        "text" => GapsFormat::Text,
                        "json" => GapsFormat::Json,
                        other => {
                            return Err(LedgerCliError::GapsBadFormat {
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
    let Some(id) = id else {
        return Err(LedgerCliError::GapsWitnessMissingField {
            field: "--id".to_string(),
            usage: usage(),
        });
    };
    let Some(witness) = witness else {
        return Err(LedgerCliError::GapsWitnessMissingField {
            field: "--witness".to_string(),
            usage: usage(),
        });
    };
    Ok(Command::GapsWitness {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        id,
        witness,
        format,
    })
}

/// Parse the `gaps retire` subcommand's arguments: one optional ROOT
/// positional, the required `--id <GAP-ID>` flag, and `--format
/// <text|json>` in any order (the `waivers revoke` shape).
fn parse_gaps_retire(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut id: Option<String> = None;
    let mut format = GapsFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--id" | "--format" => {
                let flag = rest[index].clone();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::GapsRetireMissingField {
                        field: flag,
                        usage: usage(),
                    });
                };
                // The outer match already narrowed the flag set, so the
                // final branch is exactly `--format` — no wildcard arm.
                if flag == "--id" {
                    id = Some(value.clone());
                } else {
                    format = match value.as_str() {
                        "text" => GapsFormat::Text,
                        "json" => GapsFormat::Json,
                        other => {
                            return Err(LedgerCliError::GapsBadFormat {
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
    let Some(id) = id else {
        return Err(LedgerCliError::GapsRetireMissingField {
            field: "--id".to_string(),
            usage: usage(),
        });
    };
    Ok(Command::GapsRetire {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        id,
        format,
    })
}

/// Parse the `waivers` subcommand's arguments: one optional ROOT positional
/// and `--format <text|json>` in any order (the `gates` shape minus the
/// evidence flag — the waiver file is the only input).
fn parse_waivers(rest: &[String]) -> Result<Command, LedgerCliError> {
    // The `grant` sub-word routes to the write face (the `evidence
    // record`/`evidence check` dispatch shape).
    if rest.first().map(String::as_str) == Some("grant") {
        return parse_waivers_grant(&rest[1..]);
    }
    // The `revoke` sub-word routes to the retirement write face (the
    // same dispatch shape).
    if rest.first().map(String::as_str) == Some("revoke") {
        return parse_waivers_revoke(&rest[1..]);
    }
    // The `extend` sub-word routes to the re-pin write face (the same
    // dispatch shape).
    if rest.first().map(String::as_str) == Some("extend") {
        return parse_waivers_extend(&rest[1..]);
    }
    // The `prune` sub-word routes to the batch-retirement write face (the
    // same dispatch shape).
    if rest.first().map(String::as_str) == Some("prune") {
        return parse_waivers_prune(&rest[1..]);
    }
    let mut root: Option<PathBuf> = None;
    let mut format = WaiversFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--format" => {
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::WaiversMissingValue {
                        flag: rest[index].clone(),
                        usage: usage(),
                    });
                };
                format = match value.as_str() {
                    "text" => WaiversFormat::Text,
                    "json" => WaiversFormat::Json,
                    other => {
                        return Err(LedgerCliError::WaiversBadFormat {
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
    Ok(Command::Waivers {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        format,
    })
}

/// Parse the `waivers grant` subcommand's arguments: one optional ROOT
/// positional, the three required flags (`--feature`, `--reason`,
/// `--issue`), the optional `--days <N>` horizon, and `--format
/// <text|json>` in any order (the `evidence record` shape).
fn parse_waivers_grant(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut feature: Option<String> = None;
    let mut reason: Option<String> = None;
    let mut issue: Option<String> = None;
    let mut days: Option<i64> = None;
    let mut format = WaiversFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--feature" | "--reason" | "--issue" | "--days" | "--format" => {
                let flag = rest[index].clone();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::WaiversGrantMissingField {
                        field: flag,
                        usage: usage(),
                    });
                };
                // The outer match already narrowed the flag set, so the
                // final branch is exactly `--format` — no wildcard arm.
                if flag == "--feature" {
                    feature = Some(value.clone());
                } else if flag == "--reason" {
                    reason = Some(value.clone());
                } else if flag == "--issue" {
                    issue = Some(value.clone());
                } else if flag == "--days" {
                    let Ok(parsed) = value.parse::<i64>() else {
                        return Err(LedgerCliError::WaiversGrantBadDays {
                            days: value.clone(),
                            horizon: diff::WAIVER_MAX_AGE_DAYS,
                        });
                    };
                    days = Some(parsed);
                } else {
                    format = match value.as_str() {
                        "text" => WaiversFormat::Text,
                        "json" => WaiversFormat::Json,
                        other => {
                            return Err(LedgerCliError::WaiversBadFormat {
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
    let Some(feature) = feature else {
        return Err(LedgerCliError::WaiversGrantMissingField {
            field: "--feature".to_string(),
            usage: usage(),
        });
    };
    let Some(reason) = reason else {
        return Err(LedgerCliError::WaiversGrantMissingField {
            field: "--reason".to_string(),
            usage: usage(),
        });
    };
    let Some(issue) = issue else {
        return Err(LedgerCliError::WaiversGrantMissingField {
            field: "--issue".to_string(),
            usage: usage(),
        });
    };
    Ok(Command::WaiversGrant {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        feature,
        reason,
        issue,
        days,
        format,
    })
}

/// Parse the `handoff` subcommand's arguments: one optional ROOT
/// positional, then `--items <N>`, `--format <md|json>`, `--out <PATH>`,
/// and the boolean `--check` in any order (the `status` shape).
fn parse_handoff(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut items: Option<usize> = None;
    let mut format = HandoffFormat::Markdown;
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--items" | "--out" | "--format" => {
                let flag = rest[index].clone();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::HandoffMissingValue {
                        flag,
                        usage: usage(),
                    });
                };
                match flag.as_str() {
                    "--items" => {
                        let parsed: usize =
                            value.parse().map_err(|_| LedgerCliError::HandoffBadItems {
                                value: value.clone(),
                                usage: usage(),
                            })?;
                        if parsed == 0 || parsed > handoff::HANDOFF_MAX_ITEMS {
                            return Err(LedgerCliError::HandoffBadItems {
                                value: value.clone(),
                                usage: usage(),
                            });
                        }
                        items = Some(parsed);
                    }
                    "--out" => out = Some(PathBuf::from(value)),
                    // The final branch is exactly --format: no wildcard arm.
                    _ => {
                        format = match value.as_str() {
                            "md" => HandoffFormat::Markdown,
                            "json" => HandoffFormat::Json,
                            other => {
                                return Err(LedgerCliError::HandoffBadFormat {
                                    value: other.to_string(),
                                    usage: usage(),
                                });
                            }
                        };
                    }
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
    Ok(Command::Handoff {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        items,
        format,
        out,
        check,
    })
}

/// Parse the `release-check` subcommand's arguments: one required
/// `<VERSION>` positional (validated at parse time), then one optional
/// ROOT positional and `--format <text|json>` in any order (the `gates`
/// shape plus the version positional).
fn parse_release_check(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut version: Option<ReleaseVersion> = None;
    let mut root: Option<PathBuf> = None;
    let mut format = ReleaseCheckFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--format" => {
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::ReleaseCheckMissingValue {
                        flag: "--format".to_string(),
                        usage: usage(),
                    });
                };
                format = match value.as_str() {
                    "text" => ReleaseCheckFormat::Text,
                    "json" => ReleaseCheckFormat::Json,
                    other => {
                        return Err(LedgerCliError::ReleaseCheckBadFormat {
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
                // The first positional is the required release version
                // (validated here so a typo'd triple never reaches the
                // engine), the second is ROOT.
                if version.is_none() {
                    let Some(parsed) = ReleaseVersion::parse(positional) else {
                        return Err(LedgerCliError::ReleaseCheckBadVersion {
                            value: positional.to_string(),
                        });
                    };
                    version = Some(parsed);
                } else if root.is_some() {
                    return Err(LedgerCliError::ExtraArgument {
                        arg: positional.to_string(),
                        usage: usage(),
                    });
                } else {
                    root = Some(PathBuf::from(positional));
                }
                index += 1;
            }
        }
    }
    let Some(version) = version else {
        return Err(LedgerCliError::ReleaseCheckMissingVersion { usage: usage() });
    };
    Ok(Command::ReleaseCheck {
        version,
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        format,
    })
}

/// Parse the `waivers revoke` subcommand's arguments: one optional ROOT
/// positional, the required `--id <WVR-ID>` flag, and `--format
/// <text|json>` in any order (the `waivers grant` shape).
fn parse_waivers_revoke(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut id: Option<String> = None;
    let mut format = WaiversFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--id" | "--format" => {
                let flag = rest[index].clone();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::WaiversRevokeMissingField {
                        field: flag,
                        usage: usage(),
                    });
                };
                match flag.as_str() {
                    "--id" => id = Some(value.clone()),
                    // The final branch is exactly --format: no wildcard arm.
                    _ => {
                        format = match value.as_str() {
                            "text" => WaiversFormat::Text,
                            "json" => WaiversFormat::Json,
                            other => {
                                return Err(LedgerCliError::WaiversBadFormat {
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
    let Some(id) = id else {
        return Err(LedgerCliError::WaiversRevokeMissingField {
            field: "--id".to_string(),
            usage: usage(),
        });
    };
    Ok(Command::WaiversRevoke {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        id,
        format,
    })
}

/// Parse the `waivers extend` subcommand's arguments: one optional ROOT
/// positional, the required `--id <WVR-ID>` flag, the optional
/// `--days <N>` horizon (resolved to [`diff::DEFAULT_WAIVER_DAYS`] by the
/// dispatch layer when omitted), and `--format <text|json>` in any order
/// (the `waivers revoke` shape).
fn parse_waivers_extend(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut id: Option<String> = None;
    let mut days: Option<i64> = None;
    let mut format = WaiversFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--id" | "--days" | "--format" => {
                let flag = rest[index].clone();
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::WaiversExtendMissingField {
                        field: flag,
                        usage: usage(),
                    });
                };
                if flag == "--id" {
                    id = Some(value.clone());
                } else if flag == "--days" {
                    let Ok(parsed) = value.parse::<i64>() else {
                        return Err(LedgerCliError::WaiversExtendBadDays {
                            days: value.clone(),
                            horizon: diff::WAIVER_MAX_AGE_DAYS,
                        });
                    };
                    days = Some(parsed);
                } else {
                    format = match value.as_str() {
                        "text" => WaiversFormat::Text,
                        "json" => WaiversFormat::Json,
                        other => {
                            return Err(LedgerCliError::WaiversBadFormat {
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
    let Some(id) = id else {
        return Err(LedgerCliError::WaiversExtendMissingField {
            field: "--id".to_string(),
            usage: usage(),
        });
    };
    Ok(Command::WaiversExtend {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        id,
        days,
        format,
    })
}

/// Parse the `waivers prune` subcommand's arguments: one optional ROOT
/// positional and `--format <text|json>` in any order (the plain `waivers`
/// shape — the batch face takes no ID: it retires every expired entry).
fn parse_waivers_prune(rest: &[String]) -> Result<Command, LedgerCliError> {
    let mut root: Option<PathBuf> = None;
    let mut format = WaiversFormat::Text;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--format" => {
                let Some(value) = rest.get(index + 1) else {
                    return Err(LedgerCliError::WaiversMissingValue {
                        flag: rest[index].clone(),
                        usage: usage(),
                    });
                };
                format = match value.as_str() {
                    "text" => WaiversFormat::Text,
                    "json" => WaiversFormat::Json,
                    other => {
                        return Err(LedgerCliError::WaiversBadFormat {
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
    Ok(Command::WaiversPrune {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
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

/// Parse the `evidence` subcommand's arguments: the second word selects
/// the face — `record [--out PATH] [--format text|json] <SRC...>` (the
/// registry root is the working directory, mirroring every other command's
/// default; the library entry point takes an explicit root) and
/// `check [ROOT] [--format text|json]`.
fn parse_evidence(rest: &[String]) -> Result<Command, LedgerCliError> {
    let Some(kind) = rest.first() else {
        return Err(LedgerCliError::EvidenceSubcommand {
            subcommand: "<missing>".to_string(),
            usage: usage(),
        });
    };
    let body = &rest[1..];
    match kind.as_str() {
        "record" => {
            let mut sources: Vec<PathBuf> = Vec::new();
            let mut out: Option<PathBuf> = None;
            let mut format = EvidenceFormat::Text;
            let mut index = 0;
            while index < body.len() {
                match body[index].as_str() {
                    "--out" | "--format" => {
                        let flag = body[index].as_str();
                        let Some(value) = body.get(index + 1) else {
                            return Err(LedgerCliError::EvidenceMissingValue {
                                flag: flag.to_string(),
                                usage: usage(),
                            });
                        };
                        if flag == "--out" {
                            out = Some(PathBuf::from(value));
                        } else {
                            format = match value.as_str() {
                                "text" => EvidenceFormat::Text,
                                "json" => EvidenceFormat::Json,
                                other => {
                                    return Err(LedgerCliError::EvidenceBadFormat {
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
                        sources.push(PathBuf::from(positional));
                        index += 1;
                    }
                }
            }
            if sources.is_empty() {
                return Err(LedgerCliError::EvidenceMissingSources { usage: usage() });
            }
            Ok(Command::EvidenceRecord {
                sources,
                out: out.unwrap_or_else(|| PathBuf::from(evidence::DEFAULT_RECORD_TARGET)),
                format,
            })
        }
        "check" => {
            let mut root: Option<PathBuf> = None;
            let mut format = EvidenceFormat::Text;
            let mut index = 0;
            while index < body.len() {
                match body[index].as_str() {
                    "--format" => {
                        let Some(value) = body.get(index + 1) else {
                            return Err(LedgerCliError::EvidenceMissingValue {
                                flag: "--format".to_string(),
                                usage: usage(),
                            });
                        };
                        format = match value.as_str() {
                            "text" => EvidenceFormat::Text,
                            "json" => EvidenceFormat::Json,
                            other => {
                                return Err(LedgerCliError::EvidenceBadFormat {
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
            Ok(Command::EvidenceCheck {
                root: root.unwrap_or_else(|| PathBuf::from(".")),
                format,
            })
        }
        "audit" => {
            let mut root: Option<PathBuf> = None;
            let mut format = EvidenceFormat::Text;
            let mut index = 0;
            while index < body.len() {
                match body[index].as_str() {
                    "--format" => {
                        let Some(value) = body.get(index + 1) else {
                            return Err(LedgerCliError::EvidenceMissingValue {
                                flag: "--format".to_string(),
                                usage: usage(),
                            });
                        };
                        format = match value.as_str() {
                            "text" => EvidenceFormat::Text,
                            "json" => EvidenceFormat::Json,
                            other => {
                                return Err(LedgerCliError::EvidenceBadFormat {
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
            Ok(Command::EvidenceAudit {
                root: root.unwrap_or_else(|| PathBuf::from(".")),
                format,
            })
        }
        other => Err(LedgerCliError::EvidenceSubcommand {
            subcommand: other.to_string(),
            usage: usage(),
        }),
    }
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
                out: None,
                check: false,
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
                out: None,
                check: false,
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
