//! `ledger waivers` — the constitution-C9 waiver-health face.
//!
//! Constitution C9 ("gates never weaken") grants temporary exceptions
//! through `ledger/waivers.toml`: every waiver carries an issue link and
//! an expiry inside the 14-day horizon. Until this face existed, that
//! file was read only inside the [`crate::diff`] regression gate — an
//! expired or horizon-violating waiver stayed invisible until a
//! regression actually needed covering, and there was no way to inspect
//! the live exception inventory at all. This module closes that gap: the
//! waiver health is its own report, so CI can fail an expired exception
//! even on a day when nothing regressed (the C9 sentence "expired
//! waivers fail CI", checkable directly).
//!
//! # The seams
//!
//! - [`WaiverSource`] abstracts where the [`WaiverSet`] comes from:
//!   [`FileWaiverSource`] reads `<ROOT>/ledger/waivers.toml` through the
//!   same [`load_waivers`] loader and diagnostics the `diff` gate uses
//!   (exactly one definition of "what the waiver file says"), and
//!   [`InMemoryWaiverSource`] serves a caller-built set for tests and
//!   programmatic consumers. New sources plug in as one more
//!   `WaiverSource` impl, never as outcome fields.
//! - [`WaiversRenderer`] abstracts the document face:
//!   [`TextWaiversRenderer`] produces the human-readable report and
//!   [`JsonWaiversRenderer`] the `glyim.ledger-waivers/1` JSON contract.
//!   Both consume the same [`WaiversOutcome`], so there is exactly one
//!   definition of "what the waiver inventory says".
//!
//! # Determinism (plan C1/M1.13)
//!
//! No clock, no environment, no iteration-order dependence: `today` is a
//! value the caller pins (`run_waivers` consults the wall clock once),
//! rows render in ID order (the order [`load_waivers`] establishes), and
//! rendering twice yields identical bytes (pinned by golden tests).
//!
//! # Gate semantics
//!
//! [`WaiversOutcome::is_ok`] is the C9 gate: no error-severity load
//! diagnostic (schema, duplicate ID, horizon violation — the same codes
//! `diff` emits) **and** no row whose verdict is
//! [`WaiverVerdict::Expired`]. A waiver inside its horizon but close to
//! expiry is report content, never a failure: the exception is still
//! live, and the remaining-days column is the operator's reminder.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ledger_schema::{Diagnostic, Severity, load_project_registry};
use serde::Serialize;

use crate::diff::{
    NaiveDate, WAIVER_MAX_AGE_DAYS, WaiverEntry, WaiverSet, load_waivers, today_iso,
};
use crate::{LedgerCliError, usage};

/// Schema tag of the waiver-health JSON contract.
pub const WAIVERS_SCHEMA: &str = "glyim.ledger-waivers/1";

/// The expiry verdict the engine computed for one waiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WaiverVerdict {
    /// The waiver is active: its expiry is today or later.
    Valid,
    /// The waiver is past its expiry: it can no longer cover a
    /// regression (C9: expired waivers fail CI).
    Expired,
}

impl WaiverVerdict {
    /// Canonical wire and table spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Expired => "expired",
        }
    }
}

/// One loaded waiver with its computed expiry verdict (the JSON
/// contract's entry shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiverRow {
    /// Unique waiver ID (as declared in `waivers.toml`).
    pub id: String,
    /// The registry feature ID whose regressions this waiver covers.
    pub feature: String,
    /// Why the exception exists (rendered in reports).
    pub reason: String,
    /// Tracking link (the plan requires an issue link per waiver).
    pub issue: String,
    /// Expiry date in the `YYYY-MM-DD` ISO spelling (echoed verbatim).
    pub expires: String,
    /// The computed expiry verdict.
    pub verdict: WaiverVerdict,
    /// Signed whole-day distance from the assessment date: positive when
    /// days remain, zero on the expiry day, negative when overdue.
    pub remaining_days: i64,
}

/// Result of a full `ledger waivers` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaiversOutcome {
    /// The date the assessment ran against (pinned by the caller).
    pub today: NaiveDate,
    /// Load diagnostics (schema, duplicate ID, horizon violation), sorted
    /// and de-duplicated — the same diagnostics `diff` would emit.
    pub diagnostics: Vec<Diagnostic>,
    /// One row per loaded, date-valid waiver, sorted by ID.
    pub rows: Vec<WaiverRow>,
    /// Waivers loaded and assessed (equal to [`Self::rows`] length).
    pub waivers: usize,
    /// Rows whose verdict is [`WaiverVerdict::Valid`].
    pub valid: usize,
    /// Rows whose verdict is [`WaiverVerdict::Expired`].
    pub expired: usize,
}

impl WaiversOutcome {
    /// Number of error-severity load diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// True when the C9 gate passes: no error-severity diagnostic and no
    /// expired row. A verdict is gate content, not prose — an expired
    /// exception must fail CI even when no regression needs covering.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0 && self.expired == 0
    }
}

// -- source seam ------------------------------------------------------------

/// Where the [`WaiverSet`] comes from. The load seam mirrors
/// [`crate::SnapshotStore`] and [`crate::EvidenceSource`]: the engine
/// consumes the trait, so tests inject sets directly and the CLI wires
/// the file-backed implementation.
pub trait WaiverSource {
    /// Load the waiver set for the project rooted at `root`, validated
    /// against the C9 horizon at `today`.
    fn load(&self, root: &Path, today: NaiveDate) -> WaiverSet;
}

/// Reads `<ROOT>/ledger/waivers.toml` through [`load_waivers`] — the
/// same loader and diagnostic codes the `diff` gate uses, so the two
/// faces can never disagree about what the file says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileWaiverSource;

impl WaiverSource for FileWaiverSource {
    fn load(&self, root: &Path, today: NaiveDate) -> WaiverSet {
        load_waivers(&root.join("ledger/waivers.toml"), today)
    }
}

/// Serves a caller-built [`WaiverSet`] (tests, programmatic consumers);
/// the in-memory twin of [`crate::InMemorySnapshotStore`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InMemoryWaiverSource {
    /// The set every `load` call returns verbatim.
    pub set: WaiverSet,
}

impl WaiverSource for InMemoryWaiverSource {
    fn load(&self, _root: &Path, _today: NaiveDate) -> WaiverSet {
        self.set.clone()
    }
}

// -- engine -------------------------------------------------------------------

/// Computes the expiry verdict per loaded waiver (the pure function the
/// renderers consume).
///
/// Entries whose `expires` spelling does not parse carry no verdict —
/// they are skipped here and the load diagnostics (which
/// [`load_waivers`] emits for every unparseable date) are the
/// operator-facing report, the same contract the `diff` gate applies.
#[must_use]
pub fn assess_waivers(set: &WaiverSet, today: NaiveDate) -> WaiversOutcome {
    let today_days = today.to_days();
    let mut rows = Vec::with_capacity(set.waivers.len());
    let mut expired = 0usize;
    for waiver in &set.waivers {
        let Some(expires) = NaiveDate::parse_iso(&waiver.expires) else {
            continue;
        };
        let remaining_days = expires.to_days() - today_days;
        let verdict = if remaining_days >= 0 {
            WaiverVerdict::Valid
        } else {
            WaiverVerdict::Expired
        };
        if verdict == WaiverVerdict::Expired {
            expired += 1;
        }
        rows.push(WaiverRow {
            id: waiver.id.clone(),
            feature: waiver.feature.clone(),
            reason: waiver.reason.clone(),
            issue: waiver.issue.clone(),
            expires: waiver.expires.clone(),
            verdict,
            remaining_days,
        });
    }
    // Rows render in ID order regardless of the set's construction path
    // (`load_waivers` pre-sorts; hand-built sets via the in-memory source
    // are normalized here), so the face is deterministic for every source.
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    // `load_waivers` already sorts and de-duplicates; the defensive
    // re-normalization keeps hand-built sets (the in-memory source) on the
    // same deterministic contract.
    let mut diagnostics = set.diagnostics.clone();
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    let waivers = rows.len();
    let valid = waivers - expired;
    WaiversOutcome {
        today,
        diagnostics,
        rows,
        waivers,
        valid,
        expired,
    }
}

// -- executor -----------------------------------------------------------------

/// Runs a full `ledger waivers` pass over `root` through `source`: loads
/// the waiver set, pins `today` once, and assesses every entry.
#[must_use]
pub fn run_waivers(root: &Path, source: &dyn WaiverSource) -> WaiversOutcome {
    let span = tracing::info_span!("waivers", root = %root.display());
    let _enter = span.enter();

    // `today_iso` renders the `YYYY-MM-DD` spelling `parse_iso` accepts;
    // the round-trip is pinned by the diff-engine calendar tests, and an
    // unreachable fallback would silently shift every verdict to the
    // epoch, so it is a loud bug instead.
    let today =
        NaiveDate::parse_iso(&today_iso()).expect("today_iso round-trips through parse_iso");
    let set = source.load(root, today);
    let outcome = assess_waivers(&set, today);
    tracing::debug!(
        waivers = outcome.waivers,
        expired = outcome.expired,
        diagnostics = outcome.diagnostics.len(),
        "waiver health assessed"
    );
    outcome
}

// -- renderers ----------------------------------------------------------------

/// Renders a [`WaiversOutcome`] into a document face.
///
/// The trait is the renderer seam (the [`crate::KnownGapsRenderer`]
/// pattern): implement it to add a face without touching the load
/// pipeline or the CLI wiring.
pub trait WaiversRenderer {
    /// Render the outcome into the target document (bytes are a pure
    /// function of the outcome — deterministic golden output).
    fn render(&self, outcome: &WaiversOutcome) -> String;
}

/// The human-readable report face (the [`crate::format_gates_text`]
/// shape: summary line, one header line plus indented detail lines per
/// row, diagnostics last).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextWaiversRenderer;

/// The JSON document rendered by [`JsonWaiversRenderer`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiversDocument {
    /// Schema tag, always [`WAIVERS_SCHEMA`].
    pub schema: &'static str,
    /// The assessment date (`YYYY-MM-DD`).
    pub today: String,
    /// Waivers loaded and assessed.
    pub waivers: usize,
    /// Rows whose verdict is [`WaiverVerdict::Valid`].
    pub valid: usize,
    /// Rows whose verdict is [`WaiverVerdict::Expired`].
    pub expired: usize,
    /// Whether the C9 gate passed ([`WaiversOutcome::is_ok`]); diagnostics
    /// are deliberately excluded — they are operator-facing prose (the
    /// text renderer's job), the same split as the `glyim.ledger-diff/1`
    /// document.
    pub passed: bool,
    /// One row per loaded waiver, sorted by ID.
    pub rows: Vec<WaiverRow>,
}

/// The `glyim.ledger-waivers/1` JSON face.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JsonWaiversRenderer;

impl WaiversRenderer for TextWaiversRenderer {
    fn render(&self, outcome: &WaiversOutcome) -> String {
        let mut text = String::new();
        let _ = writeln!(
            text,
            "waivers: {} loaded, {} valid, {} expired, {} diagnostic(s), \
             assessed at {}",
            outcome.waivers,
            outcome.valid,
            outcome.expired,
            outcome.diagnostics.len(),
            outcome.today.to_iso(),
        );
        for row in &outcome.rows {
            let _ = writeln!(
                text,
                "  {:<26} {:<10} {:<10} expires {}",
                row.id,
                row.verdict.as_str(),
                format!("{}d", row.remaining_days),
                row.expires,
            );
            let _ = writeln!(text, "         feature: {}", row.feature);
            let _ = writeln!(text, "         reason: {}", row.reason);
            let _ = writeln!(text, "         issue: {}", row.issue);
        }
        for diagnostic in &outcome.diagnostics {
            let _ = writeln!(text, "{diagnostic}");
        }
        text
    }
}

impl WaiversRenderer for JsonWaiversRenderer {
    fn render(&self, outcome: &WaiversOutcome) -> String {
        let document = WaiversDocument {
            schema: WAIVERS_SCHEMA,
            today: outcome.today.to_iso(),
            waivers: outcome.waivers,
            valid: outcome.valid,
            expired: outcome.expired,
            passed: outcome.is_ok(),
            rows: outcome.rows.clone(),
        };
        let mut text = serde_json::to_string_pretty(&document)
            .expect("the glyim.ledger-waivers/1 document is a serialisation-only struct");
        text.push('\n');
        text
    }
}

/// Renders the text waiver-health report (the [`TextWaiversRenderer`]
/// face).
#[must_use]
pub fn format_waivers_text(outcome: &WaiversOutcome) -> String {
    TextWaiversRenderer.render(outcome)
}

/// Renders the `glyim.ledger-waivers/1` JSON contract (the
/// [`JsonWaiversRenderer`] face).
#[must_use]
pub fn format_waivers_json(outcome: &WaiversOutcome) -> String {
    JsonWaiversRenderer.render(outcome)
}

// -- grant face ---------------------------------------------------------------

/// Schema tag of the `waivers grant` JSON contract.
pub const WAIVERS_GRANT_SCHEMA: &str = "glyim.ledger-waivers-grant/1";

