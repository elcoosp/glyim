//! Readiness-regression diff and the waiver gate (plan M1.10 first slice).
//!
//! `compute.rs` computes per-feature readiness levels, but nothing compares
//! two computed states — the plan §6 M1.10 `ledger diff` regression gate
//! needs exactly that: "regressions block merge in a test PR", with
//! temporary exceptions carried by `ledger/waivers.toml` (constitution C9:
//! thresholds only tighten; waivers expire in ≤ 14 days). This module is
//! that seam, split the way `compute.rs` separates input sources from pure
//! computation:
//!
//! - [`SnapshotStore`] abstracts *where readiness snapshots come from and
//!   go*. [`FileSnapshotStore`] (a `glyim.ledger-snapshot/1` document on
//!   disk) backs the CLI; [`InMemorySnapshotStore`] backs tests and
//!   in-process consumers. A future CI history store plugs in here without
//!   touching the engine.
//! - [`diff_levels`] is the pure function `(baseline, current, waivers,
//!   today) → per-feature level deltas, verdicts, and the gate` — no
//!   filesystem access, sorted output (plan C6).
//! - [`execute_diff`] wires the two: load both sides through the store,
//!   diff, and wrap in a [`LedgerDiffOutcome`]. [`run_diff`] is the
//!   project entry point: waivers come from `ledger/waivers.toml` and
//!   today's date from the clock.
//!
//! Gate semantics (constitution C9): a feature whose computed level dropped
//! (or disappeared from the registry) between baseline and current is
//! [`LevelVerdict::Regressed`] and fails the gate unless an unexpired
//! waiver covers it. Improvements ([`LevelVerdict::Improved`]) never fail
//! the gate; an expired waiver that *would* have covered a regression is
//! itself an error diagnostic ([`CODE_WAIVER_EXPIRED`]) so a stale
//! `waivers.toml` can never mask a real regression.

use std::path::{Path, PathBuf};

use ledger_schema::{Diagnostic, Level, Severity, Tier};

use crate::compute::ComputeOutcome;

/// Schema tag pinned for the diff outcome shape.
pub const LEDGER_DIFF_SCHEMA: &str = "glyim.ledger-diff/1";

/// Schema tag pinned for the persisted readiness-snapshot shape.
pub const SNAPSHOT_SCHEMA: &str = "glyim.ledger-snapshot/1";

/// Constitution C9: a waiver may not be granted for more than this many
/// days ahead (thresholds only tighten; long-lived exceptions rot).
pub const WAIVER_MAX_AGE_DAYS: i64 = 14;

/// Diagnostic code: a snapshot document could not be read from disk.
pub const CODE_SNAPSHOT_IO: &str = "LEDGER-SNAPSHOT-IO";

/// Diagnostic code: a snapshot document did not parse or carried the wrong
/// schema tag.
pub const CODE_SNAPSHOT_SCHEMA: &str = "LEDGER-SNAPSHOT-SCHEMA";

/// Diagnostic code: `waivers.toml` did not parse or an entry is malformed.
pub const CODE_WAIVER_SCHEMA: &str = "LEDGER-WAIVER-SCHEMA";

/// Diagnostic code: two waivers share an ID.
pub const CODE_WAIVER_DUP: &str = "LEDGER-WAIVER-DUP";

/// Diagnostic code: a waiver's expiry lies beyond the C9 horizon
/// (`today + WAIVER_MAX_AGE_DAYS`); the waiver is inert.
pub const CODE_WAIVER_HORIZON: &str = "LEDGER-WAIVER-HORIZON";

/// Diagnostic code: an expired waiver covered a regression; the waiver
/// cannot mask it, so the gate fails.
pub const CODE_WAIVER_EXPIRED: &str = "LEDGER-WAIVER-EXPIRED";

/// Diagnostic code: a waiver in `waivers.toml` matched no regression in
/// this diff (hygiene: remove it — C9 ratchet hygiene, warning severity).
pub const CODE_WAIVER_UNUSED: &str = "LEDGER-WAIVER-UNUSED";

// -- calendar arithmetic ----------------------------------------------------
//
// Hand-rolled (no `chrono`): the ledger's dependency-light convention. The
// algorithms are Howard Hinnant's `days_from_civil`/`civil_from_days`
// (proleptic Gregorian), pinned by round-trip tests below.

/// A civil calendar date (proleptic Gregorian). The diff engine's `today`
/// parameter: waiver expiry is date arithmetic, not clock arithmetic, so
/// the engine takes the date as a value and only `run_diff` consults the
/// wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NaiveDate {
    /// Calendar year (can be negative).
    pub year: i32,
    /// Month, 1..=12.
    pub month: u8,
    /// Day of month, 1..=31.
    pub day: u8,
}

impl NaiveDate {
    /// Builds a date without validating (const, for literal fixtures);
    /// validation lives in [`NaiveDate::parse_iso`] and the private
    /// `is_valid` check.
    #[must_use]
    pub const fn new_unchecked(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// Days since 1970-01-01 (negative before the epoch).
    #[must_use]
    pub fn to_days(self) -> i64 {
        days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        )
    }

    /// Inverse of [`NaiveDate::to_days`].
    #[must_use]
    pub fn from_days(days: i64) -> Self {
        let (year, month, day) = civil_from_days(days);
        Self {
            year: i32::try_from(year).unwrap_or(0),
            month: month as u8,
            day: day as u8,
        }
    }

    /// Parses the `YYYY-MM-DD` ISO wire spelling. Rejects out-of-range
    /// months/days and non-existent calendar dates (`2023-02-29`).
    #[must_use]
    pub fn parse_iso(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        for (index, byte) in bytes.iter().enumerate() {
            if index != 4 && index != 7 && !byte.is_ascii_digit() {
                return None;
            }
        }
        let year: i32 = text[..4].parse().ok()?;
        let month: u8 = text[5..7].parse().ok()?;
        let day: u8 = text[8..10].parse().ok()?;
        let candidate = Self::new_unchecked(year, month, day);
        // Round-trip through the day-number domain: this rejects
        // `2023-02-29` and friends because `civil_from_days` renormalizes.
        if candidate.month == 0 || candidate.month > 12 || candidate.day == 0 {
            return None;
        }
        let reparsed = Self::from_days(candidate.to_days());
        (reparsed == candidate).then_some(candidate)
    }

    /// The `YYYY-MM-DD` ISO wire spelling.
    #[must_use]
    pub fn to_iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `self` shifted by `days` (negative shifts into the past).
    #[must_use]
    pub fn plus_days(self, days: i64) -> Self {
        Self::from_days(self.to_days() + days)
    }

    /// Whether the fields are in range (month 1..=12, day 1..=31); real
    /// calendar existence is checked by round-trip in [`Self::parse_iso`].
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.month >= 1 && self.month <= 12 && self.day >= 1 && self.day <= 31
    }
}

/// `days_from_civil` (Hinnant 2017): a proleptic-Gregorian date to a day
/// number relative to 1970-01-01.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let shifted_year = year - i64::from(month <= 2);
    let era = if shifted_year >= 0 {
        shifted_year
    } else {
        shifted_year - 399
    } / 400;
    let year_of_era = shifted_year - era * 400;
    let month_of_year = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_of_year + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `civil_from_days` (Hinnant 2017): a day number relative to 1970-01-01
/// back to `(year, month, day)`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_of_year = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_of_year + 2) / 5 + 1;
    let month = month_of_year + if month_of_year < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

/// Today's date in the `YYYY-MM-DD` ISO spelling (wall clock, UTC day).
#[must_use]
pub fn today_iso() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0);
    // Floor-division by the day length: UTC days since the epoch.
    NaiveDate::from_days(seconds.div_euclid(86_400)).to_iso()
}

