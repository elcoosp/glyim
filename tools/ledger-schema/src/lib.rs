//! Readiness Ledger registry schema (plan M1.1, `docs/roadmaps/glyim-master-plan.md` §3).
//!
//! The ledger's core promise is that *status is a pure function of
//! machine-checked evidence* — which starts with a registry whose shape is
//! strict, deterministic, and self-validating. This crate defines:
//!
//! - the strict TOML entry types (`FeatureEntry`, `GapEntry`) with
//!   `deny_unknown_fields` — a typo in a registry file is a load error,
//!   never a silently ignored key;
//! - [`load_registry`], which walks `ledger/features/**` and
//!   `ledger/gaps/**` and collects schema-level diagnostics;
//! - [`Registry::validate`], the `ledger check` engine: unique IDs,
//!   reference resolution, `requires` cycle detection, and the
//!   strict-xfail witness contract (plan §3.3);
//! - the M1.3 inventory extractors ([`extract_inventory`], module
//!   [`inventory`]): source-derived items that must be claimed or ignored
//!   so the registry cannot drift from the code (plan §3.5);
//! - hand-rolled JSON-Schema export for both entry types so external
//!   editors can validate registry files without adding a dependency.
//!
//! Everything is deterministic: diagnostics are sorted, directory walks are
//! sorted, and no clock or randomness participates in any output.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod gates;
pub mod inventory;
pub mod policy;

pub use gates::{
    CODE_GATES_DUP, CODE_GATES_ENTRY, CODE_GATES_LOAD, CODE_GATES_SCHEMA, GATES_SCHEMA, GateEntry,
    GateEvidence, GateRegistry, load_gates,
};

pub use policy::{
    CANONICAL_BACKENDS, CANONICAL_MODES, CANONICAL_OUTCOMES, CODE_POLICY_LOAD, CODE_POLICY_RULE,
    CODE_POLICY_SCHEMA, EvidenceRule, HardeningPolicy, LevelPolicy, POLICY_SCHEMA, load_policy,
    parse_level,
};

pub use inventory::{
    CODE_INVENTORY_CONFLICT, CODE_INVENTORY_DUP_CLAIM, CODE_INVENTORY_DUP_IGNORE,
    CODE_INVENTORY_PARSE, CODE_INVENTORY_STALE_CLAIM, CODE_INVENTORY_STALE_IGNORE,
    CODE_INVENTORY_UNMAPPED, InventoryExtraction, InventoryExtractor, InventoryIgnore,
    InventoryIgnoreFile, InventoryItem, LoadedIgnore, MIR_SOURCE_DIR, MIR_VARIANT_ENUMS,
    RUNTIME_SOURCE_DIR, extract_inventory, extract_mir_variants, extract_runtime_ffi,
    inventory_diagnostics, load_inventory_ignore, unmapped_item_keys,
};

// ---------------------------------------------------------------------------
// Vocabulary enums
// ---------------------------------------------------------------------------

/// Maturity tier of a feature entry (plan §3.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Tier {
    /// Part of the language core; must reach L5 (plan RG-09).
    #[serde(rename = "core")]
    Core,
    /// Standard-library surface.
    #[serde(rename = "std")]
    Std,
    /// Tooling surface (`glyip`, LSP, formatter).
    #[serde(rename = "tooling")]
    Tooling,
    /// Experimental surface; deliberately excluded from release gates.
    #[serde(rename = "experimental")]
    Experimental,
}

impl Tier {
    /// Canonical registry spelling of this tier.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Core => "core",
            Tier::Std => "std",
            Tier::Tooling => "tooling",
            Tier::Experimental => "experimental",
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Maturity level L0–L5 (plan §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Level {
    /// Declared: registry entry with spec reference and target level.
    #[serde(rename = "L0")]
    L0,
    /// Parses: parse-pass fixture, lossless CST round-trip.
    #[serde(rename = "L1")]
    L1,
    /// Checks: positive and negative typeck/borrowck fixtures.
    #[serde(rename = "L2")]
    L2,
    /// Interpreted: run-pass fixtures on the reference interpreter.
    #[serde(rename = "L3")]
    L3,
    /// Native-verified: LLVM `-O0`/`-O2` + differential + fuzz hours.
    #[serde(rename = "L4")]
    L4,
    /// Hardened: metamorphic + TV + mutation score + budgets + docs.
    #[serde(rename = "L5")]
    L5,
}

impl Level {
    /// Canonical registry spelling (`"L0"`..`"L5"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Level::L0 => "L0",
            Level::L1 => "L1",
            Level::L2 => "L2",
            Level::L3 => "L3",
            Level::L4 => "L4",
            Level::L5 => "L5",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Gap class from the repo's own taxonomy (plan §3.3, formerly `KNOWN_GAPS.md`):
/// **a** deliberate hard error, **b** silent wrong behaviour (worst),
/// **c** deferred with no code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GapClass {
    /// Deliberate hard error with a diagnostic.
    #[serde(rename = "a")]
    A,
    /// Silent wrong behaviour — always first in the work queue.
    #[serde(rename = "b")]
    B,
    /// Deferred, no code path, no diagnostic needed.
    #[serde(rename = "c")]
    C,
}

impl GapClass {
    /// Canonical registry spelling (`"a"`, `"b"`, `"c"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GapClass::A => "a",
            GapClass::B => "b",
            GapClass::C => "c",
        }
    }
}

impl fmt::Display for GapClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Compilation backend a feature must be verified on (plan §3.6 matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Backend {
    /// Reference interpreter (GAM / MIR interpreter).
    #[serde(rename = "gam")]
    Gam,
    /// LLVM native backend (O0/O2 are opt levels, not backends).
    #[serde(rename = "llvm")]
    Llvm,
    /// Cranelift backend (planned).
    #[serde(rename = "cranelift")]
    Cranelift,
    /// Stack-machine bytecode backend (`glyim-codegen` + `glyim-bytecode-vm`);
    /// used for testing and embedded execution. Slated for removal by
    /// ADR-004 — the entry goes when the backend does.
    #[serde(rename = "bytecode")]
    Bytecode,
}

impl Backend {
    /// Canonical registry spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Gam => "gam",
            Backend::Llvm => "llvm",
            Backend::Cranelift => "cranelift",
            Backend::Bytecode => "bytecode",
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Workstream lane that owns a feature (plan §6.0: L, F, T, M, B, Q).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Lane {
    /// Ledger / infra / CI lane.
    #[serde(rename = "L")]
    L,
    /// Frontend lane (span, syntax, frontend, def-map, meta, hir).
    #[serde(rename = "F")]
    F,
    /// Types lane (type, solve, typeck).
    #[serde(rename = "T")]
    T,
    /// Middle lane (mir, lower, borrowck, opt, interp, const-eval).
    #[serde(rename = "M")]
    M,
    /// Backend lane (layout, codegen-llvm, runtime, cli, glyip).
    #[serde(rename = "B")]
    B,
    /// Quality lane (test, fuzz, generators, TV).
    #[serde(rename = "Q")]
    Q,
}

impl Lane {
    /// Canonical registry spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Lane::L => "L",
            Lane::F => "F",
            Lane::T => "T",
            Lane::M => "M",
            Lane::B => "B",
            Lane::Q => "Q",
        }
    }
}

impl fmt::Display for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Witness status of a gap (plan §3.3): gaps without a reproducible witness
/// are allowed only as [`GapStatus::Unwitnessed`] and count as witness debt.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum GapStatus {
    /// A witness fixture reproduces the gap and carries the
    /// `//@ known-gap: GAP-XXXX` header (default).
    #[default]
    #[serde(rename = "witnessed")]
    Witnessed,
    /// No witness exists; the entry is explicit witness debt.
    #[serde(rename = "unwitnessed")]
    Unwitnessed,
}

impl GapStatus {
    /// Canonical registry spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GapStatus::Witnessed => "witnessed",
            GapStatus::Unwitnessed => "unwitnessed",
        }
    }
}

impl fmt::Display for GapStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Feature taxonomy (plan M1.2; the master plan's "Appendix C")
// ---------------------------------------------------------------------------

/// One classification rule of the M1.2 feature taxonomy.
///
/// The master plan (§6, M1.2) delegates its feature taxonomy to an
/// "Appendix C" that was never checked into the plan document — the
/// reference is dead (docs drift found during the M1.2 deep dive). This
/// table *is* that appendix, landed in-repo where `ledger check` can
/// enforce it mechanically: every feature entry must name a known area
/// and carry the ID prefix the area prescribes, so ~300 curated entries
/// cannot silently diverge into ad-hoc vocabularies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaRule {
    /// Dot-separated registry area, e.g. `"lang.control-flow"`.
    pub area: &'static str,
    /// Required `feature.id` prefix including the trailing dash.
    pub id_prefix: &'static str,
}