/// The `waivers grant` request (the CLI flags, validated by
/// [`run_waivers_grant`]): a temporary C9 exception for one registry
/// feature's regressions, with an issue link and a day-bounded expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaiversGrantRequest {
    /// The registry feature ID whose regressions this waiver covers.
    pub feature: String,
    /// Why the exception exists (rendered in reports).
    pub reason: String,
    /// Tracking link; the plan requires an `https://` issue link per
    /// waiver.
    pub issue: String,
    /// Horizon in whole days (`1..=WAIVER_MAX_AGE_DAYS`); the CLI default
    /// is 7 when `--days` is omitted.
    pub days: i64,
}

/// Where a granted `[[waiver]]` block lands. The write seam mirrors
/// [`crate::SnapshotStore`] and [`crate::EvidenceStore`]: the engine
/// consumes the trait, so tests record fragments in memory and the CLI
/// wires the file-backed implementation.
pub trait WaiverSink {
    /// Append the canonical `[[waiver]]` fragment to the document at
    /// `path` (created when absent); the error detail is the IO failure
    /// message (the CLI turns it into a typed write error).
    fn append(&self, path: &Path, fragment: &str) -> Result<(), String>;
}

/// Appends to `<ROOT>/ledger/waivers.toml` on disk (creating the `ledger/`
/// directory when it does not exist yet): the committed exception
/// registry the `diff` gate and the health face read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileWaiverSink;

impl WaiverSink for FileWaiverSink {
    fn append(&self, path: &Path, fragment: &str) -> Result<(), String> {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
        }
        // A document that does not end in a newline would glue the
        // appended block onto the last entry; normalize before writing.
        let needs_newline = std::fs::read_to_string(path)
            .map(|text| !text.is_empty() && !text.ends_with('\n'))
            .unwrap_or(false);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| format!("cannot open {}: {err}", path.display()))?;
        if needs_newline {
            file.write_all(b"\n")
                .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        }
        file.write_all(fragment.as_bytes())
            .map_err(|err| format!("cannot write {}: {err}", path.display()))
    }
}

/// Records every appended fragment (path, bytes) for tests and
/// programmatic consumers; the in-memory twin of
/// [`crate::FileSnapshotStore`].
#[derive(Debug, Default)]
pub struct InMemoryWaiverSink {
    /// Every `(path, fragment)` the sink was asked to append, in order.
    pub written: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl WaiverSink for InMemoryWaiverSink {
    fn append(&self, path: &Path, fragment: &str) -> Result<(), String> {
        self.written
            .borrow_mut()
            .push((path.to_path_buf(), fragment.to_string()));
        Ok(())
    }
}

/// Result of a `waivers grant` run (also the
/// `glyim.ledger-waivers-grant/1` JSON face via
/// [`format_waivers_grant_json`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiversGrantOutcome {
    /// Schema tag, always [`WAIVERS_GRANT_SCHEMA`].
    pub schema: &'static str,
    /// The granted waiver's deterministic ID
    /// (`WVR-<YYYYMMDD>-<feature slug>`).
    pub id: String,
    /// The registry feature ID the waiver covers.
    pub feature: String,
    /// The computed expiry (`today + days`, ISO spelling).
    pub expires: String,
    /// The granted horizon in days.
    pub days: i64,
    /// The document the fragment landed in.
    pub target: String,
    /// The exact canonical bytes appended (the audit trail).
    pub fragment: String,
}

/// The `[[waiver]]` fragment carrier: serializing a one-entry array
/// produces the canonical `[[waiver]]` TOML block (field order = struct
/// order, escaped by the `toml` crate — never hand-quoted).
#[derive(serde::Serialize)]
struct GrantFragmentDoc {
    waiver: Vec<WaiverEntry>,
}

/// Maps a registry feature ID to the ID's slug part: lowercase, every
/// non-alphanumeric run collapsed into a single `-`, no leading or
/// trailing `-` (`LANG-EVIDENCE-A` → `lang-evidence-a`).
#[must_use]
fn waiver_slug(feature: &str) -> String {
    let mut slug = String::with_capacity(feature.len());
    let mut pending_dash = false;
    for ch in feature.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    slug
}

/// Grants one C9 waiver: validates the request against the plan's rules,
/// checks the registry, composes the deterministic ID, and appends the
/// canonical block through `sink`.
///
/// # Validation contract
///
/// - `days` must be `1..=WAIVER_MAX_AGE_DAYS` (constitution C9: waivers
///   expire in ≤ 14 days);
/// - `reason` must be usable (non-blank, no control characters);
/// - `issue` must be an `https://` link (the plan requires an issue link
///   per waiver);
/// - `feature` must name a registry feature (the exception exists for a
///   feature's regressions);
/// - the deterministic ID must not already exist (a same-day re-grant is
///   a duplicate, never a silent overwrite).
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`WaiversGrantBadDays`, `WaiversGrantBadIssue`,
/// `WaiversGrantMissingField`, `WaiversGrantUnknownFeature`,
/// `WaiversGrantRegistry`, `WaiversGrantDuplicate`) and for a failed
/// append (`WaiversGrantWrite`).
pub fn run_waivers_grant(
    root: &Path,
    request: &WaiversGrantRequest,
    source: &dyn WaiverSource,
    sink: &dyn WaiverSink,
) -> Result<WaiversGrantOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "waivers_grant",
        root = %root.display(),
        feature = %request.feature,
        days = request.days,
    );
    let _enter = span.enter();

    if request.days < 1 || request.days > WAIVER_MAX_AGE_DAYS {
        return Err(LedgerCliError::WaiversGrantBadDays {
            days: request.days.to_string(),
            horizon: WAIVER_MAX_AGE_DAYS,
        });
    }
    if request.reason.trim().is_empty() || request.reason.chars().any(char::is_control) {
        return Err(LedgerCliError::WaiversGrantMissingField {
            field: "--reason".to_string(),
            usage: usage(),
        });
    }
    if !request.issue.starts_with("https://") || request.issue.len() <= "https://".len() {
        return Err(LedgerCliError::WaiversGrantBadIssue {
            issue: request.issue.clone(),
        });
    }

    // The exception is about a registry feature's regressions: the ID
    // must name a declared feature, and a registry that does not load is
    // a CLI failure (no write happens).
    let loaded = load_project_registry(root);
    if loaded
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        let detail = loaded
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LedgerCliError::WaiversGrantRegistry { detail });
    }
    if loaded.registry.feature_by_id(&request.feature).is_none() {
        let candidates = loaded
            .registry
            .features()
            .iter()
            .take(3)
            .map(|sourced| sourced.entry.id.clone())
            .collect::<Vec<_>>();
        return Err(LedgerCliError::WaiversGrantUnknownFeature {
            feature: request.feature.clone(),
            candidates,
        });
    }

    // `today_iso` renders the `YYYY-MM-DD` spelling `parse_iso` accepts;
    // the round-trip is pinned by the diff-engine calendar tests (the
    // same pin the health face relies on).
    let today =
        NaiveDate::parse_iso(&today_iso()).expect("today_iso round-trips through parse_iso");
    let id = format!(
        "WVR-{}{:02}{:02}-{}",
        today.year,
        today.month,
        today.day,
        waiver_slug(&request.feature),
    );
    let set = source.load(root, today);
    if set.waivers.iter().any(|w| w.id == id) {
        return Err(LedgerCliError::WaiversGrantDuplicate {
            id: id.clone(),
            path: root.join("ledger/waivers.toml").display().to_string(),
        });
    }

    let entry = WaiverEntry {
        id: id.clone(),
        feature: request.feature.clone(),
        reason: request.reason.clone(),
        issue: request.issue.clone(),
        expires: today.plus_days(request.days).to_iso(),
    };
    let fragment = toml::to_string(&GrantFragmentDoc {
        waiver: vec![entry],
    })
    .map_err(|err| LedgerCliError::WaiversGrantWrite {
        path: root.join("ledger/waivers.toml").display().to_string(),
        detail: err.to_string(),
    })?;
    let path = root.join("ledger/waivers.toml");
    sink.append(&path, &fragment)
        .map_err(|detail| LedgerCliError::WaiversGrantWrite {
            path: path.display().to_string(),
            detail,
        })?;
    let expires = today.plus_days(request.days).to_iso();
    tracing::debug!(id = %id, expires = %expires, "waiver granted");

    Ok(WaiversGrantOutcome {
        schema: WAIVERS_GRANT_SCHEMA,
        id,
        feature: request.feature.clone(),
        expires,
        days: request.days,
        target: path.display().to_string(),
        fragment,
    })
}

/// Renders the grant outcome's text face: one summary line plus the
/// canonical appended block (the audit trail).
#[must_use]
pub fn format_waivers_grant_text(outcome: &WaiversGrantOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "granted {} for {} ({} day(s), expires {}) -> {}",
        outcome.id, outcome.feature, outcome.days, outcome.expires, outcome.target,
    );
    text.push_str(&outcome.fragment);
    text
}

/// Renders the `glyim.ledger-waivers-grant/1` JSON contract.
#[must_use]
pub fn format_waivers_grant_json(outcome: &WaiversGrantOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-waivers-grant/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

// -- revoke face ---------------------------------------------------------------

/// Schema tag of the `waivers revoke` JSON contract.
pub const WAIVERS_REVOKE_SCHEMA: &str = "glyim.ledger-waivers-revoke/1";

/// The `[[waiver]]` document carrier for the rewriter: parsing the
/// committed `ledger/waivers.toml` into the entry array and
/// re-serializing the remaining entries produces the canonical document
/// (field order = struct order, escaped by the `toml` crate — the same
/// discipline [`GrantFragmentDoc`] applies on the write side).
#[derive(serde::Serialize, serde::Deserialize)]
struct RevokeSourceDoc {
    waiver: Vec<WaiverEntry>,
}

/// The `waivers revoke` request (the CLI flags, validated by
/// [`run_waivers_revoke`]): retire one waiver by its exact ID — the
/// counterpart of `waivers grant` for exceptions whose regression fix
/// landed (or whose horizon already expired; constitution C9 keeps the
/// file clean, and until this face there was no sanctioned way to remove
/// an entry at all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaiversRevokeRequest {
    /// The exact waiver ID (`WVR-...`) to remove.
    pub id: String,
}

/// Where a revoked `[[waiver]]` block is removed from. The rewrite seam
/// mirrors [`WaiverSink`] on the write side: the engine consumes the
/// trait, so tests drive an in-memory document and the CLI wires the
/// file-backed implementation.
pub trait WaiverRewriter {
    /// Removes the `[[waiver]]` block carrying the exact `id` from the
    /// document at `path` and rewrites the remaining entries canonically.
    /// Returns the removed entry, or `None` when the document no longer
    /// carries it; the error detail is the IO or serialization failure
    /// message (the CLI turns it into a typed write error).
    fn remove(&self, path: &Path, id: &str) -> Result<Option<WaiverEntry>, String>;
}

/// Rewrites `<ROOT>/ledger/waivers.toml` on disk: parse, drop the one
/// matching block, re-serialize the remaining entries canonically, and
/// replace the file. The rewrite never silently reorders or drops other
/// entries — a document that fails to parse is refused (`Err`), not
/// truncated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileWaiverRewriter;

impl WaiverRewriter for FileWaiverRewriter {
    fn remove(&self, path: &Path, id: &str) -> Result<Option<WaiverEntry>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let doc: RevokeSourceDoc = toml::from_str(&text)
            .map_err(|err| format!("cannot parse {}: {err}", path.display()))?;
        let Some(position) = doc.waiver.iter().position(|entry| entry.id == id) else {
            return Ok(None);
        };
        let removed = doc.waiver[position].clone();
        let mut remaining = doc.waiver;
        remaining.remove(position);
        let rewritten = toml::to_string(&RevokeSourceDoc { waiver: remaining })
            .map_err(|err| format!("cannot serialize {}: {err}", path.display()))?;
        std::fs::write(path, &rewritten)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(Some(removed))
    }
}

/// Operates on an owned in-memory document (tests and programmatic
/// consumers); the in-memory twin of [`FileWaiverRewriter`]. Every
/// successful removal is recorded in [`Self::removed`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryWaiverRewriter {
    /// The document the rewriter operates on (the file's entries).
    pub doc: std::cell::RefCell<Vec<WaiverEntry>>,
    /// Every `(path, removed ID)` performed, in order.
    pub removed: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl WaiverRewriter for InMemoryWaiverRewriter {
    fn remove(&self, path: &Path, id: &str) -> Result<Option<WaiverEntry>, String> {
        let mut doc = self.doc.borrow_mut();
        if let Some(position) = doc.iter().position(|entry| entry.id == id) {
            let removed = doc.remove(position);
            self.removed
                .borrow_mut()
                .push((path.to_path_buf(), id.to_string()));
            Ok(Some(removed))
        } else {
            Ok(None)
        }
    }
}

/// Result of a `waivers revoke` run (also the
/// `glyim.ledger-waivers-revoke/1` JSON face via
/// [`format_waivers_revoke_json`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiversRevokeOutcome {
    /// Schema tag, always [`WAIVERS_REVOKE_SCHEMA`].
    pub schema: &'static str,
    /// The revoked waiver's ID.
    pub id: String,
    /// The registry feature ID the revoked waiver covered.
    pub feature: String,
    /// The revoked waiver's expiry (`YYYY-MM-DD`, echoed verbatim).
    pub expires: String,
    /// Waivers remaining in the committed document after the removal.
    pub remaining: usize,
    /// The document the block was removed from.
    pub target: String,
}