// -- snapshot wire ----------------------------------------------------------

/// One feature's readiness level in a persisted snapshot (one JSON `entries`
/// item). The `glyim.ledger-snapshot/1` wire shape: levels, not explanations
/// (the `missing` reason prose is deliberately absent, same decision as the
/// `glyim.status/1` JSON).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEntry {
    /// Registry feature ID.
    pub id: String,
    /// Maturity tier (echoed for consumers sorting by tier).
    pub tier: Tier,
    /// The level the registry demands.
    pub target_level: Level,
    /// The level the evidence supported when the snapshot was taken.
    pub computed_level: Level,
    /// Gap IDs that capped this feature (sorted; empty when uncapped).
    pub capped_by: Vec<String>,
}

/// A persisted readiness state: the input of [`diff_levels`] and the output
/// of `compute --snapshot`. Sorted-by-ID is an invariant of
/// [`ComputeSnapshot::from_outcome`]; hand-built snapshots may carry any
/// order (the engine sorts its view).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeSnapshot {
    /// Schema tag, always [`SNAPSHOT_SCHEMA`].
    pub schema: String,
    /// Per-feature readiness entries (sorted by ID in produced snapshots).
    pub entries: Vec<SnapshotEntry>,
}

impl From<&ComputeOutcome> for ComputeSnapshot {
    fn from(outcome: &ComputeOutcome) -> Self {
        let mut entries: Vec<SnapshotEntry> = outcome
            .entries
            .iter()
            .map(|level| SnapshotEntry {
                id: level.id.clone(),
                tier: level.tier,
                target_level: level.target_level,
                computed_level: level.computed_level,
                capped_by: level.capped_by.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            schema: SNAPSHOT_SCHEMA.to_string(),
            entries,
        }
    }
}

impl ComputeSnapshot {
    /// Snapshots one compute outcome (sorted by ID, prose dropped).
    #[must_use]
    pub fn from_outcome(outcome: &ComputeOutcome) -> Self {
        Self::from(outcome)
    }
}

// -- snapshot store ----------------------------------------------------------

/// The result of loading one snapshot: the document (when readable and
/// valid) plus the load diagnostics. Failures are diagnostics, never panics
/// — the same channel convention as [`crate::compute::EvidenceBatch`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotBatch {
    /// The loaded snapshot, or `None` when the load failed.
    pub snapshot: Option<ComputeSnapshot>,
    /// Load diagnostics (sorted; one or more per failure).
    pub diagnostics: Vec<Diagnostic>,
}

/// Where readiness snapshots come from and go.
///
/// The seam that lets CI history provide baselines (a file tree today, a
/// committed snapshot or a remote history later) without the diff engine
/// knowing. Both methods take `&self`; implementations own their interior
/// mutability, mirroring the bench harness's `ReportStore`.
pub trait SnapshotStore {
    /// Load one `glyim.ledger-snapshot/1` document; failures are
    /// [`CODE_SNAPSHOT_IO`] / [`CODE_SNAPSHOT_SCHEMA`] diagnostics.
    fn load(&self, path: &Path) -> SnapshotBatch;

    /// Persist one snapshot; failures surface as the same diagnostic codes
    /// through the `Err` arm (the save counterpart of `compute --snapshot`).
    ///
    /// # Errors
    ///
    /// A [`CODE_SNAPSHOT_IO`] (unwritable path) or [`CODE_SNAPSHOT_SCHEMA`]
    /// (serialization) diagnostic naming the failing path.
    fn save(&self, path: &Path, snapshot: &ComputeSnapshot) -> Result<(), Diagnostic>;
}

/// The default store: `glyim.ledger-snapshot/1` documents on the local
/// filesystem (pretty-printed JSON, one trailing newline).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileSnapshotStore;

impl SnapshotStore for FileSnapshotStore {
    fn load(&self, path: &Path) -> SnapshotBatch {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                return SnapshotBatch {
                    snapshot: None,
                    diagnostics: vec![Diagnostic {
                        code: CODE_SNAPSHOT_IO,
                        message: format!("cannot read snapshot {}: {err}", path.display()),
                        severity: Severity::Error,
                    }],
                };
            }
        };
        match serde_json::from_str::<ComputeSnapshot>(&text) {
            Ok(snapshot) if snapshot.schema == SNAPSHOT_SCHEMA => SnapshotBatch {
                snapshot: Some(snapshot),
                diagnostics: Vec::new(),
            },
            Ok(_) => SnapshotBatch {
                snapshot: None,
                diagnostics: vec![Diagnostic {
                    code: CODE_SNAPSHOT_SCHEMA,
                    message: format!(
                        "snapshot {} carries the wrong schema tag (expected {SNAPSHOT_SCHEMA:?})",
                        path.display(),
                    ),
                    severity: Severity::Error,
                }],
            },
            Err(err) => SnapshotBatch {
                snapshot: None,
                diagnostics: vec![Diagnostic {
                    code: CODE_SNAPSHOT_SCHEMA,
                    message: format!("cannot parse snapshot {}: {err}", path.display()),
                    severity: Severity::Error,
                }],
            },
        }
    }

    fn save(&self, path: &Path, snapshot: &ComputeSnapshot) -> Result<(), Diagnostic> {
        let mut text = serde_json::to_string_pretty(snapshot).map_err(|err| Diagnostic {
            code: CODE_SNAPSHOT_SCHEMA,
            message: format!("cannot serialize snapshot {}: {err}", path.display()),
            severity: Severity::Error,
        })?;
        text.push('\n');
        std::fs::write(path, text).map_err(|err| Diagnostic {
            code: CODE_SNAPSHOT_IO,
            message: format!("cannot write snapshot {}: {err}", path.display()),
            severity: Severity::Error,
        })
    }
}

/// In-memory store: a `PathBuf -> snapshot` map for tests and in-process
/// consumers (and as a fixture source for a future CI history store's
/// tests). `save` runs the same serialization contract as disk.
#[derive(Debug, Default)]
pub struct InMemorySnapshotStore {
    snapshots: std::cell::RefCell<std::collections::BTreeMap<PathBuf, ComputeSnapshot>>,
}

impl InMemorySnapshotStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored snapshots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.snapshots.borrow().len()
    }

    /// Whether the store holds no snapshots.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshots.borrow().is_empty()
    }
}

impl SnapshotStore for InMemorySnapshotStore {
    fn load(&self, path: &Path) -> SnapshotBatch {
        match self.snapshots.borrow().get(path) {
            Some(snapshot) => SnapshotBatch {
                snapshot: Some(snapshot.clone()),
                diagnostics: Vec::new(),
            },
            None => SnapshotBatch {
                snapshot: None,
                diagnostics: vec![Diagnostic {
                    code: CODE_SNAPSHOT_IO,
                    message: format!(
                        "cannot read snapshot {}: not found in the in-memory store",
                        path.display()
                    ),
                    severity: Severity::Error,
                }],
            },
        }
    }

    fn save(&self, path: &Path, snapshot: &ComputeSnapshot) -> Result<(), Diagnostic> {
        // Same serialization contract as disk: a snapshot that cannot
        // serialize must not silently enter the store.
        let _ = serde_json::to_string_pretty(snapshot).map_err(|err| Diagnostic {
            code: CODE_SNAPSHOT_SCHEMA,
            message: format!("cannot serialize snapshot {}: {err}", path.display()),
            severity: Severity::Error,
        })?;
        self.snapshots
            .borrow_mut()
            .insert(path.to_path_buf(), snapshot.clone());
        Ok(())
    }
}

// -- waivers ------------------------------------------------------------------

