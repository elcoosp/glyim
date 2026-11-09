//! CLI library for the Readiness Ledger tool (plan M1.1).
//!
//! Three subcommands:
//!
//! - `glyim-ledger check [ROOT]` — load `ROOT/ledger`, run the schema and
//!   semantic validators, cross-check the M1.3 inventory extractors, print
//!   every diagnostic sorted, exit 1 on any error. This is the CI registry
//!   gate.
//! - `glyim-ledger schema [OUT]` — write the JSON-Schema export document
//!   for both entry types to `OUT` (pretty-printed), or to stdout.
//! - `glyim-ledger inventory [ROOT]` — enumerate the source-derived
//!   inventory items (plan §3.5) with their claim/ignore status.
//!
//! Argument handling mirrors `glyim-metrics`: hand-rolled, no `clap`, so
//! the tool's own supply chain stays dependency-light.

use std::fs;
use std::path::{Path, PathBuf};

use ledger_schema::{
    Diagnostic, Severity, extract_inventory, inventory_diagnostics, load_inventory_ignore,
    load_project_registry, registry_schema, unmapped_item_keys,
};

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `check [ROOT]` — registry gate.
    Check {
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
}

/// Usage text returned with argument errors.
#[must_use]
pub fn usage() -> String {
    "usage: glyim-ledger check [ROOT]\n   or: glyim-ledger schema [OUT]\n   or: glyim-ledger inventory [ROOT]\n\
     \x20 ROOT defaults to the current directory; with no subcommand `check`\n\
     \x20 is assumed. `schema` writes to OUT or stdout."
        .into()
}

/// Parse CLI arguments (excluding argv[0]).
///
/// # Errors
///
/// Returns a human-readable message (including the usage text) for
/// unknown subcommands, unknown flags, or extra positional arguments.
pub fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: Iterator<Item = String>,
{
    let items: Vec<String> = args.collect();
    match items.first().map(String::as_str) {
        None => Ok(Command::Check {
            root: PathBuf::from("."),
        }),
        Some("check" | "schema" | "inventory") => {
            let rest = &items[1..];
            if rest.len() > 1 {
                return Err(format!(
                    "unexpected extra argument {}\n{}",
                    rest[1],
                    usage()
                ));
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
        Some("-h" | "--help") => Err(usage()),
        Some(flag) if flag.starts_with('-') => Err(format!("unknown flag {flag}\n{}", usage())),
        Some(subcommand) => Err(format!("unknown subcommand {subcommand}\n{}", usage())),
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

/// Write the combined JSON-Schema export to `out` (pretty-printed).
///
/// # Errors
///
/// Returns a message when the file cannot be written.
pub fn write_schema(out: &Path) -> Result<String, String> {
    let document = registry_schema();
    let text = serde_json::to_string_pretty(&document)
        .map_err(|err| format!("cannot serialize ledger schema: {err}"))?
        + "\n";
    fs::write(out, text).map_err(|err| format!("cannot write {}: {err}", out.display()))?;
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
        assert!(err.contains("unexpected extra argument"), "{err}");
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
    fn unknown_subcommand_is_rejected_with_usage() {
        let err = parse_args(args(&["frobnicate"])).expect_err("reject");
        assert!(err.contains("usage:"), "error must include usage: {err}");
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let err = parse_args(args(&["--wat"])).expect_err("reject");
        assert!(
            err.contains("unknown flag"),
            "message names the flag: {err}"
        );
    }

    #[test]
    fn help_flag_returns_usage() {
        let err = parse_args(args(&["-h"])).expect_err("help exits via usage");
        assert!(err.contains("usage:"));
    }

    #[test]
    fn extra_positional_is_rejected() {
        let err = parse_args(args(&["check", "a", "b"])).expect_err("reject");
        assert!(err.contains("unexpected extra argument"), "{err}");
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