/// Revokes one C9 waiver: validates the request, refuses to touch a
/// document that does not load cleanly, and removes exactly the one
/// matching block through `rewriter`.
///
/// # Validation contract
///
/// - `id` must be usable (non-blank, no control characters);
/// - the committed document must load with zero error-severity
///   diagnostics (the rewriter refuses to rewrite what it cannot fully
///   parse — a duplicate-ID file fails here, never silently);
/// - the ID must be present (an unknown ID writes nothing and names a
///   few existing candidates for the help line).
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`WaiversRevokeBadId`, `WaiversRevokeUnknownId`, `WaiversRevokeLoad`)
/// and for a failed rewrite (`WaiversRevokeWrite`).
pub fn run_waivers_revoke(
    root: &Path,
    request: &WaiversRevokeRequest,
    source: &dyn WaiverSource,
    rewriter: &dyn WaiverRewriter,
) -> Result<WaiversRevokeOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "waivers_revoke",
        root = %root.display(),
        id = %request.id,
    );
    let _enter = span.enter();

    if request.id.trim().is_empty() || request.id.chars().any(char::is_control) {
        return Err(LedgerCliError::WaiversRevokeBadId {
            id: request.id.clone(),
        });
    }

    // `today_iso` renders the `YYYY-MM-DD` spelling `parse_iso` accepts;
    // the same pinned round-trip the grant face relies on.
    let today =
        NaiveDate::parse_iso(&today_iso()).expect("today_iso round-trips through parse_iso");
    let set = source.load(root, today);
    if set
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        let detail = set
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LedgerCliError::WaiversRevokeLoad { detail });
    }
    if !set.waivers.iter().any(|w| w.id == request.id) {
        let candidates = set
            .waivers
            .iter()
            .take(3)
            .map(|w| w.id.clone())
            .collect::<Vec<_>>();
        return Err(LedgerCliError::WaiversRevokeUnknownId {
            id: request.id.clone(),
            candidates,
        });
    }

    let path = root.join("ledger/waivers.toml");
    let removed = rewriter.remove(&path, &request.id).map_err(|detail| {
        LedgerCliError::WaiversRevokeWrite {
            path: path.display().to_string(),
            detail,
        }
    })?;
    let Some(removed) = removed else {
        // The loaded set carried the ID but the document no longer does:
        // the registry drifted between the load pass and the rewrite.
        // Fail without a partial write.
        return Err(LedgerCliError::WaiversRevokeUnknownId {
            id: request.id.clone(),
            candidates: Vec::new(),
        });
    };
    let remaining = set.waivers.len().saturating_sub(1);
    tracing::debug!(id = %removed.id, remaining, "waiver revoked");

    Ok(WaiversRevokeOutcome {
        schema: WAIVERS_REVOKE_SCHEMA,
        id: removed.id,
        feature: removed.feature,
        expires: removed.expires,
        remaining,
        target: path.display().to_string(),
    })
}

/// Renders the revoke outcome's text face: one summary line (the
/// removed block's ID, feature, expiry, and the remaining count).
#[must_use]
pub fn format_waivers_revoke_text(outcome: &WaiversRevokeOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "revoked {} for {} (expires {}) -> {} waiver(s) remain at {}",
        outcome.id, outcome.feature, outcome.expires, outcome.remaining, outcome.target,
    );
    text
}

/// Renders the `glyim.ledger-waivers-revoke/1` JSON contract.
#[must_use]
pub fn format_waivers_revoke_json(outcome: &WaiversRevokeOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-waivers-revoke/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

// -- extend face ---------------------------------------------------------------

/// Schema tag of the `waivers extend` JSON contract.
pub const WAIVERS_EXTEND_SCHEMA: &str = "glyim.ledger-waivers-extend/1";

/// The `waivers extend` request (the CLI flags, validated by
/// [`run_waivers_extend`]): re-pin one existing waiver's expiry to
/// `today + days`, keeping the entry's identity, reason, and issue — the
/// sanctioned counterpart of the duplicate-grant rejection, whose help
/// line already instructs operators to "extend the existing entry's
/// expiry instead of re-granting it".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaiversExtendRequest {
    /// The exact waiver ID (`WVR-...`) whose expiry moves.
    pub id: String,
    /// The new horizon in whole days from today
    /// (`1..=WAIVER_MAX_AGE_DAYS`); the CLI default is
    /// [`crate::DEFAULT_WAIVER_DAYS`] when `--days` is omitted.
    pub days: i64,
}

/// Where an extended `[[waiver]]` block is rewritten. The mutation seam
/// mirrors [`WaiverRewriter`] on the retirement side: the engine consumes
/// the trait, so tests drive an in-memory document and the CLI wires the
/// file-backed implementation.
pub trait WaiverExtender {
    /// Moves the `expires` field of the `[[waiver]]` block carrying the
    /// exact `id` to `expires` and rewrites the remaining entries
    /// canonically. Returns the updated entry, or `None` when the
    /// document no longer carries it; the error detail is the IO or
    /// serialization failure message (the CLI turns it into a typed write
    /// error).
    fn extend_expiry(
        &self,
        path: &Path,
        id: &str,
        expires: &str,
    ) -> Result<Option<WaiverEntry>, String>;
}

/// Rewrites `<ROOT>/ledger/waivers.toml` on disk: parse, move the one
/// matching block's expiry, re-serialize the remaining entries
/// canonically, and replace the file. The rewrite never silently
/// reorders or drops other entries — a document that fails to parse is
/// refused (`Err`), not truncated (the [`FileWaiverRewriter`] contract).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileWaiverExtender;

impl WaiverExtender for FileWaiverExtender {
    fn extend_expiry(
        &self,
        path: &Path,
        id: &str,
        expires: &str,
    ) -> Result<Option<WaiverEntry>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let doc: RevokeSourceDoc = toml::from_str(&text)
            .map_err(|err| format!("cannot parse {}: {err}", path.display()))?;
        let Some(position) = doc.waiver.iter().position(|entry| entry.id == id) else {
            return Ok(None);
        };
        let mut remaining = doc.waiver;
        remaining[position].expires = expires.to_string();
        let updated = remaining[position].clone();
        let rewritten = toml::to_string(&RevokeSourceDoc { waiver: remaining })
            .map_err(|err| format!("cannot serialize {}: {err}", path.display()))?;
        std::fs::write(path, &rewritten)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(Some(updated))
    }
}

/// Operates on an owned in-memory document (tests and programmatic
/// consumers); the in-memory twin of [`FileWaiverExtender`]. Every
/// successful extension is recorded in [`Self::extended`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryWaiverExtender {
    /// The document the extender operates on (the file's entries).
    pub doc: std::cell::RefCell<Vec<WaiverEntry>>,
    /// Every `(path, extended ID)` performed, in order.
    pub extended: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl WaiverExtender for InMemoryWaiverExtender {
    fn extend_expiry(
        &self,
        path: &Path,
        id: &str,
        expires: &str,
    ) -> Result<Option<WaiverEntry>, String> {
        let mut doc = self.doc.borrow_mut();
        if let Some(position) = doc.iter().position(|entry| entry.id == id) {
            doc[position].expires = expires.to_string();
            let updated = doc[position].clone();
            self.extended
                .borrow_mut()
                .push((path.to_path_buf(), id.to_string()));
            Ok(Some(updated))
        } else {
            Ok(None)
        }
    }
}

/// Result of a `waivers extend` run (also the
/// `glyim.ledger-waivers-extend/1` JSON face via
/// [`format_waivers_extend_json`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiversExtendOutcome {
    /// Schema tag, always [`WAIVERS_EXTEND_SCHEMA`].
    pub schema: &'static str,
    /// The extended waiver's ID (unchanged by the extension).
    pub id: String,
    /// The registry feature ID the extended waiver covers (unchanged).
    pub feature: String,
    /// The expiry the entry carried before the extension
    /// (`YYYY-MM-DD`, echoed verbatim — the audit trail).
    pub previous_expires: String,
    /// The new expiry (`today + days`, ISO spelling).
    pub expires: String,
    /// The granted horizon in days.
    pub days: i64,
    /// Waivers in the committed document (unchanged by an extension).
    pub remaining: usize,
    /// The document the block was rewritten in.
    pub target: String,
}

/// Extends one C9 waiver: validates the request, refuses to touch a
/// document that does not load cleanly, and moves exactly the one
/// matching block's expiry through `extender` — keeping the entry's ID,
/// feature, reason, and issue (identity is never rewritten).
///
/// # Validation contract
///
/// - `days` must be `1..=WAIVER_MAX_AGE_DAYS` (constitution C9: the new
///   expiry is pinned relative to today, so a horizon violation is
///   structurally impossible);
/// - `id` must be usable (non-blank, no control characters);
/// - the committed document must load with zero error-severity
///   diagnostics (the extender refuses to rewrite what it cannot fully
///   parse — a duplicate-ID file fails here, never silently);
/// - the ID must be present (an unknown ID writes nothing and names a
///   few existing candidates for the help line).
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`WaiversExtendBadDays`, `WaiversExtendBadId`, `WaiversExtendUnknownId`,
/// `WaiversExtendLoad`) and for a failed rewrite (`WaiversExtendWrite`).
pub fn run_waivers_extend(
    root: &Path,
    request: &WaiversExtendRequest,
    source: &dyn WaiverSource,
    extender: &dyn WaiverExtender,
) -> Result<WaiversExtendOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "waivers_extend",
        root = %root.display(),
        id = %request.id,
        days = request.days,
    );
    let _enter = span.enter();

    if request.days < 1 || request.days > WAIVER_MAX_AGE_DAYS {
        return Err(LedgerCliError::WaiversExtendBadDays {
            days: request.days.to_string(),
            horizon: WAIVER_MAX_AGE_DAYS,
        });
    }
    if request.id.trim().is_empty() || request.id.chars().any(char::is_control) {
        return Err(LedgerCliError::WaiversExtendBadId {
            id: request.id.clone(),
        });
    }

    // `today_iso` renders the `YYYY-MM-DD` spelling `parse_iso` accepts;
    // the same pinned round-trip the grant and revoke faces rely on.
    let today =
        NaiveDate::parse_iso(&today_iso()).expect("today_iso round-trips through parse_iso");
    let set = source.load(root, today);
    if set
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        let detail = set
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LedgerCliError::WaiversExtendLoad { detail });
    }
    let previous = set.waivers.iter().find(|w| w.id == request.id).cloned();
    let Some(previous) = previous else {
        let candidates = set
            .waivers
            .iter()
            .take(3)
            .map(|w| w.id.clone())
            .collect::<Vec<_>>();
        return Err(LedgerCliError::WaiversExtendUnknownId {
            id: request.id.clone(),
            candidates,
        });
    };

    let expires = today.plus_days(request.days).to_iso();
    let path = root.join("ledger/waivers.toml");
    let extended = extender
        .extend_expiry(&path, &request.id, &expires)
        .map_err(|detail| LedgerCliError::WaiversExtendWrite {
            path: path.display().to_string(),
            detail,
        })?;
    let Some(extended) = extended else {
        // The loaded set carried the ID but the document no longer does:
        // the registry drifted between the load pass and the rewrite.
        // Fail without a partial write.
        return Err(LedgerCliError::WaiversExtendUnknownId {
            id: request.id.clone(),
            candidates: Vec::new(),
        });
    };
    tracing::debug!(
        id = %extended.id,
        previous = %previous.expires,
        expires = %expires,
        "waiver extended"
    );

    Ok(WaiversExtendOutcome {
        schema: WAIVERS_EXTEND_SCHEMA,
        id: extended.id,
        feature: extended.feature,
        previous_expires: previous.expires,
        expires,
        days: request.days,
        remaining: set.waivers.len(),
        target: path.display().to_string(),
    })
}

/// Renders the extend outcome's text face: one summary line (the
/// extended block's ID, feature, previous and new expiry, and the
/// remaining count).
#[must_use]
pub fn format_waivers_extend_text(outcome: &WaiversExtendOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "extended {} for {} (expires {} -> {}, {} day(s)) -> {} waiver(s) remain at {}",
        outcome.id,
        outcome.feature,
        outcome.previous_expires,
        outcome.expires,
        outcome.days,
        outcome.remaining,
        outcome.target,
    );
    text
}

/// Renders the `glyim.ledger-waivers-extend/1` JSON contract.
#[must_use]
pub fn format_waivers_extend_json(outcome: &WaiversExtendOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-waivers-extend/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

// -- prune face ---------------------------------------------------------------

/// Schema tag of the `waivers prune` JSON contract.
pub const WAIVERS_PRUNE_SCHEMA: &str = "glyim.ledger-waivers-prune/1";

/// One pruned `[[waiver]]` block, echoed for the report (the identity
/// the C9 file loses: ID, covered feature, expiry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiversPruneRow {
    /// The pruned waiver's ID.
    pub id: String,
    /// The registry feature ID the pruned waiver covered.
    pub feature: String,
    /// The pruned waiver's expiry (`YYYY-MM-DD`, echoed verbatim).
    pub expires: String,
}