/// One `[[waiver]]` entry in `ledger/waivers.toml` (constitution C9): a
/// temporary, time-boxed exception to the readiness-regression gate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaiverEntry {
    /// Unique waiver ID (referenced by diff rows and diagnostics).
    pub id: String,
    /// The registry feature ID whose regressions this waiver covers.
    pub feature: String,
    /// Why the exception exists (rendered in reports).
    pub reason: String,
    /// Tracking link (the plan requires an issue link per waiver).
    pub issue: String,
    /// Expiry date in the `YYYY-MM-DD` ISO spelling; expired waivers fail
    /// the gate when they would otherwise cover a regression.
    pub expires: String,
}

/// The parsed `waivers.toml` set plus its load diagnostics (invalid entries
/// are dropped and reported, never fatal on their own — the gate fails
/// through the diff verdicts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaiverSet {
    /// Valid, horizon-checked waivers sorted by ID.
    pub waivers: Vec<WaiverEntry>,
    /// Load diagnostics (sorted, de-duplicated).
    pub diagnostics: Vec<Diagnostic>,
}

impl WaiverSet {
    /// An empty set with no diagnostics (the no-waivers baseline).
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

/// The `waivers.toml` document shape (`[[waiver]]` array).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WaiversDoc {
    waiver: Vec<WaiverEntry>,
}

/// Loads `waivers.toml` from `path`, validates each entry against the C9
/// horizon (`today + WAIVER_MAX_AGE_DAYS`), and returns the valid set with
/// diagnostics. An absent file is an empty set, not an error — waivers are
/// optional by design.
#[must_use]
pub fn load_waivers(path: &Path, today: NaiveDate) -> WaiverSet {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return WaiverSet::empty();
        }
        Err(err) => {
            return WaiverSet {
                diagnostics: vec![Diagnostic {
                    code: CODE_WAIVER_SCHEMA,
                    message: format!("cannot read waivers {}: {err}", path.display()),
                    severity: Severity::Error,
                }],
                waivers: Vec::new(),
            };
        }
    };
    let doc: WaiversDoc = match toml::from_str(&text) {
        Ok(doc) => doc,
        Err(err) => {
            return WaiverSet {
                diagnostics: vec![Diagnostic {
                    code: CODE_WAIVER_SCHEMA,
                    message: format!("cannot parse waivers {}: {err}", path.display()),
                    severity: Severity::Error,
                }],
                waivers: Vec::new(),
            };
        }
    };
    let horizon = today.plus_days(WAIVER_MAX_AGE_DAYS);
    let mut waivers = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in doc.waiver {
        if waivers.iter().any(|w: &WaiverEntry| w.id == entry.id) {
            diagnostics.push(Diagnostic {
                code: CODE_WAIVER_DUP,
                message: format!(
                    "waiver id {:?} in {} is declared twice; ids must be unique",
                    entry.id,
                    path.display()
                ),
                severity: Severity::Error,
            });
            continue;
        }
        let Some(expires) = NaiveDate::parse_iso(&entry.expires) else {
            diagnostics.push(Diagnostic {
                code: CODE_WAIVER_SCHEMA,
                message: format!(
                    "waiver {:?} in {} has expiry {:?}, which is not a YYYY-MM-DD date",
                    entry.id,
                    path.display(),
                    entry.expires
                ),
                severity: Severity::Error,
            });
            continue;
        };
        if expires > horizon {
            diagnostics.push(Diagnostic {
                code: CODE_WAIVER_HORIZON,
                message: format!(
                    "waiver {:?} expires {} beyond the {}-day C9 horizon ({}); \
                     shorten the expiry and re-grant",
                    entry.id,
                    entry.expires,
                    WAIVER_MAX_AGE_DAYS,
                    horizon.to_iso()
                ),
                severity: Severity::Error,
            });
            continue;
        }
        waivers.push(entry);
    }
    waivers.sort_by(|a, b| a.id.cmp(&b.id));
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    WaiverSet {
        waivers,
        diagnostics,
    }
}

// -- engine ---------------------------------------------------------------------

/// Verdict for one feature's readiness movement between snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum LevelVerdict {
    /// The computed level rose (or the feature is new with a level above
    /// L0). Never fails the gate.
    Improved,
    /// The computed level did not move.
    Unchanged,
    /// The computed level dropped (or the feature disappeared from the
    /// registry). Fails the gate unless waived.
    Regressed,
}

/// One feature's level movement between two snapshots (one JSON `diffs`
/// item). `baseline`/`current` are `None` when the feature was absent from
/// that side (new feature, dropped feature).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureLevelDiff {
    /// Registry feature ID.
    pub id: String,
    /// Baseline computed level (`None` = not in the baseline snapshot).
    pub baseline: Option<Level>,
    /// Current computed level (`None` = dropped from the registry).
    pub current: Option<Level>,
    /// The gate verdict for this feature.
    pub verdict: LevelVerdict,
    /// The waiver ID covering this regression (empty when unwaived).
    pub waived_by: Option<String>,
}

/// The machine-consumable diff document (`glyim.ledger-diff/1`): the JSON
/// face of [`LedgerDiffOutcome`]. Diagnostics are deliberately excluded —
/// they are operator-facing prose (the text renderer's job); the verdict
/// travels in `diffs` + `passed` (the same decision as the bench harness's
/// `BenchDiffOutcome`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffDocument {
    /// Schema tag, always [`LEDGER_DIFF_SCHEMA`].
    pub schema: String,
    /// Where the baseline snapshot was loaded from.
    pub baseline_path: String,
    /// Where the current snapshot was loaded from.
    pub current_path: String,
    /// One row per compared feature, sorted by ID.
    pub diffs: Vec<FeatureLevelDiff>,
    /// Whether the gate passed (no error diagnostics, no unwaived
    /// regressions).
    pub passed: bool,
}

impl From<&LedgerDiffOutcome> for DiffDocument {
    fn from(outcome: &LedgerDiffOutcome) -> Self {
        Self {
            schema: outcome.schema.clone(),
            baseline_path: outcome.baseline_path.clone(),
            current_path: outcome.current_path.clone(),
            diffs: outcome.diffs.clone(),
            passed: outcome.passed,
        }
    }
}

/// Result of a full readiness diff: envelope, per-feature rows, load and
/// waiver diagnostics, and the C9 gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerDiffOutcome {
    /// Schema tag, always [`LEDGER_DIFF_SCHEMA`].
    pub schema: String,
    /// Where the baseline snapshot was loaded from.
    pub baseline_path: String,
    /// Where the current snapshot was loaded from.
    pub current_path: String,
    /// One row per compared feature, sorted by ID.
    pub diffs: Vec<FeatureLevelDiff>,
    /// Load, waiver-expiry, and waiver-hygiene diagnostics (sorted,
    /// de-duplicated).
    pub diagnostics: Vec<Diagnostic>,
    /// Whether the gate passed: no error-severity diagnostics **and** no
    /// unwaived regression.
    pub passed: bool,
}

impl LedgerDiffOutcome {
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

    /// The gate: `true` when nothing regressed unwaived and no error
    /// diagnostic was produced (load failures included).
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.passed
    }

    /// IDs of rows that regress **and** are not covered by a waiver — the
    /// gate-failing set a CI run must act on.
    #[must_use]
    pub fn regression_ids(&self) -> Vec<&str> {
        self.diffs
            .iter()
            .filter(|d| d.verdict == LevelVerdict::Regressed && d.waived_by.is_none())
            .map(|d| d.id.as_str())
            .collect()
    }

    /// The machine-consumable JSON document (diagnostics excluded by
    /// contract; see [`DiffDocument`]).
    #[must_use]
    pub fn to_document(&self) -> DiffDocument {
        DiffDocument::from(self)
    }
}

