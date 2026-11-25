//! M1.3-remainder consumer walkthrough: run the full §3.5 inventory
//! extraction over a synthetic mini-tree that carries one source file per
//! family (MIR variants, runtime FFI, `SyntaxKind`, type forms, CLI flags,
//! stdlib API, error codes, lints), then show the coverage gate flipping
//! from `LEDGER-INVENTORY-UNMAPPED` errors to green as claims and ignore
//! entries land — the contract the plan pins for M1.3 ("adding a MIR
//! variant or CLI flag without registration fails ledger check").
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example inventory_families
//! ```
//!
//! The walkthrough builds its registry and sources in a throwaway
//! directory, so the live `ledger/` tree is never touched. On your own
//! tree, the same faces run through the CLI:
//!
//! ```text
//! cargo run -p glyim-ledger -- check .
//! cargo run -p glyim-ledger -- inventory ignore --item cli.flag.linker \
//!   --reason "driver plumbing flag, not a feature surface"
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use ledger_schema::{
    InventoryIgnore, Sourced, extract_cli_flags, extract_error_codes, extract_inventory,
    extract_lints, extract_mir_variants, extract_runtime_ffi, extract_stdlib_api,
    extract_syntax_kinds, extract_ty_kinds, inventory_diagnostics, load_inventory_ignore,
    load_project_registry,
};

/// A `SynthesisKind` enum standing in for `SyntaxKind` (the tracked enum
/// name is fixed; the walkthrough uses the real tracked names so the
/// extractor keys match the production contract).
const SYNTAX_SOURCE: &str = r"
/// `SyntaxKind`.
pub enum SyntaxKind {
    /// `KwFn`.
    KwFn,
    /// `Ident`.
    Ident,
}
";

const TY_SOURCE: &str = r"
/// `TyKind`.
pub enum TyKind {
    /// `Bool`.
    Bool,
    /// `Error`.
    Error,
}
";

const CLI_SOURCE: &str = r"
#[derive(Parser, Debug)]
pub struct CliArgs {
    /// Verbosity.
    #[arg(long, default_value_t = false)]
    pub verbose: bool,
}
";

const STDLIB_SOURCE: &str = r"//! The synth module.

/// Doubles a value.
fn double(x: i32) -> i32 { x + x }
";

const ERROR_CODES: &str = r#"
[[code]]
category = "Type"
number = 60
title = "await in loop"
explanation = "Await inside a loop body is rejected."
"#;