/// Result of a `waivers prune` run (also the
/// `glyim.ledger-waivers-prune/1` JSON face via
/// [`format_waivers_prune_json`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaiversPruneOutcome {
    /// Schema tag, always [`WAIVERS_PRUNE_SCHEMA`].
    pub schema: &'static str,
    /// The pruned blocks, ID order (the [`assess_waivers`] contract:
    /// rows render in ID order regardless of the document's order).
    pub removed: Vec<WaiversPruneRow>,
    /// Waivers remaining in the committed document after the prune.
    pub remaining: usize,
    /// The document the blocks were removed from.
    pub target: String,
}

/// Where expired `[[waiver]]` blocks are removed from. The batch
/// rewrite seam mirrors [`WaiverRewriter`] (one exact ID) on the
/// many-expired case: the engine consumes the trait, so tests drive an
/// in-memory document and the CLI wires the file-backed implementation.
pub trait WaiverPruner {
    /// Removes every `[[waiver]]` block whose expiry is strictly before
    /// `today` from the document at `path` and rewrites the remaining
    /// entries canonically. Returns the removed entries in ID order; an
    /// empty return means nothing expired (and nothing is written — a
    /// clean document is never rewritten).
    fn prune_expired(&self, path: &Path, today: NaiveDate) -> Result<Vec<WaiverEntry>, String>;
}

/// Rewrites `<ROOT>/ledger/waivers.toml` on disk: parse, partition the
/// entries by expiry, re-serialize the remaining entries canonically,
/// and replace the file. A document that fails to parse is refused
/// (`Err`), not truncated; a document with no expired entry is never
/// written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileWaiverPruner;

impl WaiverPruner for FileWaiverPruner {
    fn prune_expired(&self, path: &Path, today: NaiveDate) -> Result<Vec<WaiverEntry>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let doc: RevokeSourceDoc = toml::from_str(&text)
            .map_err(|err| format!("cannot parse {}: {err}", path.display()))?;
        let today_days = today.to_days();
        let mut removed: Vec<WaiverEntry> = Vec::new();
        let mut remaining: Vec<WaiverEntry> = Vec::new();
        for entry in doc.waiver {
            // An unparseable expiry never expires here: the loader
            // diagnostics it before this seam runs, and the engine refuses
            // a non-clean load, so this arm is defensive only.
            let expired = NaiveDate::parse_iso(&entry.expires)
                .is_some_and(|expires| expires.to_days() < today_days);
            if expired {
                removed.push(entry);
            } else {
                remaining.push(entry);
            }
        }
        if removed.is_empty() {
            return Ok(removed);
        }
        removed.sort_by(|a, b| a.id.cmp(&b.id));
        let rewritten = toml::to_string(&RevokeSourceDoc { waiver: remaining })
            .map_err(|err| format!("cannot serialize {}: {err}", path.display()))?;
        std::fs::write(path, &rewritten)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(removed)
    }
}

/// Operates on an owned in-memory document (tests and programmatic
/// consumers); the in-memory twin of [`FileWaiverPruner`]. Every prune
/// is recorded in [`Self::pruned`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryWaiverPruner {
    /// The document the pruner operates on (the file's entries).
    pub doc: std::cell::RefCell<Vec<WaiverEntry>>,
    /// Every `(path, pruned IDs)` performed, in order.
    pub pruned: std::cell::RefCell<Vec<(PathBuf, Vec<String>)>>,
}

impl WaiverPruner for InMemoryWaiverPruner {
    fn prune_expired(&self, path: &Path, today: NaiveDate) -> Result<Vec<WaiverEntry>, String> {
        let mut doc = self.doc.borrow_mut();
        let today_days = today.to_days();
        let (mut expired, remaining): (Vec<WaiverEntry>, Vec<WaiverEntry>) =
            doc.drain(..).partition(|entry| {
                NaiveDate::parse_iso(&entry.expires)
                    .is_some_and(|expires| expires.to_days() < today_days)
            });
        *doc = remaining;
        // ID order for the return and the audit trail both — the same
        // determinism contract `assess_waivers` applies to its rows.
        expired.sort_by(|a, b| a.id.cmp(&b.id));
        let ids: Vec<String> = expired.iter().map(|entry| entry.id.clone()).collect();
        if !ids.is_empty() {
            self.pruned.borrow_mut().push((path.to_path_buf(), ids));
        }
        Ok(expired)
    }
}

/// Prunes every expired C9 waiver: validates the document, refuses to
/// touch one that does not load cleanly, and removes every expired
/// block through `pruner` — the batch counterpart of
/// [`run_waivers_revoke`] (one ID at a time) for the moment an operator
/// clears the whole C9 debt at once.
///
/// # Validation contract
///
/// - the committed document must load with zero error-severity
///   diagnostics (the pruner refuses to rewrite what it cannot fully
///   parse — a duplicate-ID file fails here, never silently);
/// - a document with no expired waiver is never rewritten (the seam is
///   not invoked; the file bytes stay untouched).
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for a refused document
/// (`WaiversPruneLoad`) and for a failed rewrite (`WaiversPruneWrite`).
pub fn run_waivers_prune(
    root: &Path,
    source: &dyn WaiverSource,
    pruner: &dyn WaiverPruner,
) -> Result<WaiversPruneOutcome, LedgerCliError> {
    let span = tracing::info_span!("waivers_prune", root = %root.display());
    let _enter = span.enter();

    // `today_iso` renders the `YYYY-MM-DD` spelling `parse_iso` accepts;
    // the same pinned round-trip the grant/revoke/extend faces rely on.
    let today =
        NaiveDate::parse_iso(&today_iso()).expect("today_iso round-trips through parse_iso");
    let set = source.load(root, today);
    if set
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        let detail = set
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LedgerCliError::WaiversPruneLoad { detail });
    }
    let path = root.join("ledger/waivers.toml");
    let expired = assess_waivers(&set, today).expired;
    if expired == 0 {
        // The batch face never rewrites a clean document: the seam is not
        // invoked and the file bytes stay untouched.
        return Ok(WaiversPruneOutcome {
            schema: WAIVERS_PRUNE_SCHEMA,
            removed: Vec::new(),
            remaining: set.waivers.len(),
            target: path.display().to_string(),
        });
    }
    let removed =
        pruner
            .prune_expired(&path, today)
            .map_err(|detail| LedgerCliError::WaiversPruneWrite {
                path: path.display().to_string(),
                detail,
            })?;
    // Rows report in ID order (the `assess_waivers` contract) regardless
    // of the document's construction path.
    let mut removed = removed;
    removed.sort_by(|a, b| a.id.cmp(&b.id));
    let rows = removed
        .iter()
        .map(|entry| WaiversPruneRow {
            id: entry.id.clone(),
            feature: entry.feature.clone(),
            expires: entry.expires.clone(),
        })
        .collect();
    let remaining = set.waivers.len().saturating_sub(removed.len());
    tracing::debug!(pruned = removed.len(), remaining, "waivers pruned");

    Ok(WaiversPruneOutcome {
        schema: WAIVERS_PRUNE_SCHEMA,
        removed: rows,
        remaining,
        target: path.display().to_string(),
    })
}

/// Renders the prune outcome's text face: one summary line (the pruned
/// count, the remaining count, and the target document) plus one line
/// per pruned block.
#[must_use]
pub fn format_waivers_prune_text(outcome: &WaiversPruneOutcome) -> String {
    let mut text = String::new();
    if outcome.removed.is_empty() {
        let _ = writeln!(
            text,
            "no expired waiver to prune at {} ({} waiver(s) active)",
            outcome.target, outcome.remaining,
        );
        return text;
    }
    let _ = writeln!(
        text,
        "pruned {} expired waiver(s) -> {} waiver(s) remain at {}",
        outcome.removed.len(),
        outcome.remaining,
        outcome.target,
    );
    for row in &outcome.removed {
        let _ = writeln!(
            text,
            "- {} for {} (expires {})",
            row.id, row.feature, row.expires,
        );
    }
    text
}