/// The feature taxonomy: area → required ID prefix.
///
/// Sorted by `area` (enforced by test); six namespaces, one prefix each:
/// `lang.*` (language surface), `compiler.*` (analysis and translation),
/// `codegen.*` (backends), `tooling.*` (driver, build tool, LSP, harness,
/// and the ledger/metrics tooling itself), `std.*` (library modules),
/// `runtime.*` (FFI surface).
pub const TAXONOMY: &[AreaRule] = &[
    AreaRule {
        area: "codegen.bytecode",
        id_prefix: "CODEGEN-",
    },
    AreaRule {
        area: "codegen.llvm",
        id_prefix: "CODEGEN-",
    },
    AreaRule {
        area: "compiler.borrowck",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.frontend",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.incremental",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.interp",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.layout",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.mir-opt",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.monomorphization",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.proc-macro",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.trait-solving",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "compiler.type-inference",
        id_prefix: "COMPILER-",
    },
    AreaRule {
        area: "lang.async",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.builtins",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.closures",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.control-flow",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.errors",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.generics",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.macros",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.panics",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.pattern-matching",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.scopes",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.syntax",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.traits",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.types",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "lang.views",
        id_prefix: "LANG-",
    },
    AreaRule {
        area: "runtime.ffi",
        id_prefix: "RUNTIME-",
    },
    AreaRule {
        area: "std.alloc",
        id_prefix: "STD-",
    },
    AreaRule {
        area: "std.core",
        id_prefix: "STD-",
    },
    AreaRule {
        area: "std.std",
        id_prefix: "STD-",
    },
    AreaRule {
        area: "tooling.bench",
        id_prefix: "TOOL-",
    },
    AreaRule {
        area: "tooling.cli",
        id_prefix: "TOOL-",
    },
    AreaRule {
        area: "tooling.glyip",
        id_prefix: "TOOL-",
    },
    AreaRule {
        area: "tooling.ledger",
        id_prefix: "TOOL-",
    },
    AreaRule {
        area: "tooling.lsp",
        id_prefix: "TOOL-",
    },
    AreaRule {
        area: "tooling.test-harness",
        id_prefix: "TOOL-",
    },
];

/// Look up the taxonomy rule for `area`, if it is a known area.
#[must_use]
pub fn area_rule(area: &str) -> Option<&'static AreaRule> {
    TAXONOMY.iter().find(|rule| rule.area == area)
}

/// Minimum `target_level` a feature of `tier` may promise.
///
/// Encodes release gate RG-09 ("All `core` and `std` features ≥ L4;
/// `core` ≥ L5") at declaration time: a registry entry that promises less
/// than its tier's floor is a registry bug, not a status fact. Tooling and
/// experimental entries have no floor — their honest target may be low.
#[must_use]
pub fn min_target_level(tier: Tier) -> Level {
    match tier {
        Tier::Core => Level::L5,
        Tier::Std => Level::L4,
        Tier::Tooling | Tier::Experimental => Level::L0,
    }
}

// ---------------------------------------------------------------------------
// Entry types
// ---------------------------------------------------------------------------

/// What the witness fixture must do *while* the gap is open (plan §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapExpectation {
    /// Diagnostic code the witness must produce, e.g. `"T0060"`.
    pub diagnostic: String,
}

/// One `[[feature]]` registry entry (plan §3.8).
///
/// The registry declares *what we claim to build* — it deliberately has no
/// status field (plan C1): status is computed from evidence, never typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureEntry {
    /// Stable registry ID, e.g. `"LANG-FOR-VEC-INTOITER"`.
    pub id: String,
    /// Human-readable one-liner.
    pub title: String,
    /// Dot-separated area, e.g. `"lang.control-flow"`.
    pub area: String,
    /// Maturity tier.
    pub tier: Tier,
    /// Spec reference, e.g. `"docs/specs/v0.1.0.md"`.
    pub spec: String,
    /// Level this feature must eventually reach.
    pub target_level: Level,
    /// Backends that must reach `target_level`.
    pub backends: Vec<Backend>,
    /// Feature IDs this feature depends on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Error codes this feature may emit (each needs a negative fixture).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// Source-derived inventory keys this feature claims (plan §3.5),
    /// e.g. `"mir.TerminatorKind.Call"` or `"runtime.ffi.glyim_alloc"`.
    /// Every extracted inventory item must be claimed here or ignored in
    /// `ledger/inventory-ignore.toml`; `ledger check` fails on anything
    /// unmapped, stale, duplicated, or both-claimed-and-ignored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inventory: Vec<String>,
    /// Owning workstream lane.
    pub owner_lane: Lane,
}

/// One `[[gap]]` registry entry (plan §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapEntry {
    /// Stable gap ID, e.g. `"GAP-0007"`.
    pub id: String,
    /// Gap class (`a` diagnosed, `b` silent, `c` deferred).
    pub class: GapClass,
    /// Human-readable one-liner.
    pub title: String,
    /// Feature IDs this gap limits.
    pub affects: Vec<String>,
    /// Level ceiling this gap imposes on every affected feature.
    pub caps: Level,
    /// Witness fixture path, relative to the registry root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness: Option<String>,
    /// What the witness must do while the gap is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<GapExpectation>,
    /// Tag or ref where the gap was first recorded.
    pub opened_in: String,
    /// Milestone targeted for closure, if scheduled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_milestone: Option<String>,
    /// Class-level defences that must land when the gap closes (plan C8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prevention: Vec<String>,
    /// Witness status; defaults to [`GapStatus::Witnessed`].
    #[serde(default)]
    pub status: GapStatus,
}

/// File wrapper for `ledger/features/**/*.toml` files.
///
/// A file holds one or more `[[feature]]` tables; any other top-level key
/// is rejected by `deny_unknown_fields`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureFile {
    /// The feature entries declared by this file.
    #[serde(default)]
    pub feature: Vec<FeatureEntry>,
}

/// File wrapper for `ledger/gaps/**/*.toml` files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapFile {
    /// The gap entries declared by this file.
    #[serde(default)]
    pub gap: Vec<GapEntry>,
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Severity of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Fails `ledger check` (exit status 1).
    Error,
    /// Reported but non-fatal (e.g. witness debt).
    Warning,
}

impl Severity {
    /// Lowercase spelling used in rendered output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Diagnostic code: a load/validation failure found in the registry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    /// Stable machine-readable code, e.g. [`CODE_DUP_FEATURE`].
    pub code: &'static str,
    /// Human-readable, self-locating message (contains file path and ID).
    pub message: String,
    /// Severity.
    pub severity: Severity,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.severity, self.code, self.message)
    }
}

/// Schema/parse failures and unexpected files.
pub const CODE_SCHEMA: &str = "LEDGER-SCHEMA";
/// A registry file declared no entries.
pub const CODE_EMPTY_FILE: &str = "LEDGER-EMPTY-FILE";
/// Duplicate `[[feature]]` ID across the registry.
pub const CODE_DUP_FEATURE: &str = "LEDGER-DUP-FEATURE";
/// Duplicate `[[gap]]` ID across the registry.
pub const CODE_DUP_GAP: &str = "LEDGER-DUP-GAP";
/// `gap.affects` references an unknown feature ID.
pub const CODE_UNKNOWN_AFFECTS: &str = "LEDGER-UNKNOWN-AFFECTS";
/// `feature.requires` references an unknown feature ID.
pub const CODE_UNKNOWN_REQUIRES: &str = "LEDGER-UNKNOWN-REQUIRES";
/// `feature.requires` contains a cycle.
pub const CODE_REQUIRES_CYCLE: &str = "LEDGER-REQUIRES-CYCLE";
/// A gap claims to be witnessed but the witness is absent or unnamed.
pub const CODE_WITNESS_MISSING: &str = "LEDGER-WITNESS-MISSING";
/// A gap is `unwitnessed` yet names a witness.
pub const CODE_WITNESS_CONTRADICTORY: &str = "LEDGER-WITNESS-CONTRADICTORY";
/// A witness fixture lacks the `//@ known-gap:` header.
pub const CODE_WITNESS_HEADER: &str = "LEDGER-WITNESS-HEADER";
/// An unwitnessed gap (witness debt, warning).
pub const CODE_WITNESS_DEBT: &str = "LEDGER-WITNESS-DEBT";
/// A gap affects no feature.
pub const CODE_AFFECTS_EMPTY: &str = "LEDGER-AFFECTS-EMPTY";
/// A feature lists no backends.
pub const CODE_BACKENDS_EMPTY: &str = "LEDGER-BACKENDS-EMPTY";
/// A feature names an area outside [`TAXONOMY`] (plan M1.2).
pub const CODE_AREA_UNKNOWN: &str = "LEDGER-AREA-UNKNOWN";
/// A feature `id` does not carry the prefix its `area` prescribes
/// (plan M1.2).
pub const CODE_ID_AREA_MISMATCH: &str = "LEDGER-ID-AREA-MISMATCH";
/// A feature `target_level` is below the floor its `tier` implies via
/// [`min_target_level`] (release gate RG-09, plan M1.2).
pub const CODE_TIER_TARGET: &str = "LEDGER-TIER-TARGET";
/// A feature `spec` reference does not resolve to a repo-relative file
/// under the registry root (plan M1.2: curated entries point at real
/// documentation; dead pointers are a check failure).
pub const CODE_SPEC_MISSING: &str = "LEDGER-SPEC-MISSING";