/// Applies the waiver set to one regression row: the latest-expiring
/// unexpired waiver wins; an expired cover produces a C9 error diagnostic
/// instead of a waiver.
fn apply_waiver(
    id: &str,
    waivers: &[WaiverEntry],
    today: NaiveDate,
    baseline: Option<Level>,
    current: Option<Level>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let mut candidates: Vec<&WaiverEntry> = waivers.iter().filter(|w| w.feature == id).collect();
    candidates.sort_by(|a, b| b.expires.cmp(&a.expires).then(b.id.cmp(&a.id)));
    if candidates.is_empty() {
        return None;
    }
    // Candidates are sorted latest-expiry first; the first unexpired one
    // covers the regression.
    if let Some(waiver) = candidates
        .iter()
        .find(|w| NaiveDate::parse_iso(&w.expires).is_some_and(|d| d >= today))
    {
        return Some(waiver.id.clone());
    }
    // Every candidate is expired: a stale waivers.toml must never mask a
    // real regression (C9), so name the best candidate in an error.
    let best = candidates[0];
    let (from, to) = match (baseline, current) {
        (Some(b), Some(c)) => (b.as_str(), c.as_str()),
        (Some(b), None) => (b.as_str(), "absent"),
        _ => ("absent", "absent"),
    };
    diagnostics.push(Diagnostic {
        code: CODE_WAIVER_EXPIRED,
        message: format!(
            "waiver {:?} for {id} expired on {} and no longer covers the \
             regression ({from} -> {to}); renew or remove it",
            best.id, best.expires
        ),
        severity: Severity::Error,
    });
    None
}

/// Pure diff of two readiness snapshots: per-feature level deltas, verdicts,
/// and waiver coverage — no filesystem access, no clock (plan C6). Rows are
/// sorted by feature ID; diagnostics are sorted and de-duplicated.
#[must_use]
pub fn diff_levels(
    baseline: &ComputeSnapshot,
    current: &ComputeSnapshot,
    waivers: &WaiverSet,
    today: NaiveDate,
) -> LedgerDiffOutcome {
    let base_by_id: std::collections::BTreeMap<&str, &SnapshotEntry> = baseline
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let current_by_id: std::collections::BTreeMap<&str, &SnapshotEntry> = current
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();

    let ids: std::collections::BTreeSet<&str> = base_by_id
        .keys()
        .chain(current_by_id.keys())
        .copied()
        .collect();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut diffs = Vec::with_capacity(ids.len());
    let mut covered: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut passed = true;

    // `ids` iterates ascending: rows (and `regression_ids`) come out sorted
    // by feature ID (plan C6).
    for id in &ids {
        let baseline_level = base_by_id.get(id).map(|entry| entry.computed_level);
        let current_level = current_by_id.get(id).map(|entry| entry.computed_level);
        let verdict = match (baseline_level, current_level) {
            (Some(base), Some(cur)) => {
                if cur > base {
                    LevelVerdict::Improved
                } else if cur == base {
                    LevelVerdict::Unchanged
                } else {
                    LevelVerdict::Regressed
                }
            }
            (None, Some(_)) => LevelVerdict::Improved,
            (Some(_), None) => LevelVerdict::Regressed,
            // A feature in neither snapshot cannot reach the union.
            (None, None) => continue,
        };
        let waived_by = if verdict == LevelVerdict::Regressed {
            let waiver = apply_waiver(
                id,
                &waivers.waivers,
                today,
                baseline_level,
                current_level,
                &mut diagnostics,
            );
            if let Some(waiver_id) = &waiver {
                covered.insert(waiver_id.clone());
            } else {
                passed = false;
            }
            waiver
        } else {
            None
        };
        diffs.push(FeatureLevelDiff {
            id: id.to_string(),
            baseline: baseline_level,
            current: current_level,
            verdict,
            waived_by,
        });
    }

    // Waiver hygiene: a waiver that matched no regression is dead weight —
    // report it so the file shrinks back (C9: thresholds only tighten).
    for waiver in &waivers.waivers {
        if !covered.contains(waiver.id.as_str()) {
            diagnostics.push(Diagnostic {
                code: CODE_WAIVER_UNUSED,
                message: format!(
                    "waiver {:?} for feature {} matched no regression in this \
                     diff; remove it from waivers.toml",
                    waiver.id, waiver.feature
                ),
                severity: Severity::Warning,
            });
        }
    }

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();

    LedgerDiffOutcome {
        schema: LEDGER_DIFF_SCHEMA.to_string(),
        baseline_path: String::new(),
        current_path: String::new(),
        diffs,
        diagnostics,
        passed,
    }
}

/// Load both sides through `store`, diff them, and wrap the gate (the wiring
/// counterpart of [`diff_levels`]).
///
/// A failed snapshot load (missing file, wrong schema) fails the gate with
/// [`CODE_SNAPSHOT_IO`] / [`CODE_SNAPSHOT_SCHEMA`] diagnostics — a diff
/// against a snapshot that could not be read must never exit green.
#[must_use]
pub fn execute_diff(
    store: &dyn SnapshotStore,
    waivers: &WaiverSet,
    baseline_path: &Path,
    current_path: &Path,
    today: NaiveDate,
) -> LedgerDiffOutcome {
    let baseline_batch = store.load(baseline_path);
    let current_batch = store.load(current_path);
    let mut diagnostics: Vec<Diagnostic> = baseline_batch
        .diagnostics
        .into_iter()
        .chain(current_batch.diagnostics)
        .collect();
    let (mut outcome, passed) = if let (Some(baseline), Some(current)) =
        (baseline_batch.snapshot, current_batch.snapshot)
    {
        let outcome = diff_levels(&baseline, &current, waivers, today);
        let passed = outcome.passed;
        (outcome, passed)
    } else {
        let outcome = diff_levels(
            &ComputeSnapshot {
                schema: SNAPSHOT_SCHEMA.to_string(),
                entries: Vec::new(),
            },
            &ComputeSnapshot {
                schema: SNAPSHOT_SCHEMA.to_string(),
                entries: Vec::new(),
            },
            waivers,
            today,
        );
        (outcome, false)
    };
    diagnostics.extend(outcome.diagnostics);
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    outcome.diagnostics = diagnostics;
    outcome.baseline_path = baseline_path.display().to_string();
    outcome.current_path = current_path.display().to_string();
    outcome.passed = passed;
    outcome
}

/// Runs the full project-level diff (the `ledger diff` entry point): loads
/// `ledger/waivers.toml` under `root`, takes today's date from the wall
/// clock, and executes the diff through the [`FileSnapshotStore`].
#[must_use]
pub fn run_diff(root: &Path, baseline_path: &Path, current_path: &Path) -> LedgerDiffOutcome {
    let span = tracing::info_span!(
        "ledger_diff",
        root = %root.display(),
        baseline = %baseline_path.display(),
        current = %current_path.display()
    );
    let _enter = span.enter();

    let today = NaiveDate::parse_iso(&today_iso()).unwrap_or_else(|| {
        // The clock cannot produce an unparseable ISO date; the fallback
        // keeps the engine total without a panic arm.
        NaiveDate::new_unchecked(1970, 1, 1)
    });
    let waivers = load_waivers(&root.join("ledger/waivers.toml"), today);
    let outcome = execute_diff(
        &FileSnapshotStore,
        &waivers,
        baseline_path,
        current_path,
        today,
    );
    tracing::debug!(
        features = outcome.diffs.len(),
        diagnostics = outcome.diagnostics.len(),
        passed = outcome.passed,
        "ledger diff finished"
    );
    outcome
}