/// Renders the `glyim.ledger-waivers-prune/1` JSON contract.
#[must_use]
pub fn format_waivers_prune_json(outcome: &WaiversPruneOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-waivers-prune/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    //! Tests pin the C9 waiver-health contract: verdicts are pure date
    //! arithmetic over the loaded [`WaiverSet`], both renderer faces are
    //! deterministic (golden), the JSON parses back into the
    //! `glyim.ledger-waivers/1` contract, the file source reads
    //! `ledger/waivers.toml` through the same [`load_waivers`] loader the
    //! `diff` gate uses, and the CLI seam parses/rejects exactly like the
    //! neighbouring `gates` face.

    use std::path::{Path, PathBuf};

    use ledger_schema::{Diagnostic, Severity};

    use super::{
        FileWaiverRewriter, FileWaiverSource, InMemoryWaiverRewriter, InMemoryWaiverSource,
        JsonWaiversRenderer, TextWaiversRenderer, WaiverRewriter, WaiverSource, WaiverVerdict,
        WaiversRenderer, WaiversRevokeRequest,
    };
    use crate::diff::{NaiveDate, WAIVER_MAX_AGE_DAYS, WaiverEntry, WaiverSet};
    use crate::{Command, LedgerCliError, WaiversFormat, parse_args, usage};

    /// Fixed assessment date: no clock in the tests (the determinism
    /// contract treats `today` as a value).
    fn today() -> NaiveDate {
        NaiveDate::new_unchecked(2026, 11, 13)
    }

    /// One synthetic waiver; `days_ahead` is signed from `today()`
    /// (negative = already expired, but still inside the C9 horizon).
    fn entry(id: &str, feature: &str, expires_days_ahead: i64) -> WaiverEntry {
        WaiverEntry {
            id: id.to_string(),
            feature: feature.to_string(),
            reason: format!("{id}: synthetic exception for the test suite"),
            issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
            expires: today().plus_days(expires_days_ahead).to_iso(),
        }
    }

    /// A set with one valid waiver, one expired waiver, and one valid
    /// waiver with zero days left (the `expires == today` boundary).
    fn mixed_set() -> WaiverSet {
        WaiverSet {
            waivers: vec![
                entry("WVR-0003", "LANG-FOR-VEC-INTOITER", 6),
                entry("WVR-0001", "LANG-TEST-SNAPSHOT", -3),
                entry("WVR-0002", "LANG-ASYNC-AWAIT", 0),
            ],
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn assess_marks_valid_expired_and_boundary_rows() {
        let outcome = super::assess_waivers(&mixed_set(), today());
        assert_eq!(outcome.waivers, 3);
        assert_eq!(outcome.valid, 2);
        assert_eq!(outcome.expired, 1);
        // Rows stay sorted by ID (the order `load_waivers` establishes).
        let ids: Vec<&str> = outcome.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["WVR-0001", "WVR-0002", "WVR-0003"]);
        let first = &outcome.rows[0];
        assert_eq!(first.verdict, WaiverVerdict::Expired);
        assert_eq!(first.remaining_days, -3);
        let boundary = &outcome.rows[1];
        assert_eq!(boundary.verdict, WaiverVerdict::Valid);
        assert_eq!(
            boundary.remaining_days, 0,
            "expires == today is still valid"
        );
        let last = &outcome.rows[2];
        assert_eq!(last.verdict, WaiverVerdict::Valid);
        assert_eq!(last.remaining_days, 6);
    }

    #[test]
    fn expired_or_errored_sets_fail_the_c9_gate() {
        // A fully-valid set (the expired entry removed) passes.
        let mut valid_set = mixed_set();
        valid_set.waivers.retain(|w| w.id != "WVR-0001");
        let outcome = super::assess_waivers(&valid_set, today());
        assert!(outcome.is_ok(), "no expired rows, no diagnostics");
        // The full set carries one expired row: C9 fails it.
        let outcome = super::assess_waivers(&mixed_set(), today());
        assert!(!outcome.is_ok(), "an expired waiver fails per C9");
        // An error-severity load diagnostic fails like `check`.
        let mut errored = mixed_set();
        errored.waivers.clear();
        errored.diagnostics = vec![Diagnostic {
            code: crate::diff::CODE_WAIVER_DUP,
            message: "waiver id \"W\" is declared twice".to_string(),
            severity: Severity::Error,
        }];
        let outcome = super::assess_waivers(&errored, today());
        assert!(!outcome.is_ok(), "an error diagnostic fails like `check`");
    }

    #[test]
    fn both_renderers_render_twice_is_byte_identical() {
        let outcome = super::assess_waivers(&mixed_set(), today());
        let first = TextWaiversRenderer.render(&outcome);
        let second = TextWaiversRenderer.render(&outcome);
        assert_eq!(first, second, "C1/M1.13: outputs deterministic (golden)");
        let first = JsonWaiversRenderer.render(&outcome);
        let second = JsonWaiversRenderer.render(&outcome);
        assert_eq!(first, second);
    }

    #[test]
    fn text_golden_pins_the_report_face() {
        let outcome = super::assess_waivers(&mixed_set(), today());
        let text = TextWaiversRenderer.render(&outcome);
        let expected = "\
waivers: 3 loaded, 2 valid, 1 expired, 0 diagnostic(s), assessed at 2026-11-13
  WVR-0001                   expired    -3d        expires 2026-11-10
         feature: LANG-TEST-SNAPSHOT
         reason: WVR-0001: synthetic exception for the test suite
         issue: https://github.com/elcoosp/glyim/issues/42
  WVR-0002                   valid      0d         expires 2026-11-13
         feature: LANG-ASYNC-AWAIT
         reason: WVR-0002: synthetic exception for the test suite
         issue: https://github.com/elcoosp/glyim/issues/42
  WVR-0003                   valid      6d         expires 2026-11-19
         feature: LANG-FOR-VEC-INTOITER
         reason: WVR-0003: synthetic exception for the test suite
         issue: https://github.com/elcoosp/glyim/issues/42
";
        assert_eq!(text, expected);
    }

    #[test]
    fn json_contract_parses_back_with_schema_and_counts() {
        let outcome = super::assess_waivers(&mixed_set(), today());
        let text = JsonWaiversRenderer.render(&outcome);
        let value: serde_json::Value = serde_json::from_str(&text)
            .expect("the glyim.ledger-waivers/1 face must be valid JSON");
        assert_eq!(value["schema"], "glyim.ledger-waivers/1");
        assert_eq!(value["today"], "2026-11-13");
        assert_eq!(value["waivers"], 3);
        assert_eq!(value["valid"], 2);
        assert_eq!(value["expired"], 1);
        assert_eq!(value["passed"], false, "one expired row fails the C9 gate");
        let rows = value["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["id"], "WVR-0001");
        assert_eq!(rows[0]["verdict"], "expired");
        assert_eq!(rows[0]["remaining_days"], -3);
        assert_eq!(rows[2]["verdict"], "valid");
        assert_eq!(rows[2]["remaining_days"], 6);
        assert_eq!(rows[2]["feature"], "LANG-FOR-VEC-INTOITER");
    }

    #[test]
    fn file_source_reads_ledger_waivers_toml_and_absent_file_is_empty() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        std::fs::create_dir_all(root.join("ledger")).expect("mkdir");
        std::fs::write(
            root.join("ledger/waivers.toml"),
            "[[waiver]]\n\
             id = \"WVR-0001\"\n\
             feature = \"LANG-TEST-SNAPSHOT\"\n\
             reason = \"expired while the fix lands\"\n\
             issue = \"https://github.com/elcoosp/glyim/issues/42\"\n\
             expires = \"2026-11-10\"\n\
             \n\
             [[waiver]]\n\
             id = \"WVR-0002\"\n\
             feature = \"LANG-ASYNC-AWAIT\"\n\
             reason = \"fresh grant inside the horizon\"\n\
             issue = \"https://github.com/elcoosp/glyim/issues/43\"\n\
             expires = \"2026-11-19\"\n",
        )
        .expect("write waivers.toml");
        let set = FileWaiverSource.load(root, today());
        let outcome = super::assess_waivers(&set, today());
        assert_eq!(outcome.waivers, 2, "both entries load");
        assert_eq!(outcome.expired, 1, "the 2026-11-10 entry is expired");
        assert!(!outcome.is_ok(), "C9: the expired waiver fails the face");

        let empty = FileWaiverSource.load(Path::new("/nonexistent-root"), today());
        assert!(
            empty.waivers.is_empty() && empty.diagnostics.is_empty(),
            "an absent waivers.toml is the empty baseline, not an error"
        );
    }

    #[test]
    fn in_memory_source_round_trips_the_set() {
        let set = mixed_set();
        let source = InMemoryWaiverSource { set: set.clone() };
        let loaded = source.load(Path::new("."), today());
        assert_eq!(loaded, set);
    }

    #[test]
    fn horizon_violations_surface_through_load_diagnostics() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        std::fs::create_dir_all(root.join("ledger")).expect("mkdir");
        std::fs::write(
            root.join("ledger/waivers.toml"),
            format!(
                "[[waiver]]\n\
                 id = \"WVR-LONG\"\n\
                 feature = \"LANG-ASYNC-AWAIT\"\n\
                 reason = \"granted far too long\"\n\
                 issue = \"https://github.com/elcoosp/glyim/issues/44\"\n\
                 expires = \"{}\"\n",
                today().plus_days(WAIVER_MAX_AGE_DAYS + 1).to_iso()
            ),
        )
        .expect("write waivers.toml");
        let set = FileWaiverSource.load(root, today());
        assert!(set.waivers.is_empty(), "the entry is inert (dropped)");
        let outcome = super::assess_waivers(&set, today());
        assert!(
            !outcome.is_ok(),
            "the horizon diagnostic fails like `check`"
        );
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|d| d.code == crate::diff::CODE_WAIVER_HORIZON),
            "the horizon code surfaces verbatim: {:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn parse_args_accepts_waivers_flags() {
        let command = parse_args(
            ["waivers", "docs", "--format", "json"]
                .into_iter()
                .map(String::from),
        )
        .expect("parse");
        assert_eq!(
            command,
            Command::Waivers {
                root: PathBuf::from("docs"),
                format: WaiversFormat::Json,
            }
        );
        let default = parse_args(["waivers"].into_iter().map(String::from)).expect("parse");
        assert_eq!(
            default,
            Command::Waivers {
                root: PathBuf::from("."),
                format: WaiversFormat::Text,
            }
        );
    }

    #[test]
    fn parse_args_rejects_bad_waivers_flags() {
        let err = parse_args(
            ["waivers", "--format", "yaml"]
                .into_iter()
                .map(String::from),
        )
        .expect_err("bad format");
        assert!(matches!(err, LedgerCliError::WaiversBadFormat { .. }));
        let err = parse_args(["waivers", "--format"].into_iter().map(String::from))
            .expect_err("missing value");
        assert!(matches!(err, LedgerCliError::WaiversMissingValue { .. }));
        let err = parse_args(["waivers", "a", "b"].into_iter().map(String::from))
            .expect_err("extra positional");
        assert!(matches!(err, LedgerCliError::ExtraArgument { .. }));
    }

    #[test]
    fn waivers_error_variants_carry_code_and_help() {
        for err in [
            LedgerCliError::WaiversBadFormat {
                value: "yaml".to_string(),
                usage: usage(),
            },
            LedgerCliError::WaiversMissingValue {
                flag: "--format".to_string(),
                usage: usage(),
            },
        ] {
            assert!(
                miette::Diagnostic::code(&err).is_some(),
                "{err:?} must carry a code"
            );
            assert!(
                miette::Diagnostic::help(&err).is_some(),
                "{err:?} must carry help"
            );
        }
    }

    #[test]
    fn usage_lists_waivers() {
        assert!(usage().contains("waivers"));
    }

    // -- grant face -------------------------------------------------------------

    use super::{FileWaiverSink, WaiversGrantRequest, run_waivers_grant};
    use std::path::Path as GrantPath;

    /// Writes the minimal valid registry the grant face validates against
    /// (the evidence-store fixture shape: one feature over a spec anchor
    /// plus the seed policy) and seeds the committed waiver document.
    fn write_grant_project(root: &GrantPath, seed_waivers: &str) {
        std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
        std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
        std::fs::write(
            root.join("docs/specs/v0.1.0.md"),
            "# spec fixture\n\n## evidence\n",
        )
        .expect("write spec");
        std::fs::write(
            root.join("ledger/policy.toml"),
            "[levels]\n\
             L1 = { min_pass = 1, require_evidence = false }\n\
             [gap_caps]\n\
             enabled = true\n\
             [hardening]\n\
             fuzz_cpu_hours = 24\n\
             mutation_kill_rate_pct = 75\n",
        )
        .expect("write policy");
        std::fs::write(
            root.join("ledger/features/grant.toml"),
            "[[feature]]\n\
             id = \"LANG-EVIDENCE-A\"\n\
             title = \"grant fixture feature\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\"]\n\
             owner_lane = \"M\"\n",
        )
        .expect("write feature");
        if !seed_waivers.is_empty() {
            std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");
            std::fs::write(root.join("ledger/waivers.toml"), seed_waivers).expect("seed waivers");
        }
    }

    /// The canonical grant request for the fixture feature.
    fn grant_request() -> WaiversGrantRequest {
        WaiversGrantRequest {
            feature: "LANG-EVIDENCE-A".to_string(),
            reason: "regressions covered while the fix lands".to_string(),
            issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
            days: 7,
        }
    }

    #[test]
    fn grant_appends_a_canonical_block_that_loads_back() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(root, "");

        let outcome = run_waivers_grant(root, &grant_request(), &FileWaiverSource, &FileWaiverSink)
            .expect("grant pass");
        // Deterministic ID: WVR-<YYYYMMDD>-<feature slug>.
        assert_eq!(outcome.id, "WVR-20261113-lang-evidence-a");
        assert_eq!(outcome.feature, "LANG-EVIDENCE-A");
        assert_eq!(outcome.expires, "2026-11-20");
        assert_eq!(outcome.days, 7);
        assert!(
            outcome.target.ends_with("ledger/waivers.toml"),
            "the fragment lands in the committed document: {}",
            outcome.target
        );
        assert!(
            outcome.fragment.starts_with("[[waiver]]\n"),
            "the fragment is a canonical [[waiver]] block: {:?}",
            outcome.fragment
        );

        // The appended document must parse back through the same loader
        // the health face and the diff gate use.
        let set = FileWaiverSource.load(root, today());
        assert_eq!(set.waivers.len(), 1, "the granted entry loads back");
        assert_eq!(set.waivers[0].id, "WVR-20261113-lang-evidence-a");
        assert_eq!(set.waivers[0].expires, "2026-11-20");
        assert!(set.diagnostics.is_empty());
        let outcome = super::assess_waivers(&set, today());
        assert!(outcome.is_ok(), "the fresh grant is valid and live");
    }

    #[test]
    fn grant_preserves_existing_entries_and_trailing_content() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(
            root,
            "[[waiver]]\n\
             id = \"WVR-EXISTING\"\n\
             feature = \"LANG-EVIDENCE-A\"\n\
             reason = \"earlier exception\"\n\
             issue = \"https://github.com/elcoosp/glyim/issues/41\"\n\
             expires = \"2026-11-19\"\n",
        );
        run_waivers_grant(root, &grant_request(), &FileWaiverSource, &FileWaiverSink)
            .expect("grant pass");
        let set = FileWaiverSource.load(root, today());
        assert_eq!(
            set.waivers.len(),
            2,
            "the appended block must not clobber the seeded entry"
        );
    }

    #[test]
    fn grant_rejects_unknown_feature_and_registry_errors() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(root, "");
        let mut request = grant_request();
        request.feature = "LANG-DOES-NOT-EXIST".to_string();
        let err = run_waivers_grant(root, &request, &FileWaiverSource, &FileWaiverSink)
            .expect_err("unknown feature must fail");
        match &err {
            LedgerCliError::WaiversGrantUnknownFeature { feature, .. } => {
                assert_eq!(feature, "LANG-DOES-NOT-EXIST");
            }
            other => panic!("expected WaiversGrantUnknownFeature, got {other:?}"),
        }
        assert!(miette::Diagnostic::help(&err).is_some());

        // A broken registry (a malformed feature entry) fails before any
        // write.
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(root, "");
        std::fs::write(
            root.join("ledger/features/grant.toml"),
            "[[feature]]\nid = 5\n",
        )
        .expect("break feature");
        let err = run_waivers_grant(root, &grant_request(), &FileWaiverSource, &FileWaiverSink)
            .expect_err("broken registry must fail");
        assert!(matches!(err, LedgerCliError::WaiversGrantRegistry { .. }));
        assert!(
            !root.join("ledger/waivers.toml").exists(),
            "no document is written when validation fails"
        );
    }

    #[test]
    fn grant_rejects_bad_issue_link_and_bad_days_and_empty_reason() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(root, "");

        let mut request = grant_request();
        request.issue = "http://insecure.example/issue/42".to_string();
        let err = run_waivers_grant(root, &request, &FileWaiverSource, &FileWaiverSink)
            .expect_err("http:// issue must fail");
        assert!(matches!(err, LedgerCliError::WaiversGrantBadIssue { .. }));

        let mut request = grant_request();
        request.reason = "   ".to_string();
        let err = run_waivers_grant(root, &request, &FileWaiverSource, &FileWaiverSink)
            .expect_err("blank reason must fail");
        assert!(matches!(
            err,
            LedgerCliError::WaiversGrantMissingField { .. }
        ));

        for days in [0, WAIVER_MAX_AGE_DAYS + 1] {
            let mut request = grant_request();
            request.days = days;
            let err = run_waivers_grant(root, &request, &FileWaiverSource, &FileWaiverSink)
                .expect_err("out-of-horizon days must fail");
            match &err {
                LedgerCliError::WaiversGrantBadDays { horizon, .. } => {
                    assert_eq!(*horizon, WAIVER_MAX_AGE_DAYS);
                }
                other => panic!("expected WaiversGrantBadDays, got {other:?}"),
            }
        }
        assert!(
            !root.join("ledger/waivers.toml").exists(),
            "a rejected request writes nothing"
        );
    }

    #[test]
    fn grant_rejects_a_duplicate_deterministic_id() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(root, "");
        run_waivers_grant(root, &grant_request(), &FileWaiverSource, &FileWaiverSink)
            .expect("first grant");
        let err = run_waivers_grant(root, &grant_request(), &FileWaiverSource, &FileWaiverSink)
            .expect_err("second same-day grant must fail");
        match &err {
            LedgerCliError::WaiversGrantDuplicate { id, .. } => {
                assert_eq!(id, "WVR-20261113-lang-evidence-a");
            }
            other => panic!("expected WaiversGrantDuplicate, got {other:?}"),
        }
    }

    #[test]
    fn in_memory_sink_records_the_fragment_for_tests() {
        let sink = FileWaiverSink;
        let _ = &sink;
        let in_memory = super::InMemoryWaiverSink::default();
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(root, "");
        let outcome = run_waivers_grant(root, &grant_request(), &FileWaiverSource, &in_memory)
            .expect("grant pass");
        let written = in_memory.written.borrow();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].1, outcome.fragment);
    }

    #[test]
    fn grant_outcome_renders_both_faces() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_grant_project(root, "");
        let outcome = run_waivers_grant(root, &grant_request(), &FileWaiverSource, &FileWaiverSink)
            .expect("grant pass");
        let text = super::format_waivers_grant_text(&outcome);
        assert!(
            text.contains("granted WVR-20261113-lang-evidence-a"),
            "{text}"
        );
        let json = super::format_waivers_grant_json(&outcome);
        let value: serde_json::Value = serde_json::from_str(&json)
            .expect("the glyim.ledger-waivers-grant/1 face must be valid JSON");
        assert_eq!(value["schema"], "glyim.ledger-waivers-grant/1");
        assert_eq!(value["id"], "WVR-20261113-lang-evidence-a");
        assert_eq!(value["expires"], "2026-11-20");
        assert_eq!(value["days"], 7);
    }

    #[test]
    fn parse_args_accepts_waivers_grant_flags() {
        let command = parse_args(
            [
                "waivers",
                "grant",
                "docs",
                "--feature",
                "LANG-EVIDENCE-A",
                "--reason",
                "fix in flight",
                "--issue",
                "https://github.com/elcoosp/glyim/issues/42",
                "--days",
                "7",
                "--format",
                "json",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("parse");
        assert_eq!(
            command,
            Command::WaiversGrant {
                root: PathBuf::from("docs"),
                feature: "LANG-EVIDENCE-A".to_string(),
                reason: "fix in flight".to_string(),
                issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
                days: Some(7),
                format: WaiversFormat::Json,
            }
        );
        // The default horizon is 7 days when --days is omitted.
        let default = parse_args(
            [
                "waivers",
                "grant",
                "--feature",
                "LANG-EVIDENCE-A",
                "--reason",
                "fix in flight",
                "--issue",
                "https://github.com/elcoosp/glyim/issues/42",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("parse");
        assert_eq!(
            default,
            Command::WaiversGrant {
                root: PathBuf::from("."),
                feature: "LANG-EVIDENCE-A".to_string(),
                reason: "fix in flight".to_string(),
                issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
                days: None,
                format: WaiversFormat::Text,
            }
        );
    }

    #[test]
    fn parse_args_rejects_bad_waivers_grant_flags() {
        let err = parse_args(
            ["waivers", "grant", "--feature", "LANG-EVIDENCE-A"]
                .into_iter()
                .map(String::from),
        )
        .expect_err("missing fields");
        assert!(matches!(
            err,
            LedgerCliError::WaiversGrantMissingField { .. }
        ));

        let err = parse_args(
            [
                "waivers",
                "grant",
                "--feature",
                "LANG-EVIDENCE-A",
                "--reason",
                "r",
                "--issue",
                "https://github.com/elcoosp/glyim/issues/42",
                "--days",
                "later",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect_err("bad days");
        assert!(matches!(err, LedgerCliError::WaiversGrantBadDays { .. }));

        let err = parse_args(["waivers", "grant", "--wat"].into_iter().map(String::from))
            .expect_err("unknown flag");
        assert!(matches!(err, LedgerCliError::UnknownFlag { .. }));

        let err = parse_args(["waivers", "grant"].into_iter().map(String::from))
            .expect_err("missing all fields");
        assert!(matches!(
            err,
            LedgerCliError::WaiversGrantMissingField { .. }
        ));
    }

    #[test]
    fn grant_error_variants_carry_code_and_help() {
        for err in [
            LedgerCliError::WaiversGrantMissingField {
                field: "--feature".to_string(),
                usage: usage(),
            },
            LedgerCliError::WaiversGrantBadDays {
                days: "15".to_string(),
                horizon: WAIVER_MAX_AGE_DAYS,
            },
            LedgerCliError::WaiversGrantBadIssue {
                issue: "http://x".to_string(),
            },
            LedgerCliError::WaiversGrantUnknownFeature {
                feature: "LANG-X".to_string(),
                candidates: vec!["LANG-EVIDENCE-A".to_string()],
            },
            LedgerCliError::WaiversGrantRegistry {
                detail: "policy unreadable".to_string(),
            },
            LedgerCliError::WaiversGrantDuplicate {
                id: "WVR-1".to_string(),
                path: "ledger/waivers.toml".to_string(),
            },
            LedgerCliError::WaiversGrantWrite {
                path: "ledger/waivers.toml".to_string(),
                detail: "denied".to_string(),
            },
        ] {
            assert!(
                miette::Diagnostic::code(&err).is_some(),
                "{err:?} must carry a code"
            );
            assert!(
                miette::Diagnostic::help(&err).is_some(),
                "{err:?} must carry help"
            );
        }
    }

    #[test]
    fn usage_lists_waivers_grant() {
        assert!(usage().contains("waivers grant"));
    }

    // -- revoke face ------------------------------------------------------------

    /// Serializes `entries` into the canonical committed document text
    /// (the exact bytes a grant run would produce, reused to seed the
    /// file-backed rewriter fixtures).
    fn canonical_document(entries: &[WaiverEntry]) -> String {
        toml::to_string(&super::RevokeSourceDoc {
            waiver: entries.to_vec(),
        })
        .expect("the canonical [[waiver]] document serializes")
    }

    /// A tempdir project whose `ledger/waivers.toml` carries the three
    /// canonical entries; returns the root path.
    fn seeded_root(entries: &[WaiverEntry]) -> PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");
        std::fs::write(
            root.join("ledger/waivers.toml"),
            canonical_document(entries),
        )
        .expect("seed waivers.toml");
        // Leak the tempdir for the body of the test (`TempDir` deletes on
        // drop; keeping the guard alive in the caller is equivalent and
        // cleaner, so callers own the guard instead).
        std::mem::forget(dir);
        root
    }

    #[test]
    fn revoke_removes_exactly_the_target_block_and_loads_back() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261105-second", "LANG-B", 3),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let root = seeded_root(&entries);

        let outcome = super::run_waivers_revoke(
            &root,
            &WaiversRevokeRequest {
                id: "WVR-20261105-second".to_string(),
            },
            &FileWaiverSource,
            &FileWaiverRewriter,
        )
        .expect("revoke of a present ID succeeds");

        assert_eq!(outcome.id, "WVR-20261105-second");
        assert_eq!(outcome.feature, "LANG-B");
        assert_eq!(outcome.remaining, 2);

        let reloaded = FileWaiverSource.load(&root, today());
        assert_eq!(reloaded.waivers.len(), 2);
        assert!(
            reloaded
                .waivers
                .iter()
                .all(|w| w.id != "WVR-20261105-second"),
            "the revoked ID is gone from the committed document"
        );
        let ids: Vec<&str> = reloaded.waivers.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["WVR-20261101-first", "WVR-20261109-third"]);
    }

    #[test]
    fn revoke_keeps_the_remaining_entries_canonical() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261105-second", "LANG-B", 3),
        ];
        let root = seeded_root(&entries);
        super::run_waivers_revoke(
            &root,
            &WaiversRevokeRequest {
                id: "WVR-20261105-second".to_string(),
            },
            &FileWaiverSource,
            &FileWaiverRewriter,
        )
        .expect("revoke succeeds");

        let text = std::fs::read_to_string(root.join("ledger/waivers.toml")).expect("read back");
        assert!(!text.contains("WVR-20261105-second"));
        assert!(
            text.ends_with('\n'),
            "the rewritten document ends with a newline"
        );
        // Byte-stability: the rewritten document re-serializes to itself
        // (the canonical round-trip the grant face established).
        assert_eq!(
            text,
            canonical_document(&[entry("WVR-20261101-first", "LANG-A", 5)]),
        );
    }

    #[test]
    fn revoke_rejects_an_unknown_id_and_writes_nothing() {
        let entries = vec![entry("WVR-20261101-first", "LANG-A", 5)];
        let root = seeded_root(&entries);
        let before = std::fs::read_to_string(root.join("ledger/waivers.toml")).expect("read");

        let err = super::run_waivers_revoke(
            &root,
            &WaiversRevokeRequest {
                id: "WVR-20261201-absent".to_string(),
            },
            &FileWaiverSource,
            &FileWaiverRewriter,
        )
        .expect_err("unknown ID fails");

        match &err {
            LedgerCliError::WaiversRevokeUnknownId { id, candidates } => {
                assert_eq!(id, "WVR-20261201-absent");
                assert_eq!(candidates.as_slice(), ["WVR-20261101-first".to_string()]);
            }
            other => panic!("expected WaiversRevokeUnknownId, got {other:?}"),
        }
        let after = std::fs::read_to_string(root.join("ledger/waivers.toml")).expect("read");
        assert_eq!(before, after, "nothing was written when validation failed");
    }

    #[test]
    fn revoke_rejects_an_unloadable_document() {
        // A duplicate ID is an error-severity load diagnostic: the
        // rewriter refuses to touch a document it cannot fully parse.
        let duplicated = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261101-first", "LANG-B", 3),
        ];
        let root = seeded_root(&duplicated);
        let err = super::run_waivers_revoke(
            &root,
            &WaiversRevokeRequest {
                id: "WVR-20261101-first".to_string(),
            },
            &FileWaiverSource,
            &FileWaiverRewriter,
        )
        .expect_err("a duplicate-ID document fails the load gate");
        assert!(matches!(err, LedgerCliError::WaiversRevokeLoad { .. }));
    }

    #[test]
    fn revoke_rejects_a_blank_or_control_id() {
        let root = seeded_root(&[entry("WVR-20261101-first", "LANG-A", 5)]);
        for id in ["", "   ", "WVR-\tbad"] {
            let err = super::run_waivers_revoke(
                &root,
                &WaiversRevokeRequest { id: id.to_string() },
                &FileWaiverSource,
                &FileWaiverRewriter,
            )
            .expect_err("blank or control-character ID fails");
            assert!(matches!(err, LedgerCliError::WaiversRevokeBadId { .. }));
        }
    }

    #[test]
    fn in_memory_rewriter_records_the_removal() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let rewriter = InMemoryWaiverRewriter {
            doc: std::cell::RefCell::new(entries.clone()),
            removed: std::cell::RefCell::new(Vec::new()),
        };
        let removed = rewriter
            .remove(
                std::path::Path::new("ledger/waivers.toml"),
                "WVR-20261101-first",
            )
            .expect("present entry removes");
        assert_eq!(
            removed.map(|w| w.id),
            Some("WVR-20261101-first".to_string())
        );
        assert_eq!(
            rewriter.removed.borrow().len(),
            1,
            "the removal is recorded for the audit"
        );
        assert_eq!(rewriter.doc.borrow().len(), 1);
    }

    #[test]
    fn revoke_text_and_json_faces_render_deterministically() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let root = seeded_root(&entries);
        let outcome = super::run_waivers_revoke(
            &root,
            &WaiversRevokeRequest {
                id: "WVR-20261109-third".to_string(),
            },
            &FileWaiverSource,
            &FileWaiverRewriter,
        )
        .expect("revoke succeeds");

        let text_a = super::format_waivers_revoke_text(&outcome);
        let text_b = super::format_waivers_revoke_text(&outcome);
        assert_eq!(text_a, text_b);
        assert!(text_a.contains("revoked WVR-20261109-third"));
        assert!(text_a.contains("1 waiver(s) remain"));

        let json_a = super::format_waivers_revoke_json(&outcome);
        let json_b = super::format_waivers_revoke_json(&outcome);
        assert_eq!(json_a, json_b);
        let document: serde_json::Value = serde_json::from_str(&json_a).expect("valid JSON");
        assert_eq!(document["schema"], "glyim.ledger-waivers-revoke/1");
        assert_eq!(document["id"], "WVR-20261109-third");
        assert_eq!(document["remaining"], 1);
        assert!(json_a.ends_with('\n'));
    }

    #[test]
    fn parse_args_accepts_waivers_revoke_flags() {
        let command = parse_args(
            [
                "waivers",
                "revoke",
                "/tmp/repo",
                "--id",
                "WVR-20261101-first",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("revoke with positional root");
        assert_eq!(
            command,
            Command::WaiversRevoke {
                root: PathBuf::from("/tmp/repo"),
                id: "WVR-20261101-first".to_string(),
                format: WaiversFormat::Text,
            }
        );
        let command = parse_args(
            [
                "waivers",
                "revoke",
                "--id",
                "WVR-20261101-first",
                "--format",
                "json",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("revoke with json format");
        assert_eq!(
            command,
            Command::WaiversRevoke {
                root: PathBuf::from("."),
                id: "WVR-20261101-first".to_string(),
                format: WaiversFormat::Json,
            }
        );
    }

    #[test]
    fn parse_args_rejects_bad_waivers_revoke_flags() {
        let err = parse_args(["waivers", "revoke"].iter().map(|s| s.to_string()))
            .expect_err("missing --id fails");
        assert!(matches!(
            err,
            LedgerCliError::WaiversRevokeMissingField { .. }
        ));
        let err = parse_args(["waivers", "revoke", "--id"].iter().map(|s| s.to_string()))
            .expect_err("a valueless --id fails");
        assert!(matches!(
            err,
            LedgerCliError::WaiversRevokeMissingField { .. }
        ));
        let err = parse_args(
            ["waivers", "revoke", "--id", "X", "--format", "yaml"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("bad format fails");
        assert!(matches!(err, LedgerCliError::WaiversBadFormat { .. }));
        let err = parse_args(
            ["waivers", "revoke", "--unknown", "x"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("unknown flag fails");
        assert!(matches!(err, LedgerCliError::UnknownFlag { .. }));
    }

    #[test]
    fn revoke_error_variants_carry_code_and_help() {
        use miette::Diagnostic as _;
        let variants = [
            LedgerCliError::WaiversRevokeMissingField {
                field: "--id".to_string(),
                usage: usage(),
            },
            LedgerCliError::WaiversRevokeBadId {
                id: "bad".to_string(),
            },
            LedgerCliError::WaiversRevokeUnknownId {
                id: "WVR-absent".to_string(),
                candidates: vec!["WVR-present".to_string()],
            },
            LedgerCliError::WaiversRevokeLoad {
                detail: "duplicate id".to_string(),
            },
            LedgerCliError::WaiversRevokeWrite {
                path: "ledger/waivers.toml".to_string(),
                detail: "readonly".to_string(),
            },
        ];
        for variant in &variants {
            let code = variant.code().map(|c| c.to_string()).expect("code");
            assert!(code.starts_with("glyim-ledger::waivers_revoke_"));
            assert!(variant.help().is_some(), "{code} carries a help line");
        }
    }

    #[test]
    fn usage_lists_waivers_revoke() {
        assert!(usage().contains("waivers revoke"));
    }

    // -- extend face ------------------------------------------------------------

    use super::{
        FileWaiverExtender, InMemoryWaiverExtender, WaiverExtender, WaiversExtendRequest,
        run_waivers_extend,
    };
    use crate::diff::DEFAULT_WAIVER_DAYS;

    #[test]
    fn extend_repins_expiry_and_keeps_identity() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let source = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: entries.clone(),
                diagnostics: Vec::new(),
            },
        };
        let extender = InMemoryWaiverExtender {
            doc: std::cell::RefCell::new(entries.clone()),
            extended: std::cell::RefCell::new(Vec::new()),
        };
        let outcome = run_waivers_extend(
            Path::new("."),
            &WaiversExtendRequest {
                id: "WVR-20261101-first".to_string(),
                days: 10,
            },
            &source,
            &extender,
        )
        .expect("extend succeeds");
        assert_eq!(outcome.schema, "glyim.ledger-waivers-extend/1");
        assert_eq!(outcome.id, "WVR-20261101-first");
        assert_eq!(outcome.feature, "LANG-A", "identity is kept");
        assert_eq!(outcome.previous_expires, today().plus_days(5).to_iso());
        assert_eq!(
            outcome.expires,
            today().plus_days(10).to_iso(),
            "the new expiry is pinned relative to today"
        );
        assert_eq!(outcome.days, 10);
        assert_eq!(outcome.remaining, 2, "an extension never changes the count");
        assert_eq!(extender.extended.borrow().len(), 1);
        assert_eq!(extender.doc.borrow()[0].expires, outcome.expires);
        assert_eq!(
            extender.doc.borrow()[0].reason,
            entries[0].reason,
            "reason survives the rewrite"
        );
        assert_eq!(
            extender.doc.borrow()[1].expires,
            today().plus_days(1).to_iso(),
            "the sibling entry is untouched"
        );
    }

    #[test]
    fn extend_rejects_days_outside_the_c9_horizon() {
        let entries = vec![entry("WVR-20261101-first", "LANG-A", 5)];
        let source = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: entries,
                diagnostics: Vec::new(),
            },
        };
        let extender = InMemoryWaiverExtender::default();
        for days in [0i64, -3, WAIVER_MAX_AGE_DAYS + 1] {
            let err = run_waivers_extend(
                Path::new("."),
                &WaiversExtendRequest {
                    id: "WVR-20261101-first".to_string(),
                    days,
                },
                &source,
                &extender,
            )
            .expect_err("days outside 1..=14 fail");
            assert!(
                matches!(err, LedgerCliError::WaiversExtendBadDays { .. }),
                "days = {days} fails as WaiversExtendBadDays"
            );
        }
        assert!(
            extender.extended.borrow().is_empty(),
            "nothing is written when validation fails"
        );
    }

    #[test]
    fn extend_rejects_blank_or_control_id() {
        let entries = vec![entry("WVR-20261101-first", "LANG-A", 5)];
        let source = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: entries,
                diagnostics: Vec::new(),
            },
        };
        let extender = InMemoryWaiverExtender::default();
        for id in ["", "  ", "bad\nid"] {
            let err = run_waivers_extend(
                Path::new("."),
                &WaiversExtendRequest {
                    id: id.to_string(),
                    days: 5,
                },
                &source,
                &extender,
            )
            .expect_err("blank or control-character ID fails");
            assert!(matches!(err, LedgerCliError::WaiversExtendBadId { .. }));
        }
        assert!(extender.extended.borrow().is_empty());
    }

    #[test]
    fn extend_refuses_a_document_with_load_errors() {
        let mut set = WaiverSet::empty();
        set.diagnostics.push(Diagnostic {
            code: "LEDGER-WAIVER-DUP",
            message: "waiver id \"WVR-X\" is declared twice".to_string(),
            severity: Severity::Error,
        });
        let source = InMemoryWaiverSource { set };
        let extender = InMemoryWaiverExtender::default();
        let err = run_waivers_extend(
            Path::new("."),
            &WaiversExtendRequest {
                id: "WVR-20261101-first".to_string(),
                days: 5,
            },
            &source,
            &extender,
        )
        .expect_err("a dirty document is refused");
        assert!(matches!(err, LedgerCliError::WaiversExtendLoad { .. }));
        assert!(
            extender.extended.borrow().is_empty(),
            "the extender refuses to rewrite what it cannot fully parse"
        );
    }

    #[test]
    fn extend_unknown_id_lists_candidates_and_writes_nothing() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let source = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: entries,
                diagnostics: Vec::new(),
            },
        };
        let extender = InMemoryWaiverExtender::default();
        let err = run_waivers_extend(
            Path::new("."),
            &WaiversExtendRequest {
                id: "WVR-absent".to_string(),
                days: 5,
            },
            &source,
            &extender,
        )
        .expect_err("unknown ID fails");
        match err {
            LedgerCliError::WaiversExtendUnknownId { candidates, .. } => {
                assert_eq!(candidates, vec!["WVR-20261101-first", "WVR-20261109-third"]);
            }
            other => panic!("expected WaiversExtendUnknownId, got {other:?}"),
        }
        assert!(extender.extended.borrow().is_empty());
    }

    #[test]
    fn extend_reports_drift_without_a_partial_write() {
        // The loaded set carries the ID; the document the extender sees
        // does not (the load pass and the rewrite drifted apart).
        let entries = vec![entry("WVR-20261101-first", "LANG-A", 5)];
        let source = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: entries,
                diagnostics: Vec::new(),
            },
        };
        let extender = InMemoryWaiverExtender {
            doc: std::cell::RefCell::new(Vec::new()),
            extended: std::cell::RefCell::new(Vec::new()),
        };
        let err = run_waivers_extend(
            Path::new("."),
            &WaiversExtendRequest {
                id: "WVR-20261101-first".to_string(),
                days: 5,
            },
            &source,
            &extender,
        )
        .expect_err("drift between load and rewrite fails");
        match err {
            LedgerCliError::WaiversExtendUnknownId { candidates, .. } => {
                assert!(candidates.is_empty(), "no candidates after drift");
            }
            other => panic!("expected WaiversExtendUnknownId, got {other:?}"),
        }
        assert!(extender.extended.borrow().is_empty());
    }

    #[test]
    fn file_extender_round_trips_the_canonical_document() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let root = seeded_root(&entries);
        let outcome = run_waivers_extend(
            &root,
            &WaiversExtendRequest {
                id: "WVR-20261101-first".to_string(),
                days: 12,
            },
            &FileWaiverSource,
            &FileWaiverExtender,
        )
        .expect("extend succeeds against the seeded document");

        // The rewritten document still parses through the same loader the
        // diff gate uses, carries both entries, and only the target moved.
        let reloaded = super::load_waivers(&root.join("ledger/waivers.toml"), today());
        assert!(
            reloaded.diagnostics.is_empty(),
            "{:?}",
            reloaded.diagnostics
        );
        assert_eq!(reloaded.waivers.len(), 2);
        let first = reloaded
            .waivers
            .iter()
            .find(|w| w.id == "WVR-20261101-first")
            .expect("entry survives the rewrite");
        assert_eq!(first.expires, outcome.expires);
        assert_eq!(first.reason, entries[0].reason, "reason survives");
        let second = reloaded
            .waivers
            .iter()
            .find(|w| w.id == "WVR-20261109-third")
            .expect("sibling survives the rewrite");
        assert_eq!(second.expires, entries[1].expires, "sibling untouched");
    }

    #[test]
    fn in_memory_extender_records_the_extension() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let extender = InMemoryWaiverExtender {
            doc: std::cell::RefCell::new(entries.clone()),
            extended: std::cell::RefCell::new(Vec::new()),
        };
        let new_expires = today().plus_days(10).to_iso();
        let extended = extender
            .extend_expiry(
                std::path::Path::new("ledger/waivers.toml"),
                "WVR-20261101-first",
                &new_expires,
            )
            .expect("present entry extends");
        assert_eq!(
            extended.as_ref().map(|w| w.expires.as_str()),
            Some(new_expires.as_str())
        );
        assert_eq!(
            extender.extended.borrow().len(),
            1,
            "the extension is recorded for the audit"
        );
        assert_eq!(extender.doc.borrow().len(), 2);
        // An absent entry reports None and records nothing.
        let absent = extender
            .extend_expiry(
                std::path::Path::new("ledger/waivers.toml"),
                "WVR-absent",
                &new_expires,
            )
            .expect("absent entry is a None, not an error");
        assert!(absent.is_none());
        assert_eq!(extender.extended.borrow().len(), 1);
    }

    #[test]
    fn extend_text_and_json_faces_render_deterministically() {
        let entries = vec![
            entry("WVR-20261101-first", "LANG-A", 5),
            entry("WVR-20261109-third", "LANG-C", 1),
        ];
        let root = seeded_root(&entries);
        let outcome = run_waivers_extend(
            &root,
            &WaiversExtendRequest {
                id: "WVR-20261101-first".to_string(),
                days: DEFAULT_WAIVER_DAYS,
            },
            &FileWaiverSource,
            &FileWaiverExtender,
        )
        .expect("extend succeeds");

        let text_a = super::format_waivers_extend_text(&outcome);
        let text_b = super::format_waivers_extend_text(&outcome);
        assert_eq!(text_a, text_b);
        assert!(text_a.contains("extended WVR-20261101-first"));
        assert!(text_a.contains("-> 2 waiver(s) remain"));

        let json_a = super::format_waivers_extend_json(&outcome);
        let json_b = super::format_waivers_extend_json(&outcome);
        assert_eq!(json_a, json_b);
        let document: serde_json::Value = serde_json::from_str(&json_a).expect("valid JSON");
        assert_eq!(document["schema"], "glyim.ledger-waivers-extend/1");
        assert_eq!(document["id"], "WVR-20261101-first");
        assert_eq!(
            document["previous_expires"],
            today().plus_days(5).to_iso(),
            "the audit trail carries the previous expiry"
        );
        assert_eq!(document["days"], DEFAULT_WAIVER_DAYS);
        assert_eq!(document["remaining"], 2);
        assert!(json_a.ends_with('\n'));
    }

    #[test]
    fn parse_args_accepts_waivers_extend_flags() {
        let command = parse_args(
            [
                "waivers",
                "extend",
                "/tmp/repo",
                "--id",
                "WVR-20261101-first",
                "--days",
                "5",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("the extend flags parse");
        assert!(matches!(
            command,
            Command::WaiversExtend {
                ref root,
                ref id,
                days: Some(5),
                format: WaiversFormat::Text,
            } if root == &PathBuf::from("/tmp/repo")
                && id == "WVR-20261101-first"
        ));

        // `--days` is optional: the CLI default resolves in the dispatch
        // layer (`DEFAULT_WAIVER_DAYS`), never in the parser.
        let command = parse_args(
            [
                "waivers",
                "extend",
                "--id",
                "WVR-20261101-first",
                "--format",
                "json",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("the minimal extend flags parse");
        assert!(matches!(
            command,
            Command::WaiversExtend {
                days: None,
                format: WaiversFormat::Json,
                ..
            }
        ));
    }

    #[test]
    fn parse_args_rejects_incomplete_waivers_extend() {
        // A missing --id is a parse error, not an engine rejection.
        let err = parse_args(["waivers", "extend"].into_iter().map(String::from))
            .expect_err("missing --id fails at parse time");
        assert!(matches!(
            err,
            LedgerCliError::WaiversExtendMissingField { .. }
        ));
        // A non-numeric --days is a parse error naming the horizon.
        let err = parse_args(
            ["waivers", "extend", "--id", "WVR-X", "--days", "soon"]
                .into_iter()
                .map(String::from),
        )
        .expect_err("non-numeric --days fails at parse time");
        assert!(matches!(err, LedgerCliError::WaiversExtendBadDays { .. }));
        // An unknown format value stays the shared WaiversBadFormat.
        let err = parse_args(
            ["waivers", "extend", "--id", "WVR-X", "--format", "yaml"]
                .into_iter()
                .map(String::from),
        )
        .expect_err("unknown format fails");
        assert!(matches!(err, LedgerCliError::WaiversBadFormat { .. }));
    }

    #[test]
    fn extend_error_variants_carry_code_and_help() {
        use miette::Diagnostic as _;
        let variants = [
            LedgerCliError::WaiversExtendMissingField {
                field: "--id".to_string(),
                usage: usage(),
            },
            LedgerCliError::WaiversExtendBadDays {
                days: "15".to_string(),
                horizon: WAIVER_MAX_AGE_DAYS,
            },
            LedgerCliError::WaiversExtendBadId {
                id: "bad".to_string(),
            },
            LedgerCliError::WaiversExtendUnknownId {
                id: "WVR-absent".to_string(),
                candidates: vec!["WVR-present".to_string()],
            },
            LedgerCliError::WaiversExtendLoad {
                detail: "duplicate id".to_string(),
            },
            LedgerCliError::WaiversExtendWrite {
                path: "ledger/waivers.toml".to_string(),
                detail: "readonly".to_string(),
            },
        ];
        for variant in &variants {
            let code = variant.code().map(|c| c.to_string()).expect("code");
            assert!(code.starts_with("glyim-ledger::waivers_extend_"));
            assert!(variant.help().is_some(), "{code} carries a help line");
        }
    }

    #[test]
    fn usage_lists_waivers_extend() {
        assert!(usage().contains("waivers extend"));
    }

    // -- prune face -------------------------------------------------------------

    use std::fmt::Write as _;

    use super::{
        FileWaiverPruner, InMemoryWaiverPruner, WAIVERS_PRUNE_SCHEMA, format_waivers_prune_json,
        format_waivers_prune_text, run_waivers_prune,
    };
    use crate::diff::load_waivers;

    /// One document entry with an explicit expiry.
    fn prune_entry(id: &str, feature: &str, expires: &str) -> WaiverEntry {
        WaiverEntry {
            id: id.to_string(),
            feature: feature.to_string(),
            reason: "prune walkthrough exception".to_string(),
            issue: "https://github.com/elcoosp/glyim/issues/42".to_string(),
            expires: expires.to_string(),
        }
    }

    /// The prune fixture document: one valid (2026-11-20) and two expired
    /// (2026-11-11, 2026-11-05) entries, deliberately unsorted.
    fn prune_doc() -> Vec<WaiverEntry> {
        vec![
            prune_entry("WVR-20261110-beta", "LANG-BETA", "2026-11-11"),
            prune_entry("WVR-20261107-alpha", "LANG-ALPHA", "2026-11-20"),
            prune_entry("WVR-20261105-gamma", "LANG-GAMMA", "2026-11-05"),
        ]
    }

    /// An in-memory source over [`prune_doc`].
    fn prune_source() -> InMemoryWaiverSource {
        InMemoryWaiverSource {
            set: WaiverSet {
                waivers: prune_doc(),
                diagnostics: Vec::new(),
            },
        }
    }

    #[test]
    fn prune_removes_only_expired_blocks_canonically() {
        let pruner = InMemoryWaiverPruner {
            doc: std::cell::RefCell::new(prune_doc()),
            pruned: std::cell::RefCell::new(Vec::new()),
        };
        let outcome =
            run_waivers_prune(Path::new("/proj"), &prune_source(), &pruner).expect("prune");
        let ids: Vec<&str> = outcome.removed.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["WVR-20261105-gamma", "WVR-20261110-beta"], "ID order");
        assert_eq!(outcome.remaining, 1, "the one valid entry stays");
        assert_eq!(outcome.schema, WAIVERS_PRUNE_SCHEMA);
        let doc = pruner.doc.borrow();
        assert_eq!(doc.len(), 1);
        assert_eq!(doc[0].id, "WVR-20261107-alpha", "the valid entry survives");
        let audit = pruner.pruned.borrow();
        assert_eq!(audit.len(), 1, "the prune is recorded");
        assert_eq!(
            audit[0].1,
            [
                "WVR-20261105-gamma".to_string(),
                "WVR-20261110-beta".to_string()
            ]
        );
    }

    #[test]
    fn prune_rows_echo_the_pruned_identity() {
        let pruner = InMemoryWaiverPruner {
            doc: std::cell::RefCell::new(prune_doc()),
            pruned: std::cell::RefCell::new(Vec::new()),
        };
        let outcome =
            run_waivers_prune(Path::new("/proj"), &prune_source(), &pruner).expect("prune");
        assert_eq!(outcome.removed[0].feature, "LANG-GAMMA");
        assert_eq!(outcome.removed[0].expires, "2026-11-05");
        assert_eq!(outcome.target, "/proj/ledger/waivers.toml");
    }

    #[test]
    fn prune_with_no_expired_waiver_never_touches_the_document() {
        let clean = vec![prune_entry(
            "WVR-20261107-alpha",
            "LANG-ALPHA",
            "2026-11-20",
        )];
        let pruner = InMemoryWaiverPruner {
            doc: std::cell::RefCell::new(clean.clone()),
            pruned: std::cell::RefCell::new(Vec::new()),
        };
        let source = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: clean,
                diagnostics: Vec::new(),
            },
        };
        let outcome = run_waivers_prune(Path::new("/proj"), &source, &pruner).expect("clean prune");
        assert!(
            outcome.removed.is_empty(),
            "nothing expired: nothing pruned"
        );
        assert_eq!(outcome.remaining, 1);
        assert!(
            pruner.pruned.borrow().is_empty(),
            "the seam is not invoked on a clean document"
        );
        assert_eq!(pruner.doc.borrow().len(), 1, "the document is untouched");
    }

    #[test]
    fn prune_refuses_a_document_that_does_not_load_cleanly() {
        let source = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: Vec::new(),
                diagnostics: vec![Diagnostic {
                    code: "LEDGER-WAIVER-DUP",
                    message: "waiver id declared twice".to_string(),
                    severity: Severity::Error,
                }],
            },
        };
        let pruner = InMemoryWaiverPruner::default();
        let err = run_waivers_prune(Path::new("/proj"), &source, &pruner)
            .expect_err("a non-clean load refuses the prune");
        assert!(matches!(err, LedgerCliError::WaiversPruneLoad { .. }));
        assert!(pruner.doc.borrow().is_empty(), "nothing was rewritten");
    }

    #[test]
    fn file_pruner_rewrites_the_committed_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");
        let mut doc = String::new();
        for entry in prune_doc() {
            let _ = write!(
                &mut doc,
                "[[waiver]]\n\
                 id = \"{}\"\n\
                 feature = \"{}\"\n\
                 reason = \"{}\"\n\
                 issue = \"{}\"\n\
                 expires = \"{}\"\n\n",
                entry.id, entry.feature, entry.reason, entry.issue, entry.expires,
            );
        }
        let path = root.join("ledger/waivers.toml");
        std::fs::write(&path, &doc).expect("seed waivers.toml");

        let outcome =
            run_waivers_prune(&root, &FileWaiverSource, &FileWaiverPruner).expect("file prune");
        assert_eq!(outcome.removed.len(), 2, "both expired entries pruned");
        assert_eq!(outcome.remaining, 1);

        // The rewritten document loads cleanly and carries only the valid
        // entry (the health face sees one valid waiver, no expired one).
        let reloaded = load_waivers(&path, NaiveDate::new_unchecked(2026, 11, 13));
        assert!(reloaded.diagnostics.is_empty(), "canonical rewrite loads");
        assert_eq!(reloaded.waivers.len(), 1);
        assert_eq!(reloaded.waivers[0].id, "WVR-20261107-alpha");
    }

    #[test]
    fn file_pruner_leaves_a_clean_document_byte_identical() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");
        let entry = prune_entry("WVR-20261107-alpha", "LANG-ALPHA", "2026-11-20");
        let doc = format!(
            "[[waiver]]\n\
             id = \"{}\"\n\
             feature = \"{}\"\n\
             reason = \"{}\"\n\
             issue = \"{}\"\n\
             expires = \"{}\"\n",
            entry.id, entry.feature, entry.reason, entry.issue, entry.expires,
        );
        let path = root.join("ledger/waivers.toml");
        std::fs::write(&path, &doc).expect("seed waivers.toml");

        let outcome = run_waivers_prune(&root, &FileWaiverSource, &FileWaiverPruner)
            .expect("clean file prune");
        assert!(outcome.removed.is_empty());
        let after = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(after, doc, "a clean document is never rewritten");
    }

    #[test]
    fn format_waivers_prune_text_names_the_pruned_rows() {
        let pruner = InMemoryWaiverPruner {
            doc: std::cell::RefCell::new(prune_doc()),
            pruned: std::cell::RefCell::new(Vec::new()),
        };
        let outcome =
            run_waivers_prune(Path::new("/proj"), &prune_source(), &pruner).expect("prune");
        let text = format_waivers_prune_text(&outcome);
        assert!(text.contains("pruned 2 expired waiver(s)"), "{text}");
        assert!(text.contains("1 waiver(s) remain"), "{text}");
        assert!(text.contains("- WVR-20261105-gamma for LANG-GAMMA (expires 2026-11-05)"));
        assert!(text.contains("- WVR-20261110-beta for LANG-BETA (expires 2026-11-11)"));

        let clean = InMemoryWaiverSource {
            set: WaiverSet {
                waivers: vec![prune_entry(
                    "WVR-20261107-alpha",
                    "LANG-ALPHA",
                    "2026-11-20",
                )],
                diagnostics: Vec::new(),
            },
        };
        let pruner = InMemoryWaiverPruner::default();
        let outcome = run_waivers_prune(Path::new("/proj"), &clean, &pruner).expect("clean");
        let text = format_waivers_prune_text(&outcome);
        assert!(text.contains("no expired waiver to prune"), "{text}");
    }

    #[test]
    fn format_waivers_prune_json_parses_back_with_schema_and_counts() {
        let pruner = InMemoryWaiverPruner {
            doc: std::cell::RefCell::new(prune_doc()),
            pruned: std::cell::RefCell::new(Vec::new()),
        };
        let outcome =
            run_waivers_prune(Path::new("/proj"), &prune_source(), &pruner).expect("prune");
        let text = format_waivers_prune_json(&outcome);
        let document: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(document["schema"], WAIVERS_PRUNE_SCHEMA);
        assert_eq!(document["removed"].as_array().map(Vec::len), Some(2));
        assert_eq!(document["removed"][0]["id"], "WVR-20261105-gamma");
        assert_eq!(document["removed"][0]["feature"], "LANG-GAMMA");
        assert_eq!(document["remaining"], 1);
        assert_eq!(document["target"], "/proj/ledger/waivers.toml");
    }

    #[test]
    fn parse_args_accepts_waivers_prune() {
        let command = parse_args(["waivers", "prune"].into_iter().map(String::from))
            .expect("bare prune face");
        assert_eq!(
            command,
            Command::WaiversPrune {
                root: PathBuf::from("."),
                format: WaiversFormat::Text,
            }
        );
        let command = parse_args(
            ["waivers", "prune", "/tmp/repo", "--format", "json"]
                .into_iter()
                .map(String::from),
        )
        .expect("root and format");
        assert_eq!(
            command,
            Command::WaiversPrune {
                root: PathBuf::from("/tmp/repo"),
                format: WaiversFormat::Json,
            }
        );
    }

    #[test]
    fn parse_args_rejects_bad_waivers_prune() {
        let err = parse_args(
            ["waivers", "prune", "--format", "yaml"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("bad format");
        assert!(matches!(err, LedgerCliError::WaiversBadFormat { .. }));
        let err = parse_args(
            ["waivers", "prune", "--format"]
                .iter()
                .map(|s| s.to_string()),
        )
        .expect_err("missing value");
        assert!(matches!(err, LedgerCliError::WaiversMissingValue { .. }));
        let err = parse_args(["waivers", "prune", "a", "b"].iter().map(|s| s.to_string()))
            .expect_err("two positional roots");
        assert!(matches!(err, LedgerCliError::ExtraArgument { .. }));
    }

    #[test]
    fn waivers_prune_error_variants_carry_code_and_help() {
        use miette::Diagnostic as _;
        let variants = [
            LedgerCliError::WaiversPruneLoad {
                detail: "duplicate id".to_string(),
            },
            LedgerCliError::WaiversPruneWrite {
                path: "ledger/waivers.toml".to_string(),
                detail: "readonly".to_string(),
            },
        ];
        for variant in &variants {
            let code = variant.code().map(|c| c.to_string()).expect("code");
            assert!(code.starts_with("glyim-ledger::waivers_prune_"));
            assert!(variant.help().is_some(), "{code} carries a help line");
        }
    }

    #[test]
    fn usage_lists_waivers_prune() {
        assert!(usage().contains("waivers prune"));
    }
}