/// Marker a witness fixture must carry (plan §3.3).
pub const KNOWN_GAP_MARKER: &str = "//@ known-gap:";

// ---------------------------------------------------------------------------
// Registry loading
// ---------------------------------------------------------------------------

/// A registry entry together with the file that declared it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    /// File the entry was loaded from (workspace-relative as given).
    pub path: PathBuf,
    /// The parsed entry.
    pub entry: T,
}

impl<T> Sourced<T> {
    /// ID of the wrapped entry (both entry types carry an `id` field).
    ///
    /// Exposed because duplicate detection and cycle detection both key on
    /// IDs while diagnostics want the declaring file.
    ///
    /// # Panics
    ///
    /// Never — the function only reads the `id` field through serde
    /// accessors on the two concrete entry types it is instantiated with.
    #[must_use]
    pub fn id(&self) -> &str
    where
        T: HasId,
    {
        self.entry.id()
    }
}

/// Trait unifying the two entry types for ID access.
pub trait HasId {
    /// The entry's stable registry ID.
    fn id(&self) -> &str;
}

impl HasId for FeatureEntry {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for GapEntry {
    fn id(&self) -> &str {
        &self.id
    }
}

/// The loaded registry: everything `ledger/features/**` and
/// `ledger/gaps/**` declare, plus the project root that repo-relative
/// references (`spec` paths, gap `witness` fixtures) resolve against.
#[derive(Debug, Clone)]
pub struct Registry {
    root: PathBuf,
    features: Vec<Sourced<FeatureEntry>>,
    gaps: Vec<Sourced<GapEntry>>,
    files: usize,
}

impl Registry {
    /// Project root the registry belongs to: the anchor for repo-relative
    /// references such as `spec` paths and gap `witness` fixtures.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All feature entries in load (sorted) order.
    #[must_use]
    pub fn features(&self) -> &[Sourced<FeatureEntry>] {
        &self.features
    }

    /// All gap entries in load (sorted) order.
    #[must_use]
    pub fn gaps(&self) -> &[Sourced<GapEntry>] {
        &self.gaps
    }

    /// Number of feature entries.
    #[must_use]
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Number of gap entries.
    #[must_use]
    pub fn gap_count(&self) -> usize {
        self.gaps.len()
    }

    /// Number of registry files walked (including files that failed to
    /// parse and files with no entries).
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files
    }

    /// Look up a feature entry by ID.
    #[must_use]
    pub fn feature_by_id(&self, id: &str) -> Option<&FeatureEntry> {
        self.features.iter().map(|s| &s.entry).find(|f| f.id == id)
    }
}

/// A registry load: the entries that parsed plus every schema-level
/// diagnostic (parse failures do not abort the walk — CI wants the full
/// list in one run).
#[derive(Debug, Clone)]
pub struct LoadedRegistry {
    /// Successfully parsed entries.
    pub registry: Registry,
    /// Schema-level diagnostics from parsing/walking.
    pub diagnostics: Vec<Diagnostic>,
}

/// Load the registry rooted at `root` (the directory that contains
/// `features/` and `gaps/`).
///
/// The loaded [`Registry`] anchors repo-relative references (`spec` paths,
/// gap `witness` fixtures — plan §3.3/§3.8 use repo-relative paths such as
/// `docs/specs/control-flow.md#for-loops`) at `root` itself. Fixture and
/// test callers hand the project-shaped directory directly; production
/// callers use [`load_project_registry`], which anchors at the *project*
/// root and walks `project_root/ledger`.
///
/// Deterministic: directory walks are sorted, diagnostics are sorted by
/// `(code, message)` and de-duplicated. Parse failures do not abort the
/// walk — CI wants the complete diagnostic list in one run.
#[must_use]
pub fn load_registry(root: &Path) -> LoadedRegistry {
    load_registry_anchored(root, root)
}

/// Load the registry of the project at `project_root` (the directory that
/// contains `ledger/`, `docs/`, `crates/`, ...).
///
/// This is the production entry point (`glyim-ledger check <ROOT>` walks
/// `ROOT/ledger`): repo-relative references in registry entries resolve
/// against `project_root`, exactly as the plan's examples spell them
/// (`docs/specs/...`, `crates/glyim-test/tests/gaps/...`).
#[must_use]
pub fn load_project_registry(project_root: &Path) -> LoadedRegistry {
    load_registry_anchored(&project_root.join("ledger"), project_root)
}

/// Walk the ledger directory `ledger_dir`, anchoring repo-relative
/// references at `anchor`.
fn load_registry_anchored(ledger_dir: &Path, anchor: &Path) -> LoadedRegistry {
    let root = ledger_dir;
    let features_dir = root.join("features");
    let gaps_dir = root.join("gaps");
    let mut diagnostics = Vec::new();
    if !features_dir.is_dir() && !gaps_dir.is_dir() {
        diagnostics.push(Diagnostic {
            code: CODE_SCHEMA,
            severity: Severity::Error,
            message: format!(
                "no ledger registry directory at {} (expected features/ and gaps/ subdirectories)",
                root.display()
            ),
        });
        let registry = Registry {
            root: root.to_path_buf(),
            features: Vec::new(),
            gaps: Vec::new(),
            files: 0,
        };
        return LoadedRegistry {
            registry,
            diagnostics,
        };
    }

    let mut feature_files = Vec::new();
    collect_toml_files(&features_dir, &mut feature_files);
    let mut gap_files = Vec::new();
    collect_toml_files(&gaps_dir, &mut gap_files);
    let files = feature_files.len() + gap_files.len();

    let mut features = Vec::new();
    for path in &feature_files {
        let Some(file) = parse_registry_file::<FeatureFile>(path, &mut diagnostics) else {
            continue;
        };
        if file.feature.is_empty() {
            diagnostics.push(empty_file_diagnostic(path));
        }
        features.extend(file.feature.into_iter().map(|entry| Sourced {
            path: path.clone(),
            entry,
        }));
    }

    let mut gaps = Vec::new();
    for path in &gap_files {
        let Some(file) = parse_registry_file::<GapFile>(path, &mut diagnostics) else {
            continue;
        };
        if file.gap.is_empty() {
            diagnostics.push(empty_file_diagnostic(path));
        }
        gaps.extend(file.gap.into_iter().map(|entry| Sourced {
            path: path.clone(),
            entry,
        }));
    }

    let registry = Registry {
        root: anchor.to_path_buf(),
        features,
        gaps,
        files,
    };
    sort_diagnostics(&mut diagnostics);
    LoadedRegistry {
        registry,
        diagnostics,
    }
}