// -- renderers ---------------------------------------------------------------

/// Renders the text report (stdout form of `ledger diff`): one row per
/// feature, diagnostics sorted, and the gate line.
#[must_use]
pub fn format_diff_text(outcome: &LedgerDiffOutcome) -> String {
    let mut lines = vec![format!("ledger diff ({})", outcome.schema)];
    lines.push(format!("  baseline: {}", outcome.baseline_path));
    lines.push(format!("  current : {}", outcome.current_path));
    for diff in &outcome.diffs {
        let from = diff
            .baseline
            .map_or_else(|| "absent".to_string(), |level| level.as_str().to_string());
        let to = diff
            .current
            .map_or_else(|| "absent".to_string(), |level| level.as_str().to_string());
        let verdict = match diff.verdict {
            LevelVerdict::Improved => "improved",
            LevelVerdict::Unchanged => "unchanged",
            LevelVerdict::Regressed => "REGRESSED",
        };
        let row = if let Some(waiver) = &diff.waived_by {
            format!(
                "  {:<32} {from} -> {to}  {verdict} (waived by {waiver})",
                diff.id
            )
        } else {
            format!("  {:<32} {from} -> {to}  {verdict}", diff.id)
        };
        lines.push(row);
    }
    for diagnostic in &outcome.diagnostics {
        lines.push(format!("  {}: {}", diagnostic.severity, diagnostic));
    }
    lines.push(format!(
        "  gate: {}",
        if outcome.passed { "PASSED" } else { "FAILED" }
    ));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Renders the `glyim.ledger-diff/1` JSON contract (pretty-printed, via
/// [`DiffDocument`]; diagnostics excluded by contract).
#[must_use]
pub fn format_diff_json(outcome: &LedgerDiffOutcome) -> String {
    let mut text = serde_json::to_string_pretty(&outcome.to_document()).unwrap_or_else(|err| {
        // Field sets are fixed and values are plain strings/numbers, so
        // serialization is infallible by construction; this arm keeps the
        // renderer total without introducing a Result into the format_*
        // family contract (same shape as `format_compute_json`).
        format!("{{\"schema\":\"{LEDGER_DIFF_SCHEMA}\",\"error\":\"{err}\"}}")
    });
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_schema::Backend;
    use std::path::PathBuf;

    // -- helpers ------------------------------------------------------------

    /// Builds a snapshot from `(id, tier, target, computed)` triples.
    fn snap(entries: &[(&str, Tier, Level, Level)]) -> ComputeSnapshot {
        ComputeSnapshot {
            schema: SNAPSHOT_SCHEMA.to_string(),
            entries: entries
                .iter()
                .map(|(id, tier, target, computed)| SnapshotEntry {
                    id: (*id).to_string(),
                    tier: *tier,
                    target_level: *target,
                    computed_level: *computed,
                    capped_by: Vec::new(),
                })
                .collect(),
        }
    }

    /// One unexpired waiver (today + 3 days).
    fn waiver(id: &str, feature: &str, today: NaiveDate, offset: i64) -> WaiverEntry {
        WaiverEntry {
            id: id.to_string(),
            feature: feature.to_string(),
            reason: "temporary exception".to_string(),
            issue: "https://example.com/issue/1".to_string(),
            expires: today.plus_days(offset).to_iso(),
        }
    }

    const TODAY: NaiveDate = NaiveDate::new_unchecked(2026, 11, 12);

    // -- date arithmetic ----------------------------------------------------

    #[test]
    fn iso_parse_pins_known_dates() {
        let parsed = NaiveDate::parse_iso("1970-01-01").expect("epoch parses");
        assert_eq!(parsed.to_days(), 0);
        assert_eq!(
            NaiveDate::parse_iso("2026-11-12").map(NaiveDate::to_days),
            Some(TODAY.to_days())
        );
        assert!(NaiveDate::parse_iso("2026-13-01").is_none(), "month 13");
        assert!(NaiveDate::parse_iso("2026-00-10").is_none(), "month 0");
        assert!(NaiveDate::parse_iso("2026-02-30").is_none(), "day 30 feb");
        assert!(
            NaiveDate::parse_iso("2026-1-1").is_none(),
            "not zero-padded"
        );
        assert!(NaiveDate::parse_iso("not-a-date").is_none());
    }

    #[test]
    fn civil_days_round_trip_across_leap_years() {
        for days in [-1i64, 0, 1, 365, 366, 1_000, 20_000, 59_000] {
            let date = NaiveDate::from_days(days);
            assert_eq!(date.to_days(), days, "round trip {days}");
            let text = date.to_iso();
            let reparsed = NaiveDate::parse_iso(&text).expect("own output re-parses");
            assert_eq!(reparsed, date, "round trip {text}");
        }
        // Leap-year pins: 2024-02-29 exists, 2023-02-29 does not.
        assert_eq!(
            NaiveDate::parse_iso("2024-02-29").map(NaiveDate::to_days),
            Some(19_782)
        );
        assert!(NaiveDate::parse_iso("2023-02-29").is_none());
    }

    #[test]
    fn plus_days_crosses_month_and_year() {
        assert_eq!(TODAY.plus_days(19).to_iso(), "2026-12-01");
        assert_eq!(TODAY.plus_days(51).to_iso(), "2027-01-02");
        assert_eq!(
            NaiveDate::new_unchecked(2024, 2, 28).plus_days(1).to_iso(),
            "2024-02-29"
        );
    }

    #[test]
    fn today_iso_has_the_wire_shape() {
        let today = today_iso();
        assert_eq!(today.len(), 10, "{today}");
        assert_eq!(today.as_bytes()[4], b'-');
        assert_eq!(today.as_bytes()[7], b'-');
        assert!(NaiveDate::parse_iso(&today).is_some(), "{today}");
    }

    // -- snapshot wire ------------------------------------------------------

    #[test]
    fn snapshot_from_outcome_is_sorted_and_drops_prose() {
        // `missing` reason prose is deliberately absent from the snapshot:
        // the wire shape carries levels, not explanations (same decision as
        // the `glyim.status/1` JSON).
        let outcome = outcome_with_levels(&[
            ("TOOL-B", Tier::Tooling, Level::L4, Level::L2),
            ("LANG-A", Tier::Core, Level::L5, Level::L3),
        ]);
        let snapshot = ComputeSnapshot::from_outcome(&outcome);
        assert_eq!(snapshot.schema, SNAPSHOT_SCHEMA);
        let ids: Vec<&str> = snapshot.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["LANG-A", "TOOL-B"], "sorted by id");
        let first = &snapshot.entries[0];
        assert_eq!(first.tier, Tier::Core);
        assert_eq!(first.target_level, Level::L5);
        assert_eq!(first.computed_level, Level::L3);
        assert!(first.capped_by.is_empty());
    }

    #[test]
    fn snapshot_json_round_trips_strictly() {
        let text = r#"{
  "schema": "glyim.ledger-snapshot/1",
  "entries": [
    {
      "id": "TOOL-X",
      "tier": "tooling",
      "target_level": "L4",
      "computed_level": "L2",
      "capped_by": ["GAP-0002"]
    }
  ]
}"#;
        let parsed: ComputeSnapshot = serde_json::from_str(text).expect("parses");
        assert_eq!(parsed.entries[0].id, "TOOL-X");
        assert_eq!(parsed.entries[0].capped_by, ["GAP-0002"]);
        let round: ComputeSnapshot =
            serde_json::from_str(&serde_json::to_string_pretty(&parsed).expect("serializes"))
                .expect("round trips");
        assert_eq!(round, parsed);
        // deny_unknown_fields: a typo'd field is a parse diagnostic, not a
        // silently-ignored feature level.
        let extra = text.replace("\"capped_by\": [\"GAP-0002\"]", "\"capped_byx\": []");
        assert!(serde_json::from_str::<ComputeSnapshot>(&extra).is_err());
    }

    // -- snapshot stores ----------------------------------------------------

    #[test]
    fn file_store_round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("baseline.json");
        let store = FileSnapshotStore;
        let snapshot = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        store.save(&path, &snapshot).expect("saves");
        let batch = store.load(&path);
        assert!(batch.diagnostics.is_empty(), "{:?}", batch.diagnostics);
        assert_eq!(batch.snapshot, Some(snapshot));
    }

    #[test]
    fn file_store_missing_file_is_a_diagnostic_not_a_panic() {
        let dir = tempfile::tempdir().expect("tmp");
        let batch = FileSnapshotStore.load(&dir.path().join("absent.json"));
        assert!(batch.snapshot.is_none());
        assert_eq!(batch.diagnostics.len(), 1);
        assert_eq!(batch.diagnostics[0].code, CODE_SNAPSHOT_IO);
        assert!(batch.diagnostics[0].message.contains("absent.json"));
    }

    #[test]
    fn file_store_wrong_schema_is_a_schema_diagnostic() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("bad.json");
        std::fs::write(
            &path,
            "{\"schema\":\"glyim.ledger-compute/1\",\"entries\":[]}",
        )
        .expect("write");
        let batch = FileSnapshotStore.load(&path);
        assert!(batch.snapshot.is_none());
        assert_eq!(batch.diagnostics[0].code, CODE_SNAPSHOT_SCHEMA);
    }

    #[test]
    fn in_memory_store_round_trips_and_reports_emptiness() {
        let store = InMemorySnapshotStore::new();
        assert!(store.is_empty());
        let path = PathBuf::from("mem://baseline");
        store
            .save(
                &path,
                &snap(&[("LANG-A", Tier::Core, Level::L5, Level::L0)]),
            )
            .expect("saves");
        assert_eq!(store.len(), 1);
        let batch = store.load(&path);
        assert!(batch.diagnostics.is_empty());
        assert_eq!(batch.snapshot.map(|s| s.entries.len()), Some(1));
    }

    // -- waivers ------------------------------------------------------------

    #[test]
    fn absent_waivers_file_is_an_empty_set() {
        let dir = tempfile::tempdir().expect("tmp");
        let set = load_waivers(&dir.path().join("ledger/waivers.toml"), TODAY);
        assert!(set.waivers.is_empty());
        assert!(set.diagnostics.is_empty());
    }

    #[test]
    fn waivers_parse_and_validate() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("waivers.toml");
        std::fs::write(
            &path,
            "[[waiver]]\n\
             id = \"W-1\"\n\
             feature = \"LANG-A\"\n\
             reason = \"lowering rework in flight\"\n\
             issue = \"https://example.com/issue/9\"\n\
             expires = \"2026-11-20\"\n\n\
             [[waiver]]\n\
             id = \"W-2\"\n\
             feature = \"TOOL-B\"\n\
             reason = \"waiting on LLVM 23 pin\"\n\
             issue = \"https://example.com/issue/10\"\n\
             expires = \"2026-11-26\"\n",
        )
        .expect("write waivers");
        let set = load_waivers(&path, TODAY);
        assert!(set.diagnostics.is_empty(), "{:?}", set.diagnostics);
        let ids: Vec<&str> = set.waivers.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["W-1", "W-2"], "sorted by id");
    }

    #[test]
    fn waiver_horizon_beyond_14_days_is_an_error() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("waivers.toml");
        // TODAY + 15 days is beyond the C9 horizon; +14 is exactly on it.
        std::fs::write(
            &path,
            "[[waiver]]\n\
             id = \"W-LATE\"\n\
             feature = \"LANG-A\"\n\
             reason = \"too long\"\n\
             issue = \"https://example.com/issue/11\"\n\
             expires = \"2026-11-27\"\n\n\
             [[waiver]]\n\
             id = \"W-EDGE\"\n\
             feature = \"TOOL-B\"\n\
             reason = \"exactly on the horizon\"\n\
             issue = \"https://example.com/issue/12\"\n\
             expires = \"2026-11-26\"\n",
        )
        .expect("write waivers");
        let set = load_waivers(&path, TODAY);
        assert_eq!(set.waivers.len(), 1, "horizon-violating waiver is inert");
        assert_eq!(set.waivers[0].id, "W-EDGE");
        assert_eq!(set.diagnostics.len(), 1);
        assert_eq!(set.diagnostics[0].code, CODE_WAIVER_HORIZON);
        assert_eq!(set.diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn waiver_dup_and_bad_expiry_are_errors() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("waivers.toml");
        std::fs::write(
            &path,
            "[[waiver]]\n\
             id = \"W-1\"\n\
             feature = \"LANG-A\"\n\
             reason = \"r\"\n\
             issue = \"https://example.com/issue/1\"\n\
             expires = \"2026-11-20\"\n\n\
             [[waiver]]\n\
             id = \"W-1\"\n\
             feature = \"LANG-A\"\n\
             reason = \"duplicate id\"\n\
             issue = \"https://example.com/issue/2\"\n\
             expires = \"2026-11-20\"\n\n\
             [[waiver]]\n\
             id = \"W-3\"\n\
             feature = \"LANG-B\"\n\
             reason = \"bad date\"\n\
             issue = \"https://example.com/issue/3\"\n\
             expires = \"20th of never\"\n",
        )
        .expect("write waivers");
        let set = load_waivers(&path, TODAY);
        let codes: Vec<&str> = set.diagnostics.iter().map(|d| d.code).collect();
        assert!(codes.contains(&CODE_WAIVER_DUP), "{codes:?}");
        assert!(codes.contains(&CODE_WAIVER_SCHEMA), "{codes:?}");
        assert_eq!(set.waivers.len(), 1);
    }

    #[test]
    fn waiver_toml_garbage_is_a_schema_error() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("waivers.toml");
        std::fs::write(&path, "not = [\"valid toml\\").expect("write");
        let set = load_waivers(&path, TODAY);
        assert!(set.waivers.is_empty());
        assert_eq!(set.diagnostics.len(), 1);
        assert_eq!(set.diagnostics[0].code, CODE_WAIVER_SCHEMA);
    }

    // -- engine: verdict ladder ---------------------------------------------

    #[test]
    fn level_drops_and_drops_to_absence_regress() {
        let baseline = snap(&[
            ("LANG-A", Tier::Core, Level::L5, Level::L3),
            ("LANG-B", Tier::Core, Level::L5, Level::L2),
        ]);
        let current = snap(&[
            ("LANG-A", Tier::Core, Level::L5, Level::L1),
            // LANG-B dropped from the registry entirely.
        ]);
        let outcome = diff_levels(&baseline, &current, &WaiverSet::empty(), TODAY);
        let rows: Vec<(LevelVerdict, Option<&str>)> = outcome
            .diffs
            .iter()
            .map(|d| (d.verdict, d.waived_by.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                (LevelVerdict::Regressed, None),
                (LevelVerdict::Regressed, None)
            ]
        );
        assert!(!outcome.is_ok());
        assert_eq!(outcome.regression_ids(), ["LANG-A", "LANG-B"]);
    }

    #[test]
    fn improvements_and_unchanged_rows_never_fail() {
        let baseline = snap(&[
            ("LANG-A", Tier::Core, Level::L5, Level::L1),
            ("LANG-B", Tier::Core, Level::L5, Level::L2),
            ("LANG-C", Tier::Core, Level::L5, Level::L2),
        ]);
        let current = snap(&[
            ("LANG-A", Tier::Core, Level::L5, Level::L2), // improved
            ("LANG-B", Tier::Core, Level::L5, Level::L2), // unchanged
            ("LANG-C", Tier::Core, Level::L5, Level::L3), // improved
            ("LANG-NEW", Tier::Core, Level::L4, Level::L1), // new feature, no baseline
        ]);
        let outcome = diff_levels(&baseline, &current, &WaiverSet::empty(), TODAY);
        assert!(
            outcome.is_ok(),
            "improvements never fail: {:?}",
            outcome.diffs
        );
        let verdicts: Vec<LevelVerdict> = outcome.diffs.iter().map(|d| d.verdict).collect();
        assert_eq!(
            verdicts,
            [
                LevelVerdict::Improved,
                LevelVerdict::Unchanged,
                LevelVerdict::Improved,
                LevelVerdict::Improved,
            ]
        );
        assert!(outcome.regression_ids().is_empty());
    }

    #[test]
    fn unchanged_gate_passes_vacuously_on_identical_snapshots() {
        let both = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let outcome = diff_levels(&both, &both, &WaiverSet::empty(), TODAY);
        assert!(outcome.is_ok());
        assert_eq!(outcome.diffs[0].verdict, LevelVerdict::Unchanged);
    }

    // -- engine: waivers ------------------------------------------------------

    #[test]
    fn unexpired_waiver_covers_a_regression() {
        let baseline = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        let current = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let set = WaiverSet {
            waivers: vec![waiver("W-1", "LANG-A", TODAY, 3)],
            diagnostics: Vec::new(),
        };
        let outcome = diff_levels(&baseline, &current, &set, TODAY);
        assert!(outcome.is_ok(), "waived: {:?}", outcome.diffs);
        assert_eq!(outcome.diffs[0].verdict, LevelVerdict::Regressed);
        assert_eq!(outcome.diffs[0].waived_by.as_deref(), Some("W-1"));
        assert!(outcome.regression_ids().is_empty(), "waived ≠ unwaived");
        // Hygiene: the waiver was consumed, so it is not reported unused.
        assert!(
            outcome
                .diagnostics
                .iter()
                .all(|d| d.code != CODE_WAIVER_UNUSED)
        );
    }

    #[test]
    fn expired_waiver_cannot_mask_a_regression() {
        let baseline = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        let current = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let set = WaiverSet {
            waivers: vec![waiver("W-OLD", "LANG-A", TODAY, -1)],
            diagnostics: Vec::new(),
        };
        let outcome = diff_levels(&baseline, &current, &set, TODAY);
        assert!(!outcome.is_ok());
        assert_eq!(outcome.diffs[0].waived_by, None);
        let codes: Vec<&str> = outcome.diagnostics.iter().map(|d| d.code).collect();
        assert!(codes.contains(&CODE_WAIVER_EXPIRED), "{codes:?}");
    }

    #[test]
    fn unconsumed_waivers_are_reported_unused() {
        let both = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let set = WaiverSet {
            waivers: vec![waiver("W-IDLE", "TOOL-B", TODAY, 3)],
            diagnostics: Vec::new(),
        };
        let outcome = diff_levels(&both, &both, &set, TODAY);
        assert!(outcome.is_ok());
        let warnings: Vec<&str> = outcome
            .diagnostics
            .iter()
            .filter(|d| d.code == CODE_WAIVER_UNUSED)
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("W-IDLE"), "{}", warnings[0]);
        assert!(warnings[0].contains("TOOL-B"), "{}", warnings[0]);
    }

    #[test]
    fn latest_expiring_waiver_wins_deterministically() {
        let baseline = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        let current = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let set = WaiverSet {
            waivers: vec![
                waiver("W-EARLY", "LANG-A", TODAY, 1),
                waiver("W-LATE", "LANG-A", TODAY, 5),
            ],
            diagnostics: Vec::new(),
        };
        let outcome = diff_levels(&baseline, &current, &set, TODAY);
        assert_eq!(outcome.diffs[0].waived_by.as_deref(), Some("W-LATE"));
    }

    // -- determinism ----------------------------------------------------------

    #[test]
    fn diff_output_is_sorted_and_byte_identical_on_recompute() {
        let baseline = snap(&[
            ("TOOL-Z", Tier::Tooling, Level::L4, Level::L2),
            ("LANG-A", Tier::Core, Level::L5, Level::L3),
            ("STD-M", Tier::Std, Level::L4, Level::L4),
        ]);
        let current = snap(&[
            ("LANG-A", Tier::Core, Level::L5, Level::L2),
            ("STD-M", Tier::Std, Level::L4, Level::L4),
            ("TOOL-Z", Tier::Tooling, Level::L4, Level::L3),
        ]);
        let set = WaiverSet {
            waivers: vec![waiver("W-1", "LANG-A", TODAY, 3)],
            diagnostics: Vec::new(),
        };
        let first = diff_levels(&baseline, &current, &set, TODAY);
        let second = diff_levels(&baseline, &current, &set, TODAY);
        assert_eq!(first, second, "plan C6: deterministic recompute");
        let ids: Vec<&str> = first.diffs.iter().map(|d| d.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
        assert_eq!(format_diff_text(&first), format_diff_text(&second));
        assert_eq!(format_diff_json(&first), format_diff_json(&second));
    }

    // -- renderers --------------------------------------------------------------

    #[test]
    fn text_renders_the_gate_and_rows() {
        let baseline = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        let current = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let outcome = diff_levels(&baseline, &current, &WaiverSet::empty(), TODAY);
        let text = format_diff_text(&outcome);
        assert!(text.contains("glyim.ledger-diff/1"), "{text}");
        assert!(text.contains("LANG-A"), "{text}");
        assert!(text.contains("REGRESSED"), "{text}");
        assert!(text.contains("gate: FAILED"), "{text}");
    }

    #[test]
    fn text_renders_waived_rows_and_passing_gate() {
        let baseline = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        let current = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let set = WaiverSet {
            waivers: vec![waiver("W-1", "LANG-A", TODAY, 3)],
            diagnostics: Vec::new(),
        };
        let outcome = diff_levels(&baseline, &current, &set, TODAY);
        let text = format_diff_text(&outcome);
        assert!(text.contains("waived by W-1"), "{text}");
        assert!(text.contains("gate: PASSED"), "{text}");
    }

    #[test]
    fn json_round_trips_strictly() {
        let baseline = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        let current = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let set = WaiverSet {
            waivers: vec![waiver("W-1", "LANG-A", TODAY, 3)],
            diagnostics: Vec::new(),
        };
        let outcome = diff_levels(&baseline, &current, &set, TODAY);
        let json = format_diff_json(&outcome);
        assert!(
            json.contains("\"schema\": \"glyim.ledger-diff/1\""),
            "{json}"
        );
        let back: DiffDocument = serde_json::from_str(&json).expect("round trips");
        assert_eq!(back, DiffDocument::from(&outcome));
    }

    // -- wiring -----------------------------------------------------------------

    #[test]
    fn execute_diff_loads_both_sides_through_the_store() {
        let store = InMemorySnapshotStore::new();
        store
            .save(
                &PathBuf::from("base"),
                &snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]),
            )
            .expect("save base");
        store
            .save(
                &PathBuf::from("head"),
                &snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]),
            )
            .expect("save head");
        let outcome = execute_diff(
            &store,
            &WaiverSet::empty(),
            &PathBuf::from("base"),
            &PathBuf::from("head"),
            TODAY,
        );
        assert!(!outcome.is_ok());
        assert_eq!(outcome.baseline_path, "base");
        assert_eq!(outcome.current_path, "head");
        assert_eq!(outcome.diffs[0].verdict, LevelVerdict::Regressed);
    }

    #[test]
    fn execute_diff_load_failure_fails_the_gate() {
        let store = InMemorySnapshotStore::new();
        let outcome = execute_diff(
            &store,
            &WaiverSet::empty(),
            &PathBuf::from("absent-base"),
            &PathBuf::from("absent-head"),
            TODAY,
        );
        assert!(!outcome.is_ok(), "load failure must fail the gate");
        assert!(outcome.diffs.is_empty());
        let codes: Vec<&str> = outcome.diagnostics.iter().map(|d| d.code).collect();
        assert!(codes.contains(&CODE_SNAPSHOT_IO), "{codes:?}");
    }

    // -- run entry point (end-to-end) -------------------------------------------

    /// Seeds a one-feature project that computes to L2 (compile-pass +
    /// compile-fail evidence) under the seed policy.
    fn seed_l2_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("docs/specs")).expect("mkdir specs");
        std::fs::create_dir_all(dir.path().join("ledger/features")).expect("mkdir features");
        std::fs::create_dir_all(dir.path().join("ledger/evidence")).expect("mkdir evidence");
        std::fs::write(dir.path().join("docs/specs/v0.1.0.md"), "# spec fixture\n")
            .expect("write spec");
        std::fs::write(
            dir.path().join("ledger/features/seed.toml"),
            "[[feature]]\n\
             id = \"LANG-A\"\n\
             title = \"seed feature\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n",
        )
        .expect("write feature");
        std::fs::write(
            dir.path().join("ledger/policy.toml"),
            "schema = \"glyim.ledger-policy/1\"\n\n\
             [levels.L1]\n\
             rules = [{ mode = \"compile-pass\", outcome = \"passed\", count = 1 }]\n\n\
             [levels.L2]\n\
             rules = [\n\
             \x20 { mode = \"compile-pass\", outcome = \"passed\", count = 1 },\n\
             \x20 { mode = \"compile-fail\", outcome = \"passed\", count = 1 },\n\
             ]\n\n\
             [gap_caps]\n\
             enabled = true\n",
        )
        .expect("write policy");
        let digest = format!("{:0>64}", "a");
        let digest_b = format!("{:0>64}", "b");
        std::fs::write(
            dir.path().join("ledger/evidence/seed.jsonl"),
            format!(
                "{{\"schema\":\"glyim.evidence/1\",\"name\":\"seed-cp\",\"fixture\":\"tests/fixtures/seed_cp.g\",\"fixture_sha256\":\"{digest}\",\"revision\":\"\",\"mode\":\"compile-pass\",\"outcome\":\"passed\",\"reason\":null,\"backend\":\"gam\",\"opt\":0,\"features\":[\"LANG-A\"],\"declared_backends\":[\"gam\"],\"instructions\":null,\"rss_kb\":null}}\n{{\"schema\":\"glyim.evidence/1\",\"name\":\"seed-cf\",\"fixture\":\"tests/fixtures/seed_cf.g\",\"fixture_sha256\":\"{digest_b}\",\"revision\":\"\",\"mode\":\"compile-fail\",\"outcome\":\"passed\",\"reason\":null,\"backend\":\"gam\",\"opt\":0,\"features\":[\"LANG-A\"],\"declared_backends\":[\"gam\"],\"instructions\":null,\"rss_kb\":null}}\n"
            ),
        )
        .expect("write evidence");
        dir
    }

    #[test]
    fn run_diff_gates_a_real_compute_pair() {
        let dir = seed_l2_project();
        let root = dir.path();

        let baseline = ComputeSnapshot::from_outcome(&crate::run_compute(root, None));
        assert_eq!(baseline.entries[0].computed_level, Level::L2);
        let store = FileSnapshotStore;
        let base_path = root.join("baseline.json");
        let head_path = root.join("head.json");
        store.save(&base_path, &baseline).expect("save baseline");

        // Head: the same registry but the computed level dropped — a
        // readiness regression the gate must catch.
        let head_outcome = crate::run_compute(root, None);
        let mut head = ComputeSnapshot::from_outcome(&head_outcome);
        assert_eq!(head.entries.len(), 1);
        head.entries[0].computed_level = Level::L1;
        store.save(&head_path, &head).expect("save head");

        let outcome = run_diff(root, &base_path, &head_path);
        assert!(!outcome.is_ok());
        assert_eq!(
            outcome.diffs[0].verdict,
            LevelVerdict::Regressed,
            "{:?}",
            outcome.diffs
        );

        // With a waiver covering the drop, the same pair passes.
        std::fs::create_dir_all(root.join("ledger")).expect("mkdir ledger");
        std::fs::write(
            root.join("ledger/waivers.toml"),
            "[[waiver]]\n\
             id = \"W-SEED\"\n\
             feature = \"LANG-A\"\n\
             reason = \"fixture quarantine\"\n\
             issue = \"https://example.com/issue/42\"\n\
             expires = \"2026-11-20\"\n",
        )
        .expect("write waiver");
        let outcome = run_diff(root, &base_path, &head_path);
        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        assert_eq!(outcome.diffs[0].waived_by.as_deref(), Some("W-SEED"));
    }

    #[test]
    fn run_diff_same_snapshot_passes_end_to_end() {
        let dir = seed_l2_project();
        let root = dir.path();
        let snapshot = ComputeSnapshot::from_outcome(&crate::run_compute(root, None));
        let store = FileSnapshotStore;
        let base = root.join("baseline.json");
        let head = root.join("head.json");
        store.save(&base, &snapshot).expect("save base");
        store.save(&head, &snapshot).expect("save head");
        let outcome = run_diff(root, &base, &head);
        assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
        assert!(
            outcome
                .diffs
                .iter()
                .all(|d| d.verdict == LevelVerdict::Unchanged)
        );
    }

    // -- outcome shape ----------------------------------------------------------

    #[test]
    fn every_diff_diagnostic_renders_non_empty() {
        let baseline = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L3)]);
        let current = snap(&[("LANG-A", Tier::Core, Level::L5, Level::L2)]);
        let set = WaiverSet {
            waivers: vec![waiver("W-OLD", "LANG-A", TODAY, -1)],
            diagnostics: Vec::new(),
        };
        let outcome = diff_levels(&baseline, &current, &set, TODAY);
        assert!(!outcome.diagnostics.is_empty());
        for diagnostic in &outcome.diagnostics {
            assert!(!diagnostic.to_string().is_empty());
            assert!(diagnostic.message.contains("LANG-A"));
        }
    }

    /// Builds a `ComputeOutcome` carrying the given per-feature levels
    /// (test-only helper: bypasses the registry/evidence machinery).
    fn outcome_with_levels(levels: &[(&str, Tier, Level, Level)]) -> ComputeOutcome {
        ComputeOutcome {
            entries: levels
                .iter()
                .map(
                    |(id, tier, target, computed)| crate::compute::FeatureLevel {
                        id: (*id).to_string(),
                        tier: *tier,
                        target_level: *target,
                        computed_level: *computed,
                        capped_by: Vec::new(),
                        unmeasured: Vec::new(),
                        missing: Vec::new(),
                    },
                )
                .collect(),
            diagnostics: Vec::new(),
            features: levels.len(),
            records: 0,
        }
    }

    /// Keeps the `Backend` import honest: the evidence seeding above uses
    /// string spellings; the re-export check keeps the type set visible.
    #[test]
    fn evidence_backends_spell_as_wire_values() {
        assert_eq!(Backend::Gam.as_str(), "gam");
        assert_eq!(Backend::Llvm.as_str(), "llvm");
    }
}