fn main() {
    let dir = tempfile::tempdir().expect("walkthrough tmp");
    let root = PathBuf::from(dir.path());

    // Family sources in the exact paths the dispatcher walks.
    std::fs::create_dir_all(root.join("crates/glyim-mir/src")).expect("mkdir mir");
    std::fs::create_dir_all(root.join("crates/glyim-runtime/src")).expect("mkdir runtime");
    std::fs::create_dir_all(root.join("crates/glyim-syntax/src")).expect("mkdir syntax");
    std::fs::create_dir_all(root.join("crates/glyim-type/src")).expect("mkdir type");
    std::fs::create_dir_all(root.join("crates/glyim-cli/src")).expect("mkdir cli");
    std::fs::create_dir_all(root.join("crates/glyim-core/src")).expect("mkdir core");
    std::fs::create_dir_all(root.join("crates/glyim-lang-core/lib")).expect("mkdir lib");
    std::fs::create_dir_all(root.join("crates/glyim-diag/errors")).expect("mkdir errors");
    std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
    std::fs::create_dir_all(root.join("ledger/gaps")).expect("mkdir gaps");
    std::fs::write(
        root.join("crates/glyim-mir/src/lib.rs"),
        "pub enum TerminatorKind { Goto }",
    )
    .expect("write mir");
    std::fs::write(
        root.join("crates/glyim-runtime/src/lib.rs"),
        "#[unsafe(no_mangle)]\npub extern \"C\" fn glyim_demo(size: usize) -> usize { size }\n",
    )
    .expect("write ffi");
    std::fs::write(root.join("crates/glyim-syntax/src/lib.rs"), SYNTAX_SOURCE)
        .expect("write syntax");
    std::fs::write(root.join("crates/glyim-type/src/ty.rs"), TY_SOURCE).expect("write ty");
    std::fs::write(root.join("crates/glyim-cli/src/lib.rs"), CLI_SOURCE).expect("write cli");
    std::fs::write(
        root.join("crates/glyim-core/src/lint.rs"),
        "pub enum LintId { UnsafeFnCall }\n",
    )
    .expect("write lint");
    std::fs::write(
        root.join("crates/glyim-lang-core/lib/synth.g"),
        STDLIB_SOURCE,
    )
    .expect("write stdlib");
    std::fs::write(
        root.join("crates/glyim-diag/errors/codes.toml"),
        ERROR_CODES,
    )
    .expect("write codes");

    // Per-family extraction counts, straight through the library face.
    let families: Vec<(&str, usize)> = vec![
        (
            "mir variants",
            extract_mir_variants("pub enum TerminatorKind { Goto }").len(),
        ),
        (
            "runtime ffi",
            extract_runtime_ffi("#[unsafe(no_mangle)]\npub extern \"C\" fn g() {}\n").len(),
        ),
        ("syntax kinds", extract_syntax_kinds(SYNTAX_SOURCE).len()),
        ("ty kinds", extract_ty_kinds(TY_SOURCE).len()),
        ("cli flags", extract_cli_flags(CLI_SOURCE).len()),
        (
            "stdlib api",
            extract_stdlib_api(STDLIB_SOURCE, "core", "synth").len(),
        ),
        ("error codes", extract_error_codes(ERROR_CODES).len()),
        (
            "lints",
            extract_lints("pub enum LintId { UnsafeFnCall }\n").len(),
        ),
    ];
    eprintln!("per-family extraction over the synthetic tree:");
    for (name, count) in &families {
        eprintln!("  {name:>13}: {count} item(s)");
    }

    // The dispatcher view: every family in one pass, each item keyed.
    let extraction = extract_inventory(&root);
    eprintln!(
        "\ndispatcher: {} item(s) across the §3.5 families, sorted by key:",
        extraction.items.len()
    );
    let mut per_extractor: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &extraction.items {
        per_extractor
            .entry(item.extractor.to_string())
            .or_default()
            .push(item.key.clone());
    }
    for (extractor, keys) in &per_extractor {
        eprintln!("  {extractor}: {}", keys.join(", "));
    }

    // Gate: with an empty registry every item is unmapped.
    let loaded = load_project_registry(&root);
    let registry = loaded.registry;
    let ignore = load_inventory_ignore(&root);
    let empty = Vec::new();
    let ignores = if ignore.ignores.is_empty() {
        &empty
    } else {
        &ignore.ignores
    };
    let unmapped = inventory_diagnostics(&extraction.items, ignores, registry.features());
    eprintln!(
        "\ngate with an empty registry: {} LEDGER-INVENTORY-UNMAPPED error(s) (drift is loud)",
        unmapped.len()
    );
    assert_eq!(
        extraction.items.len(),
        unmapped.len(),
        "all unmapped at first"
    );

    // Cover every item: the LANG-SYN claim demonstrates the feature
    // claim face; everything else lands as ignore entries with reasons.
    let claims = ["syntax.SyntaxKind.KwFn"];
    let claimed_feature = Sourced {
        path: PathBuf::from("ledger/features/lang/syntax.toml"),
        entry: ledger_schema::FeatureEntry {
            id: "LANG-SYN-FN-DEF".to_string(),
            title: "fn definitions".to_string(),
            area: "lang.syntax".to_string(),
            tier: ledger_schema::Tier::Core,
            spec: "docs/specs/v0.1.0.md".to_string(),
            target_level: ledger_schema::Level::L4,
            backends: vec![ledger_schema::Backend::Gam, ledger_schema::Backend::Llvm],
            requires: Vec::new(),
            errors: Vec::new(),
            inventory: claims.iter().map(|c| (*c).to_string()).collect(),
            owner_lane: ledger_schema::Lane::F,
        },
    };
    let ignores: Vec<InventoryIgnore> = extraction
        .items
        .iter()
        .filter(|item| !claims.contains(&item.key.as_str()))
        .map(|item| InventoryIgnore {
            item: item.key.clone(),
            reason: "walkthrough exemption (every real ignore carries its audited reason)"
                .to_string(),
        })
        .collect();
    let covered = inventory_diagnostics(&extraction.items, &ignores, &[claimed_feature]);
    eprintln!(
        "gate after claims + ignores: {} diagnostic(s) (green)",
        covered.len()
    );
    assert!(covered.is_empty(), "covered registry passes: {covered:?}");
}