/// Recursively collect every file under `dir` in sorted order so the walk
/// is deterministic on every platform.
fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort_unstable();
    for path in paths {
        if path.is_dir() {
            collect_toml_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Parse one registry file, converting every failure shape (unreadable,
/// non-TOML, schema violation) into a `LEDGER-SCHEMA` diagnostic.
fn parse_registry_file<T: for<'de> Deserialize<'de>>(
    path: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<T> {
    if path.extension().is_none_or(|ext| ext != "toml") {
        diagnostics.push(Diagnostic {
            code: CODE_SCHEMA,
            severity: Severity::Error,
            message: format!(
                "unexpected non-TOML file in the registry: {}",
                path.display()
            ),
        });
        return None;
    }
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            diagnostics.push(Diagnostic {
                code: CODE_SCHEMA,
                severity: Severity::Error,
                message: format!("cannot read registry file {}: {err}", path.display()),
            });
            return None;
        }
    };
    match toml::from_str::<T>(&raw) {
        Ok(file) => Some(file),
        Err(err) => {
            diagnostics.push(Diagnostic {
                code: CODE_SCHEMA,
                severity: Severity::Error,
                message: format!(
                    "registry file {} violates the schema: {err}",
                    path.display()
                ),
            });
            None
        }
    }
}

fn empty_file_diagnostic(path: &Path) -> Diagnostic {
    Diagnostic {
        code: CODE_EMPTY_FILE,
        severity: Severity::Error,
        message: format!("registry file declares no entries: {}", path.display()),
    }
}

/// Sort by `(code, message)` and drop exact duplicates.
fn sort_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
}

impl Registry {
    /// Validate the registry: the semantic half of `ledger check`.
    ///
    /// Checks (all deterministic, all sorted):
    ///
    /// - unique feature and gap IDs;
    /// - every `gap.affects` and `feature.requires` reference resolves;
    /// - the `requires` graph is acyclic;
    /// - the witness contract (plan §3.3): a `witnessed` gap must name an
    ///   existing fixture carrying the `//@ known-gap:` header; an
    ///   `unwitnessed` gap must not name one and is reported as witness
    ///   debt;
    /// - `affects` and `backends` are non-empty.
    #[must_use]
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let mut seen_features: BTreeMap<&str, &Path> = BTreeMap::new();
        for sourced in &self.features {
            let id = sourced.id();
            if let Some(first) = seen_features.get(id) {
                diagnostics.push(Diagnostic {
                    code: CODE_DUP_FEATURE,
                    severity: Severity::Error,
                    message: format!(
                        "duplicate feature id {id} (first declared in {}, redeclared in {})",
                        first.display(),
                        sourced.path.display()
                    ),
                });
            } else {
                seen_features.insert(id, &sourced.path);
            }
        }

        let mut seen_gaps: BTreeMap<&str, &Path> = BTreeMap::new();
        for sourced in &self.gaps {
            let id = sourced.id();
            if let Some(first) = seen_gaps.get(id) {
                diagnostics.push(Diagnostic {
                    code: CODE_DUP_GAP,
                    severity: Severity::Error,
                    message: format!(
                        "duplicate gap id {id} (first declared in {}, redeclared in {})",
                        first.display(),
                        sourced.path.display()
                    ),
                });
            } else {
                seen_gaps.insert(id, &sourced.path);
            }
        }

        let feature_ids: HashSet<&str> = self
            .features
            .iter()
            .map(|sourced| sourced.entry.id.as_str())
            .collect();

        for sourced in &self.gaps {
            let gap = &sourced.entry;
            if gap.affects.is_empty() {
                diagnostics.push(Diagnostic {
                    code: CODE_AFFECTS_EMPTY,
                    severity: Severity::Error,
                    message: format!(
                        "gap {} affects no feature (declared in {})",
                        gap.id,
                        sourced.path.display()
                    ),
                });
            }
            for affected in &gap.affects {
                if !feature_ids.contains(affected.as_str()) {
                    diagnostics.push(Diagnostic {
                        code: CODE_UNKNOWN_AFFECTS,
                        severity: Severity::Error,
                        message: format!(
                            "gap {} affects unknown feature id {affected} (declared in {})",
                            gap.id,
                            sourced.path.display()
                        ),
                    });
                }
            }
            self.check_witness(sourced, &mut diagnostics);
        }

        for sourced in &self.features {
            let feature = &sourced.entry;
            if feature.backends.is_empty() {
                diagnostics.push(Diagnostic {
                    code: CODE_BACKENDS_EMPTY,
                    severity: Severity::Error,
                    message: format!(
                        "feature {} lists no backends (declared in {})",
                        feature.id,
                        sourced.path.display()
                    ),
                });
            }
            self.check_feature_taxonomy(sourced, &mut diagnostics);
            for required in &feature.requires {
                if !feature_ids.contains(required.as_str()) {
                    diagnostics.push(Diagnostic {
                        code: CODE_UNKNOWN_REQUIRES,
                        severity: Severity::Error,
                        message: format!(
                            "feature {} requires unknown feature id {required} (declared in {})",
                            feature.id,
                            sourced.path.display()
                        ),
                    });
                }
            }
        }

        self.detect_requires_cycles(&mut diagnostics);
        sort_diagnostics(&mut diagnostics);
        diagnostics
    }

    /// Enforce the strict-xfail witness contract for one gap entry.
    fn check_witness(&self, sourced: &Sourced<GapEntry>, diagnostics: &mut Vec<Diagnostic>) {
        let gap = &sourced.entry;
        match (gap.status, &gap.witness) {
            (GapStatus::Witnessed, None) => diagnostics.push(Diagnostic {
                code: CODE_WITNESS_MISSING,
                severity: Severity::Error,
                message: format!(
                    "gap {} declares itself witnessed but names no witness fixture",
                    gap.id
                ),
            }),
            (GapStatus::Witnessed, Some(witness)) => {
                self.check_witness_fixture(gap, witness, diagnostics);
            }
            (GapStatus::Unwitnessed, Some(_)) => diagnostics.push(Diagnostic {
                code: CODE_WITNESS_CONTRADICTORY,
                severity: Severity::Error,
                message: format!(
                    "gap {} is unwitnessed but names a witness fixture; drop the path or set status = \"witnessed\"",
                    gap.id
                ),
            }),
            (GapStatus::Unwitnessed, None) => diagnostics.push(Diagnostic {
                code: CODE_WITNESS_DEBT,
                severity: Severity::Warning,
                message: format!(
                    "gap {} is unwitnessed (witness debt): add a fixture carrying //@ known-gap: {} and flip status",
                    gap.id, gap.id
                ),
            }),
        }
    }

    /// Verify an existing witness fixture: the file must exist and carry a
    /// `//@ known-gap: GAP-XXXX` header naming this gap.
    fn check_witness_fixture(
        &self,
        gap: &GapEntry,
        witness: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let path = self.root.join(witness);
        let Ok(contents) = fs::read_to_string(&path) else {
            diagnostics.push(Diagnostic {
                code: CODE_WITNESS_MISSING,
                severity: Severity::Error,
                message: format!(
                    "gap {} witness fixture not found: {}",
                    gap.id,
                    path.display()
                ),
            });
            return;
        };
        let headers: Vec<&str> = contents
            .lines()
            .filter(|line| line.trim_start().starts_with(KNOWN_GAP_MARKER))
            .collect();
        if headers.is_empty() || headers.iter().all(|line| !line.contains(gap.id.as_str())) {
            diagnostics.push(Diagnostic {
                code: CODE_WITNESS_HEADER,
                severity: Severity::Error,
                message: format!(
                    "gap {} witness fixture {} lacks a //@ known-gap: {} header",
                    gap.id,
                    path.display(),
                    gap.id
                ),
            });
        }
    }

    /// Enforce the M1.2 feature taxonomy on one entry: a known `area`
    /// ([`TAXONOMY`]), an `id` carrying the area's prescribed prefix, a
    /// `target_level` at or above the `tier`'s floor ([`min_target_level`],
    /// release gate RG-09), and a `spec` reference that resolves to a
    /// repo-relative file under the registry root.
    fn check_feature_taxonomy(
        &self,
        sourced: &Sourced<FeatureEntry>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let feature = &sourced.entry;
        match area_rule(&feature.area) {
            None => diagnostics.push(Diagnostic {
                code: CODE_AREA_UNKNOWN,
                severity: Severity::Error,
                message: format!(
                    "feature {} uses unknown area `{}` (declared in {}); known areas: {}",
                    feature.id,
                    feature.area,
                    sourced.path.display(),
                    TAXONOMY
                        .iter()
                        .map(|rule| rule.area)
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            }),
            Some(rule) => {
                if !feature.id.starts_with(rule.id_prefix) {
                    diagnostics.push(Diagnostic {
                        code: CODE_ID_AREA_MISMATCH,
                        severity: Severity::Error,
                        message: format!(
                            "feature {} id prefix does not match area `{}` (expects `{}`; \
                             declared in {})",
                            feature.id,
                            feature.area,
                            rule.id_prefix,
                            sourced.path.display(),
                        ),
                    });
                }
            }
        }
        let floor = min_target_level(feature.tier);
        if feature.target_level < floor {
            diagnostics.push(Diagnostic {
                code: CODE_TIER_TARGET,
                severity: Severity::Error,
                message: format!(
                    "feature {} has tier `{}` which requires target_level >= {floor}, got {} \
                     (declared in {})",
                    feature.id,
                    feature.tier,
                    feature.target_level,
                    sourced.path.display(),
                ),
            });
        }
        let spec = Path::new(&feature.spec);
        if spec.is_absolute() {
            diagnostics.push(Diagnostic {
                code: CODE_SPEC_MISSING,
                severity: Severity::Error,
                message: format!(
                    "feature {} spec path must be relative to the registry root, got `{}` \
                     (declared in {})",
                    feature.id,
                    feature.spec,
                    sourced.path.display(),
                ),
            });
            return;
        }
        let spec_path = self.root.join(spec);
        if !spec_path.is_file() {
            diagnostics.push(Diagnostic {
                code: CODE_SPEC_MISSING,
                severity: Severity::Error,
                message: format!(
                    "feature {} spec path not found: {} (declared in {})",
                    feature.id,
                    spec_path.display(),
                    sourced.path.display(),
                ),
            });
        }
    }

    /// Report `requires` cycles once each, canonicalized by sorted member
    /// set, with a deterministic `A -> B -> A` rendering.
    fn detect_requires_cycles(&self, diagnostics: &mut Vec<Diagnostic>) {
        let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let known: HashSet<&str> = self
            .features
            .iter()
            .map(|sourced| sourced.entry.id.as_str())
            .collect();
        for sourced in &self.features {
            let feature = &sourced.entry;
            let mut deps: Vec<&str> = feature
                .requires
                .iter()
                .map(String::as_str)
                .filter(|dep| known.contains(dep))
                .collect();
            deps.sort_unstable();
            graph.insert(feature.id.as_str(), deps);
        }

        let mut state: BTreeMap<&str, u8> = graph.keys().map(|id| (*id, STATE_FRESH)).collect();
        let mut stack: Vec<&str> = Vec::new();
        let mut reported: HashSet<Vec<&str>> = HashSet::new();

        for id in graph.keys().copied().collect::<Vec<_>>() {
            if state.get(id).copied().unwrap_or(STATE_FRESH) != STATE_FRESH {
                continue;
            }
            visit_requires(
                id,
                &graph,
                &mut state,
                &mut stack,
                &mut reported,
                diagnostics,
            );
        }
    }
}

/// DFS visit states for cycle detection.
const STATE_FRESH: u8 = 0;
/// Node currently on the DFS stack (its subtree is being explored).
const STATE_IN_STACK: u8 = 1;
/// Node fully explored; revisits can never close a new cycle.
const STATE_DONE: u8 = 2;

/// Recursive DFS over the `requires` graph, reporting each cycle exactly
/// once (canonicalized by sorted member set).
fn visit_requires<'a>(
    id: &'a str,
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    state: &mut BTreeMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
    reported: &mut HashSet<Vec<&'a str>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    state.insert(id, STATE_IN_STACK);
    stack.push(id);
    if let Some(deps) = graph.get(id) {
        for dep in deps.clone() {
            match state.get(dep).copied().unwrap_or(STATE_FRESH) {
                STATE_IN_STACK => {
                    let start = stack.iter().position(|x| *x == dep).unwrap_or(0);
                    let cycle: Vec<&str> = stack[start..].to_vec();
                    let mut members = cycle.clone();
                    members.sort_unstable();
                    if reported.insert(members) {
                        let mut path = cycle.join(" -> ");
                        path.push_str(" -> ");
                        path.push_str(dep);
                        diagnostics.push(Diagnostic {
                            code: CODE_REQUIRES_CYCLE,
                            severity: Severity::Error,
                            message: format!("feature requires cycle: {path}"),
                        });
                    }
                }
                STATE_FRESH => {
                    visit_requires(dep, graph, state, stack, reported, diagnostics);
                }
                _ => {}
            }
        }
    }
    state.insert(id, STATE_DONE);
    stack.pop();
}

// ---------------------------------------------------------------------------
// JSON-Schema export
// ---------------------------------------------------------------------------

/// JSON-Schema (draft-07) for a single `[[feature]]` entry.
///
/// Hand-rolled on purpose: exporting the schema must not add a
/// `schemars`-shaped dependency to the supply chain, and the schema is
/// pinned by golden tests against the struct's field list.
#[must_use]
pub fn feature_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "ledger feature entry",
        "type": "object",
        "additionalProperties": false,
        "required": [
            "id", "title", "area", "tier", "spec", "target_level",
            "backends", "owner_lane"
        ],
        "properties": {
            "id": { "type": "string" },
            "title": { "type": "string" },
            "area": { "type": "string" },
            "tier": { "enum": ["core", "std", "tooling", "experimental"] },
            "spec": { "type": "string" },
            "target_level": {
                "enum": ["L0", "L1", "L2", "L3", "L4", "L5"]
            },
            "backends": {
                "type": "array",
                "minItems": 1,
                "items": { "enum": ["gam", "llvm", "cranelift", "bytecode"] }
            },
            "requires": { "type": "array", "items": { "type": "string" } },
            "errors": { "type": "array", "items": { "type": "string" } },
            "inventory": { "type": "array", "items": { "type": "string" } },
            "owner_lane": { "enum": ["L", "F", "T", "M", "B", "Q"] }
        }
    })
}

/// JSON-Schema (draft-07) for a single `[[gap]]` entry.
#[must_use]
pub fn gap_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "ledger gap entry",
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "class", "title", "affects", "caps", "opened_in"],
        "properties": {
            "id": { "type": "string" },
            "class": { "enum": ["a", "b", "c"] },
            "title": { "type": "string" },
            "affects": {
                "type": "array",
                "minItems": 1,
                "items": { "type": "string" }
            },
            "caps": { "enum": ["L0", "L1", "L2", "L3", "L4", "L5"] },
            "witness": { "type": "string" },
            "expect": {
                "type": "object",
                "additionalProperties": false,
                "required": ["diagnostic"],
                "properties": { "diagnostic": { "type": "string" } }
            },
            "opened_in": { "type": "string" },
            "target_milestone": { "type": "string" },
            "prevention": { "type": "array", "items": { "type": "string" } },
            "status": { "enum": ["witnessed", "unwitnessed"] }
        }
    })
}

/// Combined export document: `{"$schema": …, "feature": …, "gap": …}`.
#[must_use]
pub fn registry_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "glyim readiness ledger registry",
        "feature": feature_schema(),
        "gap": gap_schema()
    })
}

// ---------------------------------------------------------------------------
// Tests (written first — RED phase)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    // -- helpers -----------------------------------------------------------

    fn temp_ledger() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        // M1.2 spec-path validation: every fixture registry resolves feature
        // `spec` references, so the canonical spec file must exist on disk.
        write_file(dir.path(), SPEC_TEST_FILE, "# spec fixture\n");
        dir
    }

    fn write_file(root: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, contents).expect("write");
        path
    }

    /// Spec path every fixture feature resolves against (written by
    /// [`temp_ledger`]).
    const SPEC_TEST_FILE: &str = "docs/specs/v0.1.0.md";

    fn feature_toml(id: &str) -> String {
        format!(
            "[[feature]]\n\
             id = \"{id}\"\n\
             title = \"test feature {id}\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n"
        )
    }

    fn gap_toml(id: &str, affects: &str) -> String {
        format!(
            "[[gap]]\n\
             id = \"{id}\"\n\
             class = \"a\"\n\
             title = \"test gap {id}\"\n\
             affects = [\"{affects}\"]\n\
             caps = \"L2\"\n\
             opened_in = \"v0.3.0\"\n\
             status = \"unwitnessed\"\n"
        )
    }

    fn load_ok(root: &Path) -> LoadedRegistry {
        let loaded = load_registry(root);
        for d in &loaded.diagnostics {
            assert_eq!(
                d.severity,
                Severity::Warning,
                "unexpected error diagnostic in fixture registry: {d}"
            );
        }
        loaded
    }

    fn errors(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    fn warnings(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect()
    }

    fn codes(diags: &[&Diagnostic]) -> Vec<&'static str> {
        diags.iter().map(|d| d.code).collect()
    }

    // -- schema parsing ------------------------------------------------------

    #[test]
    fn parses_feature_entry_minimal() {
        let raw = feature_toml("LANG-X");
        let file: FeatureFile = toml::from_str(&raw).expect("parse");
        assert_eq!(file.feature.len(), 1);
        let f = &file.feature[0];
        assert_eq!(f.id, "LANG-X");
        assert_eq!(f.tier, Tier::Core);
        assert_eq!(f.target_level, Level::L5);
        assert_eq!(f.backends, vec![Backend::Gam, Backend::Llvm]);
        assert_eq!(f.owner_lane, Lane::M);
        assert!(f.requires.is_empty(), "requires defaults to empty");
        assert!(f.errors.is_empty(), "errors defaults to empty");
    }

    #[test]
    fn rejects_unknown_field_on_feature_entry() {
        let raw = format!("{}\nunknown_key = true\n", feature_toml("LANG-X"));
        let err = toml::from_str::<FeatureFile>(&raw).expect_err("must reject unknown field");
        assert!(
            err.to_string().contains("unknown field"),
            "error should name the problem: {err}"
        );
    }

    #[test]
    fn rejects_unknown_field_on_file_wrapper() {
        let raw = format!(
            "{}\n[[not_a_feature]]\nid = \"X\"\n",
            feature_toml("LANG-X")
        );
        assert!(
            toml::from_str::<FeatureFile>(&raw).is_err(),
            "wrapper is strict"
        );
    }

    #[test]
    fn rejects_invalid_enum_spellings() {
        let bad_tier = feature_toml("LANG-X").replace("tier = \"core\"", "tier = \"cortex\"");
        assert!(toml::from_str::<FeatureFile>(&bad_tier).is_err(), "tier");
        let bad_level =
            feature_toml("LANG-X").replace("target_level = \"L5\"", "target_level = \"L9\"");
        assert!(toml::from_str::<FeatureFile>(&bad_level).is_err(), "level");
        let bad_backend = feature_toml("LANG-X").replace("\"gam\"", "\"gcc\"");
        assert!(
            toml::from_str::<FeatureFile>(&bad_backend).is_err(),
            "backend"
        );
        let bad_lane = feature_toml("LANG-X").replace("owner_lane = \"M\"", "owner_lane = \"Z\"");
        assert!(toml::from_str::<FeatureFile>(&bad_lane).is_err(), "lane");
    }

    #[test]
    fn feature_toml_round_trips() {
        let raw = feature_toml("LANG-ROUND-TRIP");
        let first: FeatureFile = toml::from_str(&raw).expect("parse");
        let reserialized = toml::to_string(&first).expect("serialize");
        let second: FeatureFile = toml::from_str(&reserialized).expect("reparse");
        assert_eq!(first, second, "parse -> print -> parse must be identity");
    }

    #[test]
    fn parses_gap_entry_with_expectation() {
        let raw = "[[gap]]\n\
                   id = \"GAP-0001\"\n\
                   class = \"a\"\n\
                   title = \".await inside a loop body rejected\"\n\
                   affects = [\"LANG-ASYNC-AWAIT\"]\n\
                   caps = \"L2\"\n\
                   witness = \"crates/glyim-test/tests/gaps/async_await_in_loop.g\"\n\
                   expect = { diagnostic = \"T0060\" }\n\
                   opened_in = \"v0.3.0\"\n\
                   prevention = [\"CO-5:generator-await-loops\"]\n";
        let file: GapFile = toml::from_str(raw).expect("parse");
        let g = &file.gap[0];
        assert_eq!(g.class, GapClass::A);
        assert_eq!(g.caps, Level::L2);
        assert_eq!(
            g.expect.as_ref().map(|e| e.diagnostic.as_str()),
            Some("T0060")
        );
        assert_eq!(
            g.status,
            GapStatus::Witnessed,
            "status defaults to witnessed"
        );
        assert_eq!(g.prevention, vec!["CO-5:generator-await-loops"]);
    }

    #[test]
    fn rejects_unknown_field_on_gap_entry() {
        let raw = format!("{}\nunknown_key = 1\n", gap_toml("GAP-9", "LANG-X"));
        assert!(toml::from_str::<GapFile>(&raw).is_err());
    }

    #[test]
    fn gap_toml_round_trips() {
        let raw = gap_toml("GAP-0002", "LANG-X");
        let first: GapFile = toml::from_str(&raw).expect("parse");
        let reserialized = toml::to_string(&first).expect("serialize");
        let second: GapFile = toml::from_str(&reserialized).expect("reparse");
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_invalid_gap_class_and_status() {
        let bad_class = gap_toml("GAP-9", "LANG-X").replace("class = \"a\"", "class = \"d\"");
        assert!(toml::from_str::<GapFile>(&bad_class).is_err(), "class");
        let bad_status =
            gap_toml("GAP-9", "LANG-X").replace("status = \"unwitnessed\"", "status = \"maybe\"");
        assert!(toml::from_str::<GapFile>(&bad_status).is_err(), "status");
    }

    #[test]
    fn vocabulary_display_matches_registry_spelling() {
        let _ = format!("{} {} {}", Tier::Std, Level::L4, GapClass::B);
        assert_eq!(Tier::Std.as_str(), "std");
        assert_eq!(Level::L4.as_str(), "L4");
        assert_eq!(GapClass::B.as_str(), "b");
        assert_eq!(Backend::Gam.as_str(), "gam");
        assert_eq!(Lane::Q.as_str(), "Q");
        assert_eq!(GapStatus::Unwitnessed.as_str(), "unwitnessed");
        assert_eq!(GapStatus::default(), GapStatus::Witnessed);
    }

    // -- registry loading ----------------------------------------------------

    #[test]
    fn loads_nested_feature_and_gap_files() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(
            root.path(),
            "features/lang/deep/b.toml",
            &feature_toml("LANG-B"),
        );
        write_file(
            root.path(),
            "gaps/async/0001.toml",
            &gap_toml("GAP-0001", "LANG-A"),
        );
        let loaded = load_ok(root.path());
        assert_eq!(loaded.registry.feature_count(), 2);
        assert_eq!(loaded.registry.gap_count(), 1);
        assert_eq!(
            loaded.registry.file_count(),
            3,
            "all three toml files walked"
        );
        assert!(loaded.registry.feature_by_id("LANG-B").is_some());
        let path = &loaded.registry.features()[0].path;
        assert!(
            path.ends_with("features/lang/a.toml"),
            "sorted walk first file: {path:?}"
        );
    }

    #[test]
    fn reports_schema_error_for_unknown_field_file() {
        let root = temp_ledger();
        write_file(
            root.path(),
            "features/lang/broken.toml",
            &format!("{}\nunknown_key = true\n", feature_toml("LANG-A")),
        );
        let loaded = load_registry(root.path());
        let errs = errors(&loaded.diagnostics);
        assert_eq!(
            codes(&errs),
            vec![CODE_SCHEMA],
            "unknown field is a schema error"
        );
        assert!(
            errs[0].message.contains("broken.toml"),
            "message locates the file"
        );
    }

    #[test]
    fn reports_schema_error_for_non_toml_file() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/notes.txt", "scratch");
        let loaded = load_registry(root.path());
        assert_eq!(codes(&errors(&loaded.diagnostics)), vec![CODE_SCHEMA]);
    }

    #[test]
    fn reports_empty_file_error() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/empty.toml", "# nothing here\n");
        let loaded = load_registry(root.path());
        assert_eq!(codes(&errors(&loaded.diagnostics)), vec![CODE_EMPTY_FILE]);
    }

    #[test]
    fn reports_missing_ledger_directory() {
        let root = temp_ledger();
        let loaded = load_registry(root.path());
        let errs = errors(&loaded.diagnostics);
        assert_eq!(errs.len(), 1, "one loud error for a missing registry");
        assert!(
            errs[0].message.contains("ledger"),
            "error names the problem: {}",
            errs[0].message
        );
    }

    // -- validation ----------------------------------------------------------

    #[test]
    fn detects_duplicate_feature_ids() {
        let root = temp_ledger();
        write_file(
            root.path(),
            "features/lang/a.toml",
            &feature_toml("LANG-DUP"),
        );
        write_file(
            root.path(),
            "features/lang/b.toml",
            &feature_toml("LANG-DUP"),
        );
        let loaded = load_ok(root.path());
        let diags = loaded.registry.validate();
        let errs = errors(&diags);
        assert_eq!(codes(&errs), vec![CODE_DUP_FEATURE]);
    }

    #[test]
    fn detects_duplicate_gap_ids() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(root.path(), "gaps/one.toml", &gap_toml("GAP-DUP", "LANG-A"));
        write_file(root.path(), "gaps/two.toml", &gap_toml("GAP-DUP", "LANG-A"));
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_DUP_GAP]
        );
    }

    #[test]
    fn detects_unknown_affects_reference() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(root.path(), "gaps/x.toml", &gap_toml("GAP-1", "LANG-NOPE"));
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_UNKNOWN_AFFECTS]
        );
    }

    #[test]
    fn detects_unknown_requires_reference() {
        let root = temp_ledger();
        let raw = feature_toml("LANG-A").replace(
            "owner_lane = \"M\"",
            "requires = [\"LANG-MISSING\"]\nowner_lane = \"M\"",
        );
        write_file(root.path(), "features/lang/a.toml", &raw);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_UNKNOWN_REQUIRES]
        );
    }

    #[test]
    fn detects_requires_cycle() {
        let root = temp_ledger();
        let a = feature_toml("LANG-A").replace(
            "owner_lane = \"M\"",
            "requires = [\"LANG-B\"]\nowner_lane = \"M\"",
        );
        let b = feature_toml("LANG-B").replace(
            "owner_lane = \"M\"",
            "requires = [\"LANG-A\"]\nowner_lane = \"M\"",
        );
        write_file(root.path(), "features/lang/a.toml", &a);
        write_file(root.path(), "features/lang/b.toml", &b);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_REQUIRES_CYCLE]
        );
    }

    #[test]
    fn detects_self_requires_cycle() {
        let root = temp_ledger();
        let raw = feature_toml("LANG-A").replace(
            "owner_lane = \"M\"",
            "requires = [\"LANG-A\"]\nowner_lane = \"M\"",
        );
        write_file(root.path(), "features/lang/a.toml", &raw);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_REQUIRES_CYCLE]
        );
    }

    #[test]
    fn witnessed_gap_with_missing_witness_file_is_an_error() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        let raw = gap_toml("GAP-1", "LANG-A")
            .replace("status = \"unwitnessed\"", "status = \"witnessed\"")
            .replace(
                "opened_in = \"v0.3.0\"",
                "witness = \"ledger/witnesses/nope.g\"\nopened_in = \"v0.3.0\"",
            );
        write_file(root.path(), "gaps/x.toml", &raw);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_WITNESS_MISSING]
        );
    }

    #[test]
    fn witnessed_gap_without_witness_path_is_an_error() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        let raw = gap_toml("GAP-1", "LANG-A")
            .replace("status = \"unwitnessed\"", "status = \"witnessed\"");
        write_file(root.path(), "gaps/x.toml", &raw);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_WITNESS_MISSING]
        );
    }

    #[test]
    fn witness_without_known_gap_header_is_an_error() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(
            root.path(),
            "witnesses/no_marker.g",
            "// no header here\nprint(1)\n",
        );
        let raw = gap_toml("GAP-1", "LANG-A")
            .replace("status = \"unwitnessed\"", "status = \"witnessed\"")
            .replace(
                "opened_in = \"v0.3.0\"",
                "witness = \"witnesses/no_marker.g\"\nopened_in = \"v0.3.0\"",
            );
        write_file(root.path(), "gaps/x.toml", &raw);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_WITNESS_HEADER]
        );
    }

    #[test]
    fn witnessed_gap_with_valid_witness_passes() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(
            root.path(),
            "witnesses/marked.g",
            "//@ known-gap: GAP-1\nprint(1)\n",
        );
        let raw = gap_toml("GAP-1", "LANG-A")
            .replace("status = \"unwitnessed\"", "status = \"witnessed\"")
            .replace(
                "opened_in = \"v0.3.0\"",
                "witness = \"witnesses/marked.g\"\nopened_in = \"v0.3.0\"",
            );
        write_file(root.path(), "gaps/x.toml", &raw);
        let loaded = load_ok(root.path());
        assert!(
            errors(&loaded.registry.validate()).is_empty(),
            "valid witness must produce no error"
        );
    }

    #[test]
    fn unwitnessed_gap_naming_a_witness_is_contradictory() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(root.path(), "witnesses/marked.g", "//@ known-gap: GAP-1\n");
        let raw = gap_toml("GAP-1", "LANG-A").replace(
            "opened_in = \"v0.3.0\"",
            "witness = \"witnesses/marked.g\"\nopened_in = \"v0.3.0\"",
        );
        write_file(root.path(), "gaps/x.toml", &raw);
        let loaded = load_ok(root.path());
        let diags = loaded.registry.validate();
        let errs = errors(&diags);
        assert_eq!(codes(&errs), vec![CODE_WITNESS_CONTRADICTORY]);
    }

    #[test]
    fn unwitnessed_gap_is_witness_debt_warning() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(root.path(), "gaps/x.toml", &gap_toml("GAP-1", "LANG-A"));
        let loaded = load_ok(root.path());
        let diags = loaded.registry.validate();
        assert!(errors(&diags).is_empty(), "debt is a warning, not an error");
        assert_eq!(codes(&warnings(&diags)), vec![CODE_WITNESS_DEBT]);
    }

    #[test]
    fn empty_affects_and_backends_are_errors() {
        let root = temp_ledger();
        let raw = gap_toml("GAP-1", "LANG-A").replace("affects = [\"LANG-A\"]", "affects = []");
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        write_file(root.path(), "gaps/x.toml", &raw);
        let no_backends =
            feature_toml("LANG-B").replace("backends = [\"gam\", \"llvm\"]", "backends = []");
        write_file(root.path(), "features/lang/b.toml", &no_backends);
        let loaded = load_ok(root.path());
        let mut got = codes(&errors(&loaded.registry.validate()));
        got.sort_unstable();
        assert_eq!(got, vec![CODE_AFFECTS_EMPTY, CODE_BACKENDS_EMPTY]);
    }

    #[test]
    fn validation_is_deterministic() {
        let root = temp_ledger();
        write_file(
            root.path(),
            "features/lang/a.toml",
            &feature_toml("LANG-DUP"),
        );
        write_file(
            root.path(),
            "features/lang/b.toml",
            &feature_toml("LANG-DUP"),
        );
        write_file(root.path(), "features/lang/c.toml", &feature_toml("LANG-C"));
        write_file(root.path(), "gaps/x.toml", &gap_toml("GAP-1", "LANG-A"));
        let first = load_registry(root.path()).registry.validate();
        let second = load_registry(root.path()).registry.validate();
        assert_eq!(first, second, "two runs must produce identical diagnostics");
        let mut sorted = first.clone();
        sorted.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
        assert_eq!(first, sorted, "diagnostics are returned already sorted");
    }

    // -- M1.2: taxonomy, tier policy, spec paths ------------------------------

    #[test]
    fn unknown_area_is_an_error() {
        let root = temp_ledger();
        let raw = feature_toml("LANG-A")
            .replace("area = \"lang.control-flow\"", "area = \"lang.not-real\"");
        write_file(root.path(), "features/lang/a.toml", &raw);
        let loaded = load_ok(root.path());
        let diags = loaded.registry.validate();
        let errs = errors(&diags);
        assert_eq!(codes(&errs), vec![CODE_AREA_UNKNOWN]);
        assert!(
            errs[0].message.contains("lang.control-flow"),
            "message must list a known area as a hint: {}",
            errs[0].message
        );
    }

    #[test]
    fn feature_id_prefix_must_match_area() {
        let root = temp_ledger();
        // `STD-` prefix with a `lang.*` area is a taxonomy violation.
        write_file(root.path(), "features/lang/a.toml", &feature_toml("STD-A"));
        let loaded = load_ok(root.path());
        let diags = loaded.registry.validate();
        let errs = errors(&diags);
        assert_eq!(codes(&errs), vec![CODE_ID_AREA_MISMATCH]);
        assert!(
            errs[0].message.contains("LANG-"),
            "message must name the expected prefix: {}",
            errs[0].message
        );
    }

    #[test]
    fn matching_id_prefix_and_area_pass() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        let std_entry = feature_toml("STD-B")
            .replace("area = \"lang.control-flow\"", "area = \"std.core\"")
            .replace("tier = \"core\"", "tier = \"std\"")
            .replace("target_level = \"L5\"", "target_level = \"L4\"");
        write_file(root.path(), "features/std/b.toml", &std_entry);
        let loaded = load_ok(root.path());
        assert!(
            errors(&loaded.registry.validate()).is_empty(),
            "prefix/area agreement must pass: {:?}",
            errors(&loaded.registry.validate())
        );
    }

    #[test]
    fn core_tier_must_target_l5() {
        let root = temp_ledger();
        let raw = feature_toml("LANG-A").replace("target_level = \"L5\"", "target_level = \"L3\"");
        write_file(root.path(), "features/lang/a.toml", &raw);
        let loaded = load_ok(root.path());
        let diags = loaded.registry.validate();
        let errs = errors(&diags);
        assert_eq!(codes(&errs), vec![CODE_TIER_TARGET]);
        assert!(
            errs[0].message.contains("L5"),
            "message must state the floor: {}",
            errs[0].message
        );
    }

    #[test]
    fn std_tier_must_target_l4_or_higher() {
        let root = temp_ledger();
        let raw = feature_toml("STD-A")
            .replace("area = \"lang.control-flow\"", "area = \"std.core\"")
            .replace("tier = \"core\"", "tier = \"std\"")
            .replace("target_level = \"L5\"", "target_level = \"L2\"");
        write_file(root.path(), "features/std/a.toml", &raw);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_TIER_TARGET]
        );
    }

    #[test]
    fn tooling_and_experimental_tiers_have_no_target_floor() {
        let root = temp_ledger();
        let tool = feature_toml("TOOL-A")
            .replace("area = \"lang.control-flow\"", "area = \"tooling.cli\"")
            .replace("tier = \"core\"", "tier = \"tooling\"")
            .replace("target_level = \"L5\"", "target_level = \"L2\"");
        let exp = feature_toml("LANG-B")
            .replace("area = \"lang.control-flow\"", "area = \"lang.views\"")
            .replace("tier = \"core\"", "tier = \"experimental\"")
            .replace("target_level = \"L5\"", "target_level = \"L2\"");
        write_file(root.path(), "features/tool/a.toml", &tool);
        write_file(root.path(), "features/lang/b.toml", &exp);
        let loaded = load_ok(root.path());
        assert!(
            errors(&loaded.registry.validate()).is_empty(),
            "tooling/experimental tiers may target below L4: {:?}",
            errors(&loaded.registry.validate())
        );
    }

    #[test]
    fn missing_spec_file_is_an_error() {
        // Deliberately no `docs/specs/missing.md` under this registry root.
        let bare = tempfile::tempdir().expect("tempdir");
        let raw = feature_toml("LANG-A").replace(
            "spec = \"docs/specs/v0.1.0.md\"",
            "spec = \"docs/specs/missing.md\"",
        );
        write_file(bare.path(), "features/lang/a.toml", &raw);
        write_file(bare.path(), "docs/specs/v0.1.0.md", "# spec\n");
        let loaded = load_ok(bare.path());
        let diags = loaded.registry.validate();
        let errs = errors(&diags);
        assert_eq!(codes(&errs), vec![CODE_SPEC_MISSING]);
        assert!(
            errs[0].message.contains("docs/specs/missing.md"),
            "message must name the dead path: {}",
            errs[0].message
        );
    }

    #[test]
    fn spec_path_that_is_a_directory_is_an_error() {
        let root = temp_ledger();
        let raw = feature_toml("LANG-A")
            .replace("spec = \"docs/specs/v0.1.0.md\"", "spec = \"docs/specs\"");
        write_file(root.path(), "features/lang/a.toml", &raw);
        let loaded = load_ok(root.path());
        assert_eq!(
            codes(&errors(&loaded.registry.validate())),
            vec![CODE_SPEC_MISSING]
        );
    }

    #[test]
    fn absolute_spec_path_is_an_error() {
        let root = temp_ledger();
        let raw = feature_toml("LANG-A")
            .replace("spec = \"docs/specs/v0.1.0.md\"", "spec = \"/etc/passwd\"");
        write_file(root.path(), "features/lang/a.toml", &raw);
        let loaded = load_ok(root.path());
        let diags = loaded.registry.validate();
        let errs = errors(&diags);
        assert_eq!(codes(&errs), vec![CODE_SPEC_MISSING]);
        assert!(
            errs[0].message.contains("relative"),
            "message must demand repo-relative specs: {}",
            errs[0].message
        );
    }

    #[test]
    fn existing_spec_file_passes() {
        let root = temp_ledger();
        write_file(root.path(), "features/lang/a.toml", &feature_toml("LANG-A"));
        let loaded = load_ok(root.path());
        assert!(errors(&loaded.registry.validate()).is_empty());
    }

    #[test]
    fn taxonomy_is_sorted_and_unique() {
        let areas: Vec<&str> = TAXONOMY.iter().map(|rule| rule.area).collect();
        let mut sorted = areas.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(areas, sorted, "taxonomy areas must be sorted and unique");
        for rule in TAXONOMY {
            assert!(
                rule.id_prefix
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '-'),
                "prefix must be ASCII upper: {}",
                rule.id_prefix
            );
            assert!(rule.id_prefix.ends_with('-'), "prefix ends with dash");
        }
    }

    #[test]
    fn taxonomy_covers_the_seed_areas() {
        let expected = [
            // the six M1.1 seed areas
            "lang.control-flow",
            "lang.pattern-matching",
            "lang.generics",
            "lang.async",
            "lang.views",
            "lang.panics",
            // the M1.2 taxonomy
            "lang.syntax",
            "lang.types",
            "lang.traits",
            "lang.closures",
            "lang.macros",
            "lang.builtins",
            "lang.errors",
            "lang.scopes",
            "compiler.frontend",
            "compiler.borrowck",
            "compiler.type-inference",
            "compiler.trait-solving",
            "compiler.monomorphization",
            "compiler.mir-opt",
            "compiler.layout",
            "compiler.interp",
            "compiler.incremental",
            "compiler.proc-macro",
            "codegen.llvm",
            "codegen.bytecode",
            "tooling.bench",
            "tooling.cli",
            "tooling.glyip",
            "tooling.lsp",
            "tooling.test-harness",
            "tooling.ledger",
            "std.alloc",
            "std.core",
            "std.std",
            "runtime.ffi",
        ];
        for area in expected {
            assert!(
                area_rule(area).is_some(),
                "{area} must be a known taxonomy area"
            );
        }
        assert!(area_rule("lang.not-real").is_none());
    }

    #[test]
    fn taxonomy_prefixes_are_consistent_per_namespace() {
        for rule in TAXONOMY {
            let namespace = rule.area.split('.').next().expect("namespace");
            let want = match namespace {
                "lang" => "LANG-",
                "compiler" => "COMPILER-",
                "codegen" => "CODEGEN-",
                "tooling" => "TOOL-",
                "std" => "STD-",
                "runtime" => "RUNTIME-",
                other => panic!("unexpected taxonomy namespace {other}"),
            };
            assert_eq!(
                rule.id_prefix, want,
                "area {} must use the {want} prefix",
                rule.area
            );
        }
    }

    #[test]
    fn min_target_level_encodes_rg09() {
        assert_eq!(min_target_level(Tier::Core), Level::L5);
        assert_eq!(min_target_level(Tier::Std), Level::L4);
        assert_eq!(min_target_level(Tier::Tooling), Level::L0);
        assert_eq!(min_target_level(Tier::Experimental), Level::L0);
    }

    // -- JSON-Schema export --------------------------------------------------

    #[test]
    fn feature_schema_lists_required_and_enum_values() {
        let schema = feature_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        let required = schema["required"].as_array().expect("required list");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "id",
                "title",
                "area",
                "tier",
                "spec",
                "target_level",
                "backends",
                "owner_lane"
            ],
            "required mirrors the non-default struct fields"
        );
        assert_eq!(
            schema["properties"]["tier"]["enum"],
            serde_json::json!(["core", "std", "tooling", "experimental"])
        );
        assert_eq!(
            schema["properties"]["target_level"]["enum"],
            serde_json::json!(["L0", "L1", "L2", "L3", "L4", "L5"])
        );
        assert_eq!(
            schema["properties"]["backends"]["items"]["enum"],
            serde_json::json!(["gam", "llvm", "cranelift", "bytecode"])
        );
        assert_eq!(
            schema["properties"]["owner_lane"]["enum"],
            serde_json::json!(["L", "F", "T", "M", "B", "Q"])
        );
    }

    #[test]
    fn gap_schema_lists_required_and_enum_values() {
        let schema = gap_schema();
        let required = schema["required"].as_array().expect("required list");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(
            names,
            vec!["id", "class", "title", "affects", "caps", "opened_in"],
            "required mirrors the non-default struct fields"
        );
        assert_eq!(
            schema["properties"]["class"]["enum"],
            serde_json::json!(["a", "b", "c"])
        );
        assert_eq!(
            schema["properties"]["status"]["enum"],
            serde_json::json!(["witnessed", "unwitnessed"])
        );
    }

    #[test]
    fn registry_schema_combines_both_documents() {
        let combined = registry_schema();
        assert_eq!(combined["feature"], feature_schema());
        assert_eq!(combined["gap"], gap_schema());
        assert!(
            combined["$schema"]
                .as_str()
                .expect("draft key")
                .contains("draft-07")
        );
    }

    #[test]
    fn schema_export_is_byte_deterministic() {
        let a = serde_json::to_string(&registry_schema()).expect("serialize");
        let b = serde_json::to_string(&registry_schema()).expect("serialize");
        assert_eq!(a, b, "schema bytes must be identical across runs");
    }

    // -- diagnostics rendering ----------------------------------------------

    #[test]
    fn diagnostic_display_contains_severity_code_and_message() {
        let d = Diagnostic {
            code: CODE_DUP_FEATURE,
            message: "duplicate feature id LANG-X".into(),
            severity: Severity::Error,
        };
        let mut rendered = String::new();
        let _ = write!(rendered, "{d}");
        assert_eq!(
            rendered,
            "error[LEDGER-DUP-FEATURE]: duplicate feature id LANG-X"
        );
    }
}
