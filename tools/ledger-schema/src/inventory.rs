//! Inventory extractors (plan M1.3, §3.5) — `ledger inventory`.
//!
//! The registry declares what we claim to build (M1.2); the extractors
//! enumerate, from source, everything that *should* map to a feature.
//! `ledger check` fails when an extracted item is neither claimed by a
//! feature's `inventory` list nor explicitly ignored (with a reason) in
//! `ledger/inventory-ignore.toml`, so adding a MIR variant, a runtime
//! FFI symbol, a `SyntaxKind`, a type form, a stdlib item, a CLI flag,
//! an error code, or a lint without registration breaks CI — drift
//! between the registry
//! and the code becomes impossible to merge silently (plan §3.5: "drift
//! is impossible").
//!
//! Two extractor families land in this slice:
//!
//! - **MIR IR variants** ([`extract_mir_variants`]): every variant of the
//!   backend-parity enums `StatementKind`, `Rvalue`, `Operand`,
//!   `ProjectionElem`, `TerminatorKind` in `crates/glyim-mir/src` becomes
//!   an item keyed `mir.<Enum>.<Variant>` (plan §3.5 row 3). Parsed with
//!   `syn`, not regexes, so attribute/macro noise cannot fake a variant.
//! - **Runtime FFI symbols** ([`extract_runtime_ffi`]): every
//!   `#[no_mangle]` (or edition-2024 `#[unsafe(no_mangle)]`) `pub extern
//!   "C" fn` under `crates/glyim-runtime/src` becomes an item keyed
//!   `runtime.ffi.<symbol>` (plan §3.5 row 7).
//!
//! The M1.3 remainder slice lands the remaining §3.5 families on the
//! same item/ignore framework:
//!
//! - **`SyntaxKind`** ([`extract_syntax_kinds`]): every variant of
//!   `glyim-syntax`'s `SyntaxKind` enum, keyed
//!   `syntax.SyntaxKind.<variant>` (plan §3.5 row 1).
//! - **Type forms** ([`extract_ty_kinds`]): every variant of
//!   `TyKind`/`AdtKind`/`BuiltinTraitDef`, keyed
//!   `ty.<Enum>.<variant>` (plan §3.5 row 2).
//! - **Stdlib API** ([`extract_stdlib_api`]): every top-level item of
//!   `glyim-lang-*/lib/*.g`, parsed with Glyim's own parser and keyed
//!   `stdlib.<crate>.<module>.<item>` (plan §3.5 row 4).
//! - **CLI flags** ([`extract_cli_flags`]): every `long` flag of the
//!   clap `Parser`-derive entry struct, introspected statically through
//!   `syn` and keyed `cli.flag.<long>` (plan §3.5 row 5).
//! - **Error/lint registries** ([`extract_error_codes`],
//!   [`extract_lints`]): the `errors/codes.toml` registry (display
//!   spelling `diag.error_code.E0001`) and the `LintId` enum
//!   (`core.lint.<id>`) (plan §3.5 row 6).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use syn::Token;
use syn::parse_file;
use syn::punctuated::Punctuated;

use crate::{Diagnostic, FeatureEntry, Severity, Sourced};

// ---------------------------------------------------------------------------
// Diagnostic codes
// ---------------------------------------------------------------------------

/// Extracted item with neither a feature claim nor an ignore entry.
pub const CODE_INVENTORY_UNMAPPED: &str = "LEDGER-INVENTORY-UNMAPPED";
/// `ledger/inventory-ignore.toml` entry that matches no extracted item.
pub const CODE_INVENTORY_STALE_IGNORE: &str = "LEDGER-INVENTORY-STALE-IGNORE";
/// Feature `inventory` claim that no extractor produces.
pub const CODE_INVENTORY_STALE_CLAIM: &str = "LEDGER-INVENTORY-STALE-CLAIM";
/// The same item claimed by more than one feature.
pub const CODE_INVENTORY_DUP_CLAIM: &str = "LEDGER-INVENTORY-DUP-CLAIM";
/// The same item ignored more than once.
pub const CODE_INVENTORY_DUP_IGNORE: &str = "LEDGER-INVENTORY-DUP-IGNORE";
/// An item that is both claimed by a feature and ignored.
pub const CODE_INVENTORY_CONFLICT: &str = "LEDGER-INVENTORY-CONFLICT";
/// A source file the extractor could not read or parse.
pub const CODE_INVENTORY_PARSE: &str = "LEDGER-INVENTORY-PARSE";

/// Schema code reused for `inventory-ignore.toml` parse failures.
const SCHEMA_CODE: &str = crate::CODE_SCHEMA;

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// Which extractor produced an [`InventoryItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InventoryExtractor {
    /// MIR IR-variant extractor (`mir.<Enum>.<Variant>` keys).
    MirVariant,
    /// Runtime FFI symbol extractor (`runtime.ffi.<symbol>` keys).
    RuntimeFfi,
    /// `SyntaxKind` extractor (`syntax.SyntaxKind.<Variant>` keys,
    /// plan §3.5 row 1: every grammar production/token kind).
    SyntaxKind,
    /// Type-form extractor (`ty.<Enum>.<Variant>` keys, plan §3.5 row 2:
    /// `TyKind`, `AdtKind`, `BuiltinTraitDef`).
    TyKind,
    /// CLI-flag extractor (`cli.flag.<long>` keys, plan §3.5 row 5:
    /// clap introspection of the `Parser`-derive entry struct).
    CliFlag,
    /// Stdlib-API extractor (`stdlib.<crate>.<module>.<item>` keys,
    /// plan §3.5 row 4: Glyim's own parser on `glyim-lang-*/lib/*.g`).
    StdlibApi,
    /// Error-code registry extractor (`diag.error_code.<CODE>` keys,
    /// plan §3.5 row 6: the `errors/codes.toml` registry).
    ErrorCode,
    /// Lint-registry extractor (`core.lint.<Id>` keys, plan §3.5 row 6:
    /// the `LintId` registry enum).
    Lint,
}

impl InventoryExtractor {
    /// Stable kebab-case spelling used in reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            InventoryExtractor::MirVariant => "mir-variant",
            InventoryExtractor::RuntimeFfi => "runtime-ffi",
            InventoryExtractor::SyntaxKind => "syntax-kind",
            InventoryExtractor::TyKind => "ty-kind",
            InventoryExtractor::CliFlag => "cli-flag",
            InventoryExtractor::StdlibApi => "stdlib-api",
            InventoryExtractor::ErrorCode => "error-code",
            InventoryExtractor::Lint => "lint",
        }
    }
}

impl std::fmt::Display for InventoryExtractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One source-derived item that should map to a ledger feature (plan
/// §3.5). Keys are stable strings; claims and ignores reference them
/// exactly (no globs, so a new variant can never be silently swallowed).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InventoryItem {
    /// Extractor that produced this item.
    pub extractor: InventoryExtractor,
    /// Stable key, e.g. `"mir.TerminatorKind.Call"` or
    /// `"runtime.ffi.glyim_alloc"`.
    pub key: String,
    /// Workspace-relative source file the item was extracted from.
    pub source: String,
}

/// MIR enums the variant extractor tracks (plan §3.5 row 3: the
/// backend-parity IR enums). Extending this list is itself a registry
/// decision — every variant of a listed enum must then be claimed or
/// ignored.
pub const MIR_VARIANT_ENUMS: &[&str] = &[
    "StatementKind",
    "Rvalue",
    "Operand",
    "ProjectionElem",
    "TerminatorKind",
];

/// Workspace-relative directory the MIR extractor walks.
pub const MIR_SOURCE_DIR: &str = "crates/glyim-mir/src";
/// Workspace-relative directory the runtime FFI extractor walks.
pub const RUNTIME_SOURCE_DIR: &str = "crates/glyim-runtime/src";

/// Rust enums the `SyntaxKind` extractor tracks (plan §3.5 row 1).
pub const SYNTAX_KIND_ENUMS: &[&str] = &["SyntaxKind"];
/// Workspace-relative directory the `SyntaxKind` extractor walks.
pub const SYNTAX_SOURCE_DIR: &str = "crates/glyim-syntax/src";

/// Rust enums the type-form extractor tracks (plan §3.5 row 2).
pub const TY_KIND_ENUMS: &[&str] = &["TyKind", "AdtKind", "BuiltinTraitDef"];
/// Workspace-relative directory the type-form extractor walks.
pub const TY_SOURCE_DIR: &str = "crates/glyim-type/src";

/// Rust enums the lint-registry extractor tracks (plan §3.5 row 6).
pub const LINT_ENUMS: &[&str] = &["LintId"];
/// Workspace-relative directory the lint-registry extractor walks.
pub const LINT_SOURCE_DIR: &str = "crates/glyim-core/src";

/// The clap `Parser`-derive entry struct the CLI-flag extractor reads
/// (plan §3.5 row 5). Introspecting the derive statically keeps the
/// extraction deterministic (no subprocess, C6) and identical across
/// machines.
pub const CLI_ARGS_STRUCT: &str = "CliArgs";
/// Workspace-relative directory the CLI-flag extractor walks.
pub const CLI_SOURCE_DIR: &str = "crates/glyim-cli/src";

/// Workspace-relative stdlib directories the stdlib-API extractor
/// parses with Glyim's own parser (plan §3.5 row 4), each paired with
/// the crate segment of the item key (`stdlib.<crate>.<module>.<item>`).
pub const STDLIB_LIB_DIRS: &[(&str, &str)] = &[
    ("crates/glyim-lang-core/lib", "core"),
    ("crates/glyim-lang-alloc/lib", "alloc"),
    ("crates/glyim-lang-std/lib", "std"),
];

/// Workspace-relative error-code registry the error-code extractor
/// reads (plan §3.5 row 6; the CT-5 source of truth, M3.3).
pub const ERROR_CODE_REGISTRY: &str = "crates/glyim-diag/errors/codes.toml";

/// Category letters of the diagnostic display spelling (`E0001`): the
/// `ErrorCategory::as_letter` mapping mirrored from `glyim-diag` (the
/// ledger does not depend on the compiler crate, so the table is
/// duplicated here and pinned by the extractor tests).
const ERROR_CATEGORY_LETTERS: &[(&str, char)] = &[
    ("Lex", 'E'),
    ("Parse", 'P'),
    ("NameResolution", 'N'),
    ("Type", 'T'),
    ("Lifetime", 'L'),
    ("Borrow", 'B'),
    ("Comptime", 'C'),
    ("Io", 'I'),
    ("Internal", 'X'),
];

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Result of a full extraction run: the items plus every read/parse
/// failure (CI wants the complete picture in one pass, mirroring the
/// registry loader's behavior).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InventoryExtraction {
    /// Extracted items, sorted by `(extractor, key)` — deterministic.
    pub items: Vec<InventoryItem>,
    /// Unreadable/unparseable source files (`LEDGER-INVENTORY-PARSE`).
    pub diagnostics: Vec<Diagnostic>,
}

/// Extract inventory items from `project_root` (the directory that
/// contains `crates/`).
///
/// Missing source directories contribute no items (small fixture projects
/// and fixture-only registries stay valid); present-but-unparseable
/// sources report [`CODE_INVENTORY_PARSE`] errors so a broken file can
/// never silently shrink the gate.
#[must_use]
pub fn extract_inventory(project_root: &Path) -> InventoryExtraction {
    let mut items = Vec::new();
    let mut diagnostics = Vec::new();
    walk_extractor(
        &project_root.join(MIR_SOURCE_DIR),
        InventoryExtractor::MirVariant,
        &mut items,
        &mut diagnostics,
    );
    walk_extractor(
        &project_root.join(RUNTIME_SOURCE_DIR),
        InventoryExtractor::RuntimeFfi,
        &mut items,
        &mut diagnostics,
    );
    walk_extractor(
        &project_root.join(SYNTAX_SOURCE_DIR),
        InventoryExtractor::SyntaxKind,
        &mut items,
        &mut diagnostics,
    );
    walk_extractor(
        &project_root.join(TY_SOURCE_DIR),
        InventoryExtractor::TyKind,
        &mut items,
        &mut diagnostics,
    );
    walk_extractor(
        &project_root.join(CLI_SOURCE_DIR),
        InventoryExtractor::CliFlag,
        &mut items,
        &mut diagnostics,
    );
    walk_extractor(
        &project_root.join(LINT_SOURCE_DIR),
        InventoryExtractor::Lint,
        &mut items,
        &mut diagnostics,
    );
    walk_stdlib_api(project_root, &mut items, &mut diagnostics);
    walk_error_codes(project_root, &mut items, &mut diagnostics);
    items.sort();
    items.dedup();
    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    InventoryExtraction { items, diagnostics }
}

/// Walk one extractor's source directory (sorted, recursive) and extract
/// from every `.rs` file. Each file is parsed exactly once: a parse
/// failure reports [`CODE_INVENTORY_PARSE`] for the file and skips it,
/// never silently shrinking the item set.
fn walk_extractor(
    dir: &Path,
    extractor: InventoryExtractor,
    items: &mut Vec<InventoryItem>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<std::path::PathBuf> =
        read.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort_unstable();
    for path in paths {
        if path.is_dir() {
            walk_extractor(&path, extractor, items, diagnostics);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            diagnostics.push(Diagnostic {
                code: CODE_INVENTORY_PARSE,
                severity: Severity::Error,
                message: format!(
                    "cannot read inventory source {}: unreadable",
                    path.display()
                ),
            });
            continue;
        };
        let file = match parse_file(&source) {
            Ok(file) => file,
            Err(err) => {
                diagnostics.push(Diagnostic {
                    code: CODE_INVENTORY_PARSE,
                    severity: Severity::Error,
                    message: format!("cannot parse inventory source {}: {err}", path.display()),
                });
                continue;
            }
        };
        let extracted = match extractor {
            InventoryExtractor::MirVariant => enum_variant_items(
                &file,
                MIR_VARIANT_ENUMS,
                "mir",
                true,
                InventoryExtractor::MirVariant,
            ),
            InventoryExtractor::RuntimeFfi => ffi_items_from_file(&file),
            InventoryExtractor::SyntaxKind => enum_variant_items(
                &file,
                SYNTAX_KIND_ENUMS,
                "syntax",
                true,
                InventoryExtractor::SyntaxKind,
            ),
            InventoryExtractor::TyKind => {
                enum_variant_items(&file, TY_KIND_ENUMS, "ty", true, InventoryExtractor::TyKind)
            }
            InventoryExtractor::CliFlag => cli_items_from_file(&file),
            InventoryExtractor::StdlibApi | InventoryExtractor::ErrorCode => Vec::new(),
            InventoryExtractor::Lint => enum_variant_items(
                &file,
                LINT_ENUMS,
                "core.lint",
                false,
                InventoryExtractor::Lint,
            ),
        };
        let display = path.display().to_string();
        items.extend(extracted.into_iter().map(|mut item| {
            item.source.clone_from(&display);
            item
        }));
    }
}

/// Walk the stdlib `lib/` directories (plan §3.5 row 4) and extract one
/// `stdlib.<crate>.<module>.<item>` key per top-level declaration.
///
/// Each `lib/*.g` file is one module; a file Glyim's parser recovers
/// with diagnostics contributes its parsed items *and* a
/// `LEDGER-INVENTORY-PARSE` diagnostic, so a broken stdlib file can
/// never silently shrink the API gate. Missing directories contribute
/// no items (fixture-only projects stay valid).
fn walk_stdlib_api(
    project_root: &Path,
    items: &mut Vec<InventoryItem>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (dir, crate_short) in STDLIB_LIB_DIRS {
        let Ok(read) = fs::read_dir(project_root.join(dir)) else {
            continue;
        };
        let mut paths: Vec<std::path::PathBuf> =
            read.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort_unstable();
        for path in paths {
            if path.extension().is_none_or(|ext| ext != "g") {
                continue;
            }
            let Ok(source) = fs::read_to_string(&path) else {
                diagnostics.push(Diagnostic {
                    code: CODE_INVENTORY_PARSE,
                    severity: Severity::Error,
                    message: format!("cannot read stdlib source {}: unreadable", path.display()),
                });
                continue;
            };
            let module = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            let extracted = extract_stdlib_api(&source, crate_short, module);
            if stdlib_api_had_parse_diagnostics() {
                diagnostics.push(Diagnostic {
                    code: CODE_INVENTORY_PARSE,
                    severity: Severity::Error,
                    message: format!(
                        "glyim parser recovered diagnostics in stdlib source {}",
                        path.display()
                    ),
                });
            }
            let display = path.display().to_string();
            items.extend(extracted.into_iter().map(|mut item| {
                item.source.clone_from(&display);
                item
            }));
        }
    }
}

/// Read the error-code registry (plan §3.5 row 6) and extract one
/// `diag.error_code.<CODE>` key per registered code. A missing registry
/// contributes no items (fixture-only projects stay valid); a present-
/// but-unreadable one is a `LEDGER-INVENTORY-PARSE` error.
fn walk_error_codes(
    project_root: &Path,
    items: &mut Vec<InventoryItem>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = project_root.join(ERROR_CODE_REGISTRY);
    let Ok(text) = fs::read_to_string(&path) else {
        // A project without the compiler registry simply has no
        // error-code items to gate.
        return;
    };
    let extracted = extract_error_codes(&text);
    if extracted.is_empty() {
        diagnostics.push(Diagnostic {
            code: CODE_INVENTORY_PARSE,
            severity: Severity::Error,
            message: format!(
                "cannot parse error-code registry {}: no codes extracted",
                path.display()
            ),
        });
        return;
    }
    let display = path.display().to_string();
    items.extend(extracted.into_iter().map(|mut item| {
        item.source.clone_from(&display);
        item
    }));
}

/// Enum-variant keys of one parsed file (unsorted; callers sort).
///
/// The shared engine behind the §3.5 enum families (MIR variants,
/// `SyntaxKind`, `TyKind` forms, lints): every variant of a *top-level*
/// `pub enum` whose name is in `tracked` becomes one item, keyed
/// `{prefix}.{Enum}.{Variant}` (or `{prefix}.{Variant}` when the enum
/// name is already the key context, as for the single lint registry
/// enum). Unit, tuple, and struct variants all extract the same way —
/// every variant must be claimed or ignored downstream regardless of
/// shape.
fn enum_variant_items(
    file: &syn::File,
    tracked: &[&str],
    prefix: &str,
    include_enum_name: bool,
    extractor: InventoryExtractor,
) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    for entry in &file.items {
        let syn::Item::Enum(item_enum) = entry else {
            continue;
        };
        if !matches!(item_enum.vis, syn::Visibility::Public(_)) {
            continue;
        }
        let name = item_enum.ident.to_string();
        if !tracked.contains(&name.as_str()) {
            continue;
        }
        for variant in &item_enum.variants {
            let key = if include_enum_name {
                format!("{prefix}.{name}.{}", variant.ident)
            } else {
                format!("{prefix}.{}", variant.ident)
            };
            items.push(InventoryItem {
                extractor,
                key,
                source: String::new(),
            });
        }
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    items
}

/// Runtime FFI symbol keys of one parsed file (unsorted; callers sort).
fn ffi_items_from_file(file: &syn::File) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    for entry in &file.items {
        let syn::Item::Fn(item_fn) = entry else {
            continue;
        };
        if !matches!(item_fn.vis, syn::Visibility::Public(_)) {
            continue;
        }
        if !item_fn.attrs.iter().any(attr_is_no_mangle) {
            continue;
        }
        if !abi_is_c(item_fn.sig.abi.as_ref()) {
            continue;
        }
        items.push(InventoryItem {
            extractor: InventoryExtractor::RuntimeFfi,
            key: format!("runtime.ffi.{}", item_fn.sig.ident),
            source: String::new(),
        });
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    items
}

/// True for `#[no_mangle]` and its edition-2024 spelling
/// `#[unsafe(no_mangle)]`.
fn attr_is_no_mangle(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("no_mangle") {
        return true;
    }
    if !attr.path().is_ident("unsafe") {
        return false;
    }
    let Ok(nested) = attr.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
    else {
        return false;
    };
    nested.iter().any(|meta| meta.path().is_ident("no_mangle"))
}

/// True for `extern "C"` and `extern "C-unwind"` ABI declarations.
fn abi_is_c(abi: Option<&syn::Abi>) -> bool {
    abi.and_then(|a| a.name.as_ref())
        .is_some_and(|name| matches!(name.value().as_str(), "C" | "C-unwind"))
}

/// Extract `mir.<Enum>.<Variant>` items from one Rust source file.
///
/// Only top-level `pub enum` items whose name is in
/// [`MIR_VARIANT_ENUMS`] count; every variant of a tracked enum must be
/// claimed or ignored downstream, regardless of shape (unit, tuple, or
/// struct variants all extract the same way). Output is sorted by key so
/// per-file extraction is already deterministic.
#[must_use]
pub fn extract_mir_variants(source: &str) -> Vec<InventoryItem> {
    let Ok(file) = parse_file(source) else {
        return Vec::new();
    };
    enum_variant_items(
        &file,
        MIR_VARIANT_ENUMS,
        "mir",
        true,
        InventoryExtractor::MirVariant,
    )
}

/// Extract `runtime.ffi.<symbol>` items from one Rust source file.
///
/// A symbol is a `pub` function with a `C` (or `C-unwind`) ABI carrying a
/// `#[no_mangle]` attribute — spelled `#[unsafe(no_mangle)]` in edition
/// 2024 sources. Anything else (private helpers, non-`no_mangle` ABI
/// shims, Rust-ABI items) is not part of the exported FFI contract.
#[must_use]
pub fn extract_runtime_ffi(source: &str) -> Vec<InventoryItem> {
    let Ok(file) = parse_file(source) else {
        return Vec::new();
    };
    ffi_items_from_file(&file)
}

/// Extract `syntax.SyntaxKind.<Variant>` items from one Rust source
/// file (plan §3.5 row 1: every grammar production/token kind).
///
/// Parsed with `syn`, not regexes, so attribute/macro noise cannot fake
/// a kind. Output is sorted by key so per-file extraction is already
/// deterministic.
#[must_use]
pub fn extract_syntax_kinds(source: &str) -> Vec<InventoryItem> {
    let Ok(file) = parse_file(source) else {
        return Vec::new();
    };
    enum_variant_items(
        &file,
        SYNTAX_KIND_ENUMS,
        "syntax",
        true,
        InventoryExtractor::SyntaxKind,
    )
}

/// Extract `ty.<Enum>.<Variant>` items from one Rust source file (plan
/// §3.5 row 2: the type-system forms `TyKind`, `AdtKind`,
/// `BuiltinTraitDef`).
///
/// Output is sorted by key so per-file extraction is already
/// deterministic.
#[must_use]
pub fn extract_ty_kinds(source: &str) -> Vec<InventoryItem> {
    let Ok(file) = parse_file(source) else {
        return Vec::new();
    };
    enum_variant_items(&file, TY_KIND_ENUMS, "ty", true, InventoryExtractor::TyKind)
}

/// Extract `core.lint.<Id>` items from one Rust source file (plan §3.5
/// row 6: the lint registry enum).
#[must_use]
pub fn extract_lints(source: &str) -> Vec<InventoryItem> {
    let Ok(file) = parse_file(source) else {
        return Vec::new();
    };
    enum_variant_items(
        &file,
        LINT_ENUMS,
        "core.lint",
        false,
        InventoryExtractor::Lint,
    )
}

/// Extract `cli.flag.<long>` items from one Rust source file (plan
/// §3.5 row 5: clap introspection of the `Parser`-derive entry struct).
///
/// The introspection is static (the derive attributes are read through
/// `syn` instead of shelling out to `--help-json`), which keeps the
/// extraction deterministic across machines (constitution C6) while
/// enforcing exactly the same contract: a flag added to
/// [`CLI_ARGS_STRUCT`] with no claim and no ignore fails `ledger check`.
/// A field counts as a flag when its `#[arg(...)]` carries a `long`
/// (explicit `long = "name"` wins, bare `long` kebab-cases the field
/// name); positionals (`value_name` only) and `#[command(skip)]`
/// fields are not part of the flag surface.
#[must_use]
pub fn extract_cli_flags(source: &str) -> Vec<InventoryItem> {
    let Ok(file) = parse_file(source) else {
        return Vec::new();
    };
    cli_items_from_file(&file)
}

/// CLI flag keys of one parsed file (unsorted; callers sort).
fn cli_items_from_file(file: &syn::File) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    for entry in &file.items {
        let syn::Item::Struct(item_struct) = entry else {
            continue;
        };
        if item_struct.ident != CLI_ARGS_STRUCT {
            continue;
        }
        let derives_parser = item_struct.attrs.iter().any(|attr| {
            attr.path().is_ident("derive")
                && attr
                    .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                    .is_ok_and(|paths| paths.iter().any(|path| path.is_ident("Parser")))
        });
        if !derives_parser {
            continue;
        }
        for field in &item_struct.fields {
            let Some(flag) = cli_flag_of_field(field) else {
                continue;
            };
            items.push(InventoryItem {
                extractor: InventoryExtractor::CliFlag,
                key: format!("cli.flag.{flag}"),
                source: String::new(),
            });
        }
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    items
}

/// The `--long` name of a clap field, or `None` when the field is not a
/// flag (positional, `#[command(skip)]`, or no `#[arg]` at all).
fn cli_flag_of_field(field: &syn::Field) -> Option<String> {
    let ident = field.ident.as_ref()?;
    for attr in &field.attrs {
        if attr.path().is_ident("command")
            && attr
                .parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
                .is_ok_and(|metas| metas.iter().any(|meta| meta.path().is_ident("skip")))
        {
            return None;
        }
        if !attr.path().is_ident("arg") {
            continue;
        }
        let metas = attr
            .parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
            .ok()?;
        let mut has_arg_attr = false;
        let mut explicit_long: Option<String> = None;
        let mut bare_long = false;
        for meta in &metas {
            match meta {
                // `long = "opt-level"`: the display spelling wins.
                syn::Meta::NameValue(named) if named.path.is_ident("long") => {
                    has_arg_attr = true;
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(text),
                        ..
                    }) = &named.value
                    {
                        explicit_long = Some(text.value());
                    }
                }
                // Bare `long`: kebab-case the field name.
                syn::Meta::Path(path) if path.is_ident("long") => {
                    has_arg_attr = true;
                    bare_long = true;
                }
                _ => {}
            }
        }
        if let Some(long) = explicit_long {
            return Some(long);
        }
        if bare_long {
            return Some(kebab_case(&ident.to_string()));
        }
        // An `#[arg]` without any `long` is a positional or short-only
        // surface — not a flag.
        if has_arg_attr {
            return None;
        }
    }
    None
}

/// `snake_case` -> `kebab-case` (clap's derive default).
fn kebab_case(snake: &str) -> String {
    snake.replace('_', "-")
}

/// Extract `stdlib.<crate>.<module>.<item>` items from one stdlib
/// source file (plan §3.5 row 4: Glyim's own parser on
/// `glyim-lang-*/lib/*.g`).
///
/// The parse goes through [`glyim_frontend::parse_to_syntax`] — the
/// compiler's own parser, exactly as the plan row prescribes. Items are
/// the top-level declarations (`fn`/`struct`/`enum`/`trait`/`type`/
/// `const`/`static`); `impl` blocks are trait-implementation rows, not
/// API surface, and stay out. A file the parser recovers with
/// diagnostics still contributes its parsed items, and the diagnostics
/// are recorded for the dispatcher's `LEDGER-INVENTORY-PARSE` gate (see
/// [`stdlib_api_had_parse_diagnostics`]). Output is sorted by key.
#[must_use]
pub fn extract_stdlib_api(source: &str, crate_short: &str, module: &str) -> Vec<InventoryItem> {
    STDLIB_PARSE_DIAGNOSTICS.with(|flag| flag.set(0));
    let parse = glyim_frontend::parse_to_syntax(source, glyim_span::FileId::from_usize(0));
    let had_diagnostics = !parse.diagnostics.is_empty();
    STDLIB_PARSE_DIAGNOSTICS.with(|flag| flag.set(usize::from(had_diagnostics)));
    let root = glyim_syntax::SyntaxNode::new_root(parse.green_node);
    let mut items = Vec::new();
    for node in root.children() {
        if !STDLIB_ITEM_KINDS.contains(&node.kind()) {
            continue;
        }
        let Some(name) = node
            .children_with_tokens()
            .find_map(|element| {
                let token = element.into_token()?;
                (token.kind() == glyim_syntax::SyntaxKind::Ident).then(|| token.text().to_string())
            })
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        items.push(InventoryItem {
            extractor: InventoryExtractor::StdlibApi,
            key: format!("stdlib.{crate_short}.{module}.{name}"),
            source: String::new(),
        });
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    items
}

/// Whether the most recent [`extract_stdlib_api`] call on this thread
/// recovered a parse with diagnostics. The dispatcher turns this into a
/// `LEDGER-INVENTORY-PARSE` error so a broken stdlib file can never
/// silently shrink the API gate.
#[must_use]
pub fn stdlib_api_had_parse_diagnostics() -> bool {
    STDLIB_PARSE_DIAGNOSTICS.with(|flag| flag.get() > 0)
}

thread_local! {
    /// Per-thread parse-diagnostics flag of the last stdlib extraction
    /// (thread-local so parallel test runs cannot race).
    static STDLIB_PARSE_DIAGNOSTICS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Glyim item node kinds the stdlib-API extractor reads.
const STDLIB_ITEM_KINDS: &[glyim_syntax::SyntaxKind] = &[
    glyim_syntax::SyntaxKind::FnDef,
    glyim_syntax::SyntaxKind::StructDef,
    glyim_syntax::SyntaxKind::EnumDef,
    glyim_syntax::SyntaxKind::TraitDef,
    glyim_syntax::SyntaxKind::TypeAlias,
    glyim_syntax::SyntaxKind::ConstDef,
    glyim_syntax::SyntaxKind::StaticDef,
];

/// Extract `diag.error_code.<CODE>` items from the error-code registry
/// text (plan §3.5 row 6; the CT-5 source of truth, M3.3).
///
/// Codes render in the diagnostic display spelling — one category
/// letter plus a zero-padded four-digit number (`E0001`), via the
/// [`ERROR_CATEGORY_LETTERS`] mirror of `ErrorCategory::as_letter`. A
/// category outside that table produces no item (the registry's own
/// shape is validated by `glyim-diag`); output is sorted by key.
///
/// # Errors
///
/// A registry that fails to parse as TOML yields no items — the
/// dispatcher surfaces that as `LEDGER-INVENTORY-PARSE` so a broken
/// registry cannot silently shrink the gate.
#[must_use]
pub fn extract_error_codes(registry_text: &str) -> Vec<InventoryItem> {
    let Ok(doc) = toml::from_str::<ErrorCodeRegistry>(registry_text) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for code in &doc.code {
        let Some((_, letter)) = ERROR_CATEGORY_LETTERS
            .iter()
            .find(|(category, _)| category == &code.category)
        else {
            continue;
        };
        items.push(InventoryItem {
            extractor: InventoryExtractor::ErrorCode,
            key: format!("diag.error_code.{letter}{:04}", code.number),
            source: String::new(),
        });
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    items
}

/// The `errors/codes.toml` shape the extractor reads (strict: unknown
/// fields are a schema drift, not data to skip).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorCodeRegistry {
    /// The registered `[[code]]` blocks.
    #[serde(default)]
    code: Vec<ErrorCodeEntry>,
}

/// One `[[code]]` registry entry. Only the key-bearing fields are read;
/// the entry's `title`/`explanation` prose stays the `glyim-diag`
/// registry's business (its loader validates the full shape, so the
/// extractor stays a consumer, not a second validator).
#[derive(Debug, Deserialize)]
struct ErrorCodeEntry {
    /// Diagnostic category (spelled as in `ErrorCategory`).
    category: String,
    /// Diagnostic number (zero-padded in the display spelling).
    number: u16,
}

// ---------------------------------------------------------------------------
// ledger/inventory-ignore.toml
// ---------------------------------------------------------------------------

/// One `[[ignore]]` entry of `ledger/inventory-ignore.toml`: an item that
/// intentionally has no claiming feature, with the reason it is exempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryIgnore {
    /// Exact inventory item key, e.g. `"mir.StatementKind.Nop"`.
    pub item: String,
    /// Why this item needs no feature (audited prose, never a glob).
    pub reason: String,
}

/// File wrapper for `ledger/inventory-ignore.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryIgnoreFile {
    /// The ignore entries declared by the file.
    #[serde(default)]
    pub ignore: Vec<InventoryIgnore>,
}

/// Result of loading `ledger/inventory-ignore.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedIgnore {
    /// Parsed ignore entries.
    pub ignores: Vec<InventoryIgnore>,
    /// Schema diagnostics (unreadable/unparseable file).
    pub diagnostics: Vec<Diagnostic>,
}

/// Load `ledger/inventory-ignore.toml` from `project_root`.
///
/// A missing file is an *empty* ignore list (projects whose extractors
/// find nothing yet need no ignore file); a present-but-invalid file
/// surfaces as a `LEDGER-SCHEMA` error.
#[must_use]
pub fn load_inventory_ignore(project_root: &Path) -> LoadedIgnore {
    let path = project_root.join("ledger").join("inventory-ignore.toml");
    if !path.exists() {
        return LoadedIgnore::default();
    }
    let loaded = |diagnostics| LoadedIgnore {
        ignores: Vec::new(),
        diagnostics,
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return loaded(vec![Diagnostic {
            code: SCHEMA_CODE,
            severity: Severity::Error,
            message: format!("cannot read {}: unreadable", path.display()),
        }]);
    };
    match toml::from_str::<InventoryIgnoreFile>(&text) {
        Ok(file) => LoadedIgnore {
            ignores: file.ignore,
            diagnostics: Vec::new(),
        },
        Err(err) => loaded(vec![Diagnostic {
            code: SCHEMA_CODE,
            severity: Severity::Error,
            message: format!("cannot parse {}: {err}", path.display()),
        }]),
    }
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// Cross-check extracted inventory items against feature claims and the
/// ignore registry (plan §3.5: every item mapped or ignored-with-reason).
///
/// Returns one diagnostic per violation, sorted by `(code, message)`:
///
/// - [`CODE_INVENTORY_UNMAPPED`] — item neither claimed nor ignored;
/// - [`CODE_INVENTORY_STALE_IGNORE`] — ignore entry matches nothing;
/// - [`CODE_INVENTORY_STALE_CLAIM`] — claim matches nothing;
/// - [`CODE_INVENTORY_DUP_CLAIM`] — item claimed by 2+ features;
/// - [`CODE_INVENTORY_DUP_IGNORE`] — item ignored twice;
/// - [`CODE_INVENTORY_CONFLICT`] — item claimed *and* ignored.
#[must_use]
pub fn inventory_diagnostics(
    items: &[InventoryItem],
    ignores: &[InventoryIgnore],
    features: &[Sourced<FeatureEntry>],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Claim index: item key -> declaring feature ids (BTreeMap keeps the
    // whole gate deterministic regardless of load order).
    let mut claims: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for sourced in features {
        for claim in &sourced.entry.inventory {
            claims
                .entry(claim.as_str())
                .or_default()
                .push(&sourced.entry.id);
        }
    }
    for ids in claims.values_mut() {
        ids.sort_unstable();
    }
    let produced: BTreeSet<&str> = items.iter().map(|i| i.key.as_str()).collect();
    let ignored_keys: BTreeSet<&str> = ignores.iter().map(|i| i.item.as_str()).collect();

    // Claims that match nothing: renamed/removed items force a registry
    // cleanup exactly like LEDGER-SPEC-MISSING does for spec paths.
    for (key, ids) in &claims {
        if !produced.contains(key) {
            diagnostics.push(Diagnostic {
                code: CODE_INVENTORY_STALE_CLAIM,
                severity: Severity::Error,
                message: format!(
                    "feature {} claims inventory item `{key}` but no extractor produces it (renamed or removed?)",
                    ids.join(", ")
                ),
            });
        }
        if ids.len() > 1 {
            diagnostics.push(Diagnostic {
                code: CODE_INVENTORY_DUP_CLAIM,
                severity: Severity::Error,
                message: format!(
                    "inventory item `{key}` is claimed by multiple features: {}",
                    ids.join(", ")
                ),
            });
        }
    }

    // Ignore entries: stale ones rot silently, and an ignored item that a
    // feature also claims is almost certainly a curation mistake.
    let mut seen_ignores: BTreeSet<&str> = BTreeSet::new();
    for entry in ignores {
        if !produced.contains(entry.item.as_str()) {
            diagnostics.push(Diagnostic {
                code: CODE_INVENTORY_STALE_IGNORE,
                severity: Severity::Error,
                message: format!(
                    "ledger/inventory-ignore.toml entry `{}` matches no extracted inventory item: {}",
                    entry.item, entry.reason
                ),
            });
        }
        if !seen_ignores.insert(entry.item.as_str()) {
            diagnostics.push(Diagnostic {
                code: CODE_INVENTORY_DUP_IGNORE,
                severity: Severity::Error,
                message: format!(
                    "ledger/inventory-ignore.toml ignores `{}` more than once",
                    entry.item
                ),
            });
        }
        if let Some(ids) = claims.get(entry.item.as_str()) {
            diagnostics.push(Diagnostic {
                code: CODE_INVENTORY_CONFLICT,
                severity: Severity::Error,
                message: format!(
                    "inventory item `{}` is both claimed by {} and ignored in ledger/inventory-ignore.toml",
                    entry.item,
                    ids.join(", ")
                ),
            });
        }
    }

    // The gate itself: every extracted item must be claimed or ignored.
    for item in items {
        if !claims.contains_key(item.key.as_str()) && !ignored_keys.contains(item.key.as_str()) {
            diagnostics.push(Diagnostic {
                code: CODE_INVENTORY_UNMAPPED,
                severity: Severity::Error,
                message: format!(
                    "inventory item `{}` ({}, {}) is neither claimed by a feature's `inventory` list nor ignored in ledger/inventory-ignore.toml",
                    item.key, item.extractor, item.source
                ),
            });
        }
    }

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics
}

/// Keys of `items` that are neither claimed by a feature nor ignored —
/// the subset [`inventory_diagnostics`] reports as
/// [`CODE_INVENTORY_UNMAPPED`]. Shared by the gate and the `ledger
/// inventory` report so both spell "unmapped" identically.
#[must_use]
pub fn unmapped_item_keys(
    items: &[InventoryItem],
    ignores: &[InventoryIgnore],
    features: &[Sourced<FeatureEntry>],
) -> Vec<String> {
    let claimed: BTreeSet<&str> = features
        .iter()
        .flat_map(|sourced| sourced.entry.inventory.iter().map(String::as_str))
        .collect();
    let ignored_keys: BTreeSet<&str> = ignores.iter().map(|i| i.item.as_str()).collect();
    items
        .iter()
        .filter(|item| {
            !claimed.contains(item.key.as_str()) && !ignored_keys.contains(item.key.as_str())
        })
        .map(|item| item.key.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests (written first — RED phase)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn item(extractor: InventoryExtractor, key: &str, source: &str) -> InventoryItem {
        InventoryItem {
            extractor,
            key: key.to_string(),
            source: source.to_string(),
        }
    }

    fn feature(id: &str, claims: &[&str]) -> Sourced<FeatureEntry> {
        Sourced {
            path: std::path::PathBuf::from("ledger/features/x.toml"),
            entry: FeatureEntry {
                id: id.to_string(),
                title: format!("feature {id}"),
                area: "runtime.ffi".to_string(),
                tier: crate::Tier::Std,
                spec: "docs/specs/v0.1.0.md".to_string(),
                target_level: crate::Level::L4,
                backends: vec![crate::Backend::Gam, crate::Backend::Llvm],
                requires: Vec::new(),
                errors: Vec::new(),
                inventory: claims.iter().map(|c| (*c).to_string()).collect(),
                owner_lane: crate::Lane::B,
            },
        }
    }

    fn ignore(item: &str, reason: &str) -> InventoryIgnore {
        InventoryIgnore {
            item: item.to_string(),
            reason: reason.to_string(),
        }
    }

    fn diag_code(diags: &[Diagnostic], code: &str) -> Vec<String> {
        diags
            .iter()
            .filter(|d| d.code == code)
            .map(|d| d.message.clone())
            .collect()
    }

    // -- MIR variant extractor ----------------------------------------------

    const MIR_FIXTURE: &str = r"
use glyim_type::*;

/// `StatementKind`.
pub enum StatementKind {
    /// `Assign`.
    Assign(Place, Rvalue),
    /// `Variant`.
    Nop,
}

/// `Rvalue`.
pub enum Rvalue {
    /// `Use`.
    Use(Operand),
    /// `BinaryOp`.
    BinaryOp(BinOp, Box<(Operand, Operand)>),
}

/// Not tracked (plan §3.5 lists five enums; this one stays out).
pub enum CastKind {
    IntToInt,
}

pub struct Place {
    pub local: u32,
}

enum PrivateEnum {
    Hidden,
}
";

    #[test]
    fn extracts_mir_variants_from_tracked_enums() {
        let items = extract_mir_variants(MIR_FIXTURE);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "mir.Rvalue.BinaryOp",
                "mir.Rvalue.Use",
                "mir.StatementKind.Assign",
                "mir.StatementKind.Nop",
            ],
            "variants of tracked enums extract, sorted by key"
        );
        assert_eq!(
            items[0].extractor,
            InventoryExtractor::MirVariant,
            "MIR extractor tags its items"
        );
    }

    #[test]
    fn mir_extractor_skips_non_tracked_and_private_enums() {
        let items = extract_mir_variants(MIR_FIXTURE);
        assert!(
            items.iter().all(|i| !i.key.contains("CastKind")),
            "CastKind is not a plan §3.5 enum: {items:?}"
        );
        assert!(
            items.iter().all(|i| !i.key.contains("PrivateEnum")),
            "non-pub enums stay out"
        );
        assert!(
            items.iter().all(|i| !i.key.contains("Place")),
            "structs are not variants"
        );
    }

    #[test]
    fn mir_extractor_handles_struct_variants_and_generics() {
        let source = r"
pub enum ProjectionElem<D> {
    Deref,
    Field(u32),
    ConstantIndex { offset: u64, from_end: bool },
}
";
        let items = extract_mir_variants(source);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "mir.ProjectionElem.ConstantIndex",
                "mir.ProjectionElem.Deref",
                "mir.ProjectionElem.Field"
            ],
            "unit/tuple/struct variants all extract; generics do not leak into keys"
        );
    }

    #[test]
    fn mir_extractor_output_is_sorted_by_key() {
        let items =
            extract_mir_variants("pub enum Operand { Copy(u32), Move(u32), Constant(u32) }");
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "mir.Operand.Constant",
                "mir.Operand.Copy",
                "mir.Operand.Move"
            ],
            "sorted by key, not source order — deterministic output"
        );
    }

    // -- runtime FFI extractor ----------------------------------------------

    const FFI_FIXTURE: &str = r#"
use std::ffi::c_char;

#[unsafe(no_mangle)]
pub extern "C" fn glyim_alloc(size: usize, align: usize) -> *mut u8 {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn glyim_legacy() {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_dealloc(ptr: *mut u8, size: usize, align: usize) {}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn glyim_unwind() {}

#[unsafe(no_mangle)]
fn glyim_private() {}

pub extern "C" fn glyim_exported_without_attribute() {}

#[unsafe(no_mangle)]
pub extern "Rust" fn glyim_rust_abi() {}
"#;

    #[test]
    fn extracts_no_mangle_c_abi_symbols() {
        let items = extract_runtime_ffi(FFI_FIXTURE);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "runtime.ffi.glyim_alloc",
                "runtime.ffi.glyim_dealloc",
                "runtime.ffi.glyim_legacy",
                "runtime.ffi.glyim_unwind",
            ],
            "no_mangle + C/C-unwind ABI + pub is the exported FFI contract"
        );
        assert_eq!(
            items[0].extractor,
            InventoryExtractor::RuntimeFfi,
            "FFI extractor tags its items"
        );
    }

    #[test]
    fn ffi_extractor_rejects_helpers_and_non_c_abi() {
        let items = extract_runtime_ffi(FFI_FIXTURE);
        for banned in [
            "glyim_private",
            "glyim_exported_without_attribute",
            "glyim_rust_abi",
        ] {
            assert!(
                items.iter().all(|i| !i.key.contains(banned)),
                "{banned} must not extract"
            );
        }
    }

    #[test]
    fn ffi_extractor_ignores_non_fn_items() {
        let items = extract_runtime_ffi("#[unsafe(no_mangle)]\npub static GLYIM_TABLE: u32 = 7;\n");
        assert!(items.is_empty(), "statics are not symbols: {items:?}");
    }

    // -- full extraction ------------------------------------------------------

    #[test]
    fn missing_source_dirs_yield_empty_extraction() {
        let dir = tempfile::tempdir().expect("tmp");
        let extraction = extract_inventory(dir.path());
        assert!(extraction.items.is_empty());
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extraction_walks_real_trees_deterministically() {
        let dir = tempfile::tempdir().expect("tmp");
        let mir = dir.path().join(MIR_SOURCE_DIR);
        let runtime = dir.path().join(RUNTIME_SOURCE_DIR);
        std::fs::create_dir_all(&mir).expect("mkdir mir");
        std::fs::create_dir_all(&runtime).expect("mkdir runtime");
        std::fs::write(mir.join("lib.rs"), "pub enum Operand { Copy(u32) }").expect("write mir");
        std::fs::write(mir.join("zeta.rs"), "pub enum Rvalue { Use(Operand) }")
            .expect("write zeta");
        std::fs::write(runtime.join("ffi.rs"), MIR_FIXTURE).expect("write runtime");
        let extraction = extract_inventory(dir.path());
        assert_eq!(extraction.items.len(), 2, "1 operand + 1 rvalue + 0 ffi");
        assert_eq!(extraction.items[0].key, "mir.Operand.Copy");
        assert_eq!(extraction.items[1].key, "mir.Rvalue.Use");
        assert_eq!(
            extraction.items[0].source,
            format!("{}", mir.join("lib.rs").display()),
            "items carry their source file"
        );
        let again = extract_inventory(dir.path());
        assert_eq!(extraction, again, "deterministic");
    }

    #[test]
    fn unparseable_source_reports_a_parse_diagnostic() {
        let dir = tempfile::tempdir().expect("tmp");
        let mir = dir.path().join(MIR_SOURCE_DIR);
        std::fs::create_dir_all(&mir).expect("mkdir");
        std::fs::write(mir.join("broken.rs"), "pub enum Broken {").expect("write");
        let extraction = extract_inventory(dir.path());
        assert!(
            extraction
                .diagnostics
                .iter()
                .any(|d| d.code == CODE_INVENTORY_PARSE && d.severity == Severity::Error),
            "broken source must surface as an error: {:?}",
            extraction.diagnostics
        );
    }

    // -- ignore file ----------------------------------------------------------

    #[test]
    fn ignore_file_missing_is_empty_and_clean() {
        let dir = tempfile::tempdir().expect("tmp");
        let loaded = load_inventory_ignore(dir.path());
        assert!(loaded.ignores.is_empty());
        assert!(loaded.diagnostics.is_empty());
    }

    #[test]
    fn ignore_file_loads_entries_in_order() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("ledger/inventory-ignore.toml");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &path,
            "[[ignore]]\n\
             item = \"mir.StatementKind.Nop\"\n\
             reason = \"deliberate no-op statement\"\n\n\
             [[ignore]]\n\
             item = \"runtime.ffi.glyim_internal\"\n\
             reason = \"not part of the exported contract\"\n",
        )
        .expect("write");
        let loaded = load_inventory_ignore(dir.path());
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        assert_eq!(loaded.ignores.len(), 2);
        assert_eq!(loaded.ignores[0].item, "mir.StatementKind.Nop");
        assert_eq!(
            loaded.ignores[1].reason,
            "not part of the exported contract"
        );
    }

    #[test]
    fn ignore_file_rejects_unknown_fields() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("ledger/inventory-ignore.toml");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "[[ignore]]\nitem = \"x\"\nreason = \"y\"\nwat = 1\n")
            .expect("write");
        let loaded = load_inventory_ignore(dir.path());
        assert!(
            loaded
                .diagnostics
                .iter()
                .any(|d| d.code == SCHEMA_CODE && d.severity == Severity::Error),
            "unknown fields are load errors (deny_unknown_fields): {:?}",
            loaded.diagnostics
        );
    }

    // -- the gate -------------------------------------------------------------

    fn items() -> Vec<InventoryItem> {
        vec![
            item(
                InventoryExtractor::RuntimeFfi,
                "runtime.ffi.glyim_alloc",
                "crates/glyim-runtime/src/lib.rs",
            ),
            item(
                InventoryExtractor::MirVariant,
                "mir.StatementKind.Nop",
                "crates/glyim-mir/src/lib.rs",
            ),
        ]
    }

    #[test]
    fn every_item_claimed_passes() {
        let diags = inventory_diagnostics(
            &items(),
            &[],
            &[feature("RUNTIME-FFI-ALLOC", &["runtime.ffi.glyim_alloc"])],
        );
        let unmapped = diag_code(&diags, CODE_INVENTORY_UNMAPPED);
        assert_eq!(unmapped.len(), 1, "Nop is unclaimed: {diags:?}");
        assert!(unmapped[0].contains("mir.StatementKind.Nop"));
    }

    #[test]
    fn every_item_ignored_passes() {
        let diags = inventory_diagnostics(
            &items(),
            &[
                ignore("runtime.ffi.glyim_alloc", "unused by fixtures"),
                ignore("mir.StatementKind.Nop", "no-op placeholder"),
            ],
            &[],
        );
        assert!(diags.is_empty(), "fully ignored registry passes: {diags:?}");
    }

    #[test]
    fn claim_and_ignore_split_passes() {
        let diags = inventory_diagnostics(
            &items(),
            &[ignore("mir.StatementKind.Nop", "no-op placeholder")],
            &[feature("RUNTIME-FFI-ALLOC", &["runtime.ffi.glyim_alloc"])],
        );
        assert!(diags.is_empty(), "split coverage passes: {diags:?}");
    }

    #[test]
    fn unmapped_item_is_an_error() {
        let diags = inventory_diagnostics(&items(), &[], &[]);
        let unmapped = diag_code(&diags, CODE_INVENTORY_UNMAPPED);
        assert_eq!(unmapped.len(), 2, "both items unmapped: {diags:?}");
        assert!(
            unmapped[0].contains("ledger/inventory-ignore.toml"),
            "message names the fix paths: {}",
            unmapped[0]
        );
    }

    #[test]
    fn stale_ignore_is_an_error() {
        let diags = inventory_diagnostics(
            &[item(
                InventoryExtractor::MirVariant,
                "mir.Operand.Copy",
                "crates/glyim-mir/src/lib.rs",
            )],
            &[ignore("mir.Operand.Ghost", "renamed upstream")],
            &[],
        );
        let stale = diag_code(&diags, CODE_INVENTORY_STALE_IGNORE);
        assert_eq!(stale.len(), 1, "{diags:?}");
        assert!(stale[0].contains("mir.Operand.Ghost"));
        assert!(stale[0].contains("renamed upstream"), "reason echoed");
    }

    #[test]
    fn stale_claim_is_an_error() {
        let diags = inventory_diagnostics(
            &[],
            &[],
            &[feature("RUNTIME-FFI-ALLOC", &["runtime.ffi.glyim_ghost"])],
        );
        let stale = diag_code(&diags, CODE_INVENTORY_STALE_CLAIM);
        assert_eq!(stale.len(), 1, "{diags:?}");
        assert!(stale[0].contains("RUNTIME-FFI-ALLOC"));
        assert!(stale[0].contains("runtime.ffi.glyim_ghost"));
    }

    #[test]
    fn duplicate_claims_are_an_error() {
        let diags = inventory_diagnostics(
            &items(),
            &[ignore("mir.StatementKind.Nop", "no-op placeholder")],
            &[
                feature("RUNTIME-FFI-ALLOC", &["runtime.ffi.glyim_alloc"]),
                feature("RUNTIME-FFI-VECSTR", &["runtime.ffi.glyim_alloc"]),
            ],
        );
        let dup = diag_code(&diags, CODE_INVENTORY_DUP_CLAIM);
        assert_eq!(dup.len(), 1, "{diags:?}");
        assert!(dup[0].contains("RUNTIME-FFI-ALLOC"));
        assert!(dup[0].contains("RUNTIME-FFI-VECSTR"));
    }

    #[test]
    fn duplicate_ignores_are_an_error() {
        let diags = inventory_diagnostics(
            &items(),
            &[
                ignore("mir.StatementKind.Nop", "first"),
                ignore("mir.StatementKind.Nop", "second"),
                ignore("runtime.ffi.glyim_alloc", "unused"),
            ],
            &[],
        );
        let dup = diag_code(&diags, CODE_INVENTORY_DUP_IGNORE);
        assert_eq!(dup.len(), 1, "{diags:?}");
    }

    #[test]
    fn claim_and_ignore_conflict_is_an_error() {
        let diags = inventory_diagnostics(
            &items(),
            &[ignore("runtime.ffi.glyim_alloc", "unused")],
            &[feature("RUNTIME-FFI-ALLOC", &["runtime.ffi.glyim_alloc"])],
        );
        let conflict = diag_code(&diags, CODE_INVENTORY_CONFLICT);
        assert_eq!(conflict.len(), 1, "{diags:?}");
        assert!(conflict[0].contains("runtime.ffi.glyim_alloc"));
        assert!(conflict[0].contains("RUNTIME-FFI-ALLOC"));
    }

    #[test]
    fn gate_diagnostics_are_sorted_by_code_then_message() {
        let diags = inventory_diagnostics(&items(), &[], &[]);
        let mut codes: Vec<&str> = diags.iter().map(|d| d.code).collect();
        codes.sort_unstable();
        let sorted = diags
            .iter()
            .map(|d| (d.code, d.message.clone()))
            .collect::<Vec<_>>();
        let mut sorted_ref = sorted.clone();
        sorted_ref.sort();
        assert_eq!(sorted, sorted_ref, "deterministic (code, message) order");
    }

    #[test]
    fn claim_maps_are_built_from_sorted_features() {
        // Two features, out-of-order ids; the dup message must list ids
        // sorted, proving the map is deterministic regardless of load
        // order.
        let diags = inventory_diagnostics(
            &items(),
            &[ignore("mir.StatementKind.Nop", "no-op placeholder")],
            &[
                feature("ZZZ", &["runtime.ffi.glyim_alloc"]),
                feature("AAA", &["runtime.ffi.glyim_alloc"]),
            ],
        );
        let dup = diag_code(&diags, CODE_INVENTORY_DUP_CLAIM);
        assert!(dup[0].contains("AAA, ZZZ"), "sorted ids: {}", dup[0]);
    }

    #[test]
    fn mir_variant_of_every_tracked_enum_requires_registration() {
        // The M1.3 DoD, at the unit level: a variant that appears in
        // source with no claim and no ignore is an error.
        let source = "pub enum TerminatorKind { Goto, Call, Return }";
        let extracted = extract_mir_variants(source);
        assert_eq!(extracted.len(), 3);
        let diags = inventory_diagnostics(&extracted, &[], &[]);
        assert_eq!(diag_code(&diags, CODE_INVENTORY_UNMAPPED).len(), 3);
    }

    // -- M1.3 remainder: the remaining §3.5 extractor families ------------
    //
    // Plan §3.5 rows 1/2/4/5/6: SyntaxKind, TyKind forms, stdlib API via
    // the Glyim parser, CLI flags via clap introspection, error/lint
    // registries. Each family follows the MIR contract: tracked items
    // extract deterministically (sorted keys) and fail the gate until a
    // feature claims them or an ignore entry exempts them with a reason.

    const SYNTAX_KIND_FIXTURE: &str = r"
/// `SyntaxKind`.
pub enum SyntaxKind {
    /// `KwFn`.
    KwFn,
    /// `Whitespace`.
    Whitespace,
}

/// Not tracked (only `SyntaxKind` maps to features).
pub enum TriviaKind {
    Hidden,
}

enum PrivateEnum {
    Hidden,
}
";

    #[test]
    fn extracts_syntax_kinds_from_the_tracked_enum() {
        let items = extract_syntax_kinds(SYNTAX_KIND_FIXTURE);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["syntax.SyntaxKind.KwFn", "syntax.SyntaxKind.Whitespace"],
            "SyntaxKind variants extract, sorted by key"
        );
        assert_eq!(items[0].extractor, InventoryExtractor::SyntaxKind);
        assert_eq!(InventoryExtractor::SyntaxKind.as_str(), "syntax-kind");
    }

    #[test]
    fn syntax_kind_without_registration_fails_the_gate() {
        // Plan §6 M1.3 DoD (docs/roadmaps/glyim-master-plan.md §6, M1.3
        // row): "adding a MIR variant or CLI flag without registration
        // fails ledger check (tested)" — the same contract every §3.5
        // family upholds; this pins it for the syntax family.
        let extracted = extract_syntax_kinds(SYNTAX_KIND_FIXTURE);
        let diags = inventory_diagnostics(&extracted, &[], &[]);
        assert_eq!(diag_code(&diags, CODE_INVENTORY_UNMAPPED).len(), 2);
        let claimed = inventory_diagnostics(
            &extracted,
            &[ignore("syntax.SyntaxKind.KwFn", "token kind")],
            &[feature("LANG-SYN-FN-DEF", &["syntax.SyntaxKind.KwFn"])],
        );
        assert_eq!(diag_code(&claimed, CODE_INVENTORY_UNMAPPED).len(), 1);
    }

    const TY_KIND_FIXTURE: &str = r"
/// `TyKind`.
pub enum TyKind {
    /// `Never`.
    Never,
    /// `Adt`.
    Adt(AdtId, Substitution),
    /// `Error`.
    Error,
}

/// `AdtKind`.
pub enum AdtKind {
    /// `Enum`.
    Enum,
}

/// `BuiltinTraitDef`.
pub enum BuiltinTraitDef {
    /// `Add`.
    Add,
}
";

    #[test]
    fn extracts_ty_kinds_from_the_tracked_enums() {
        let items = extract_ty_kinds(TY_KIND_FIXTURE);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "ty.AdtKind.Enum",
                "ty.BuiltinTraitDef.Add",
                "ty.TyKind.Adt",
                "ty.TyKind.Error",
                "ty.TyKind.Never",
            ],
            "TyKind/AdtKind/BuiltinTraitDef variants extract, sorted by key"
        );
        assert_eq!(items[0].extractor, InventoryExtractor::TyKind);
        assert_eq!(InventoryExtractor::TyKind.as_str(), "ty-kind");
    }

    const CLI_FLAG_FIXTURE: &str = r#"
#[derive(Parser, Debug)]
#[command(name = "glyim")]
pub struct CliArgs {
    /// Input file (positional, not a flag).
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Output path.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Optimization level.
    #[arg(short = 'O', long = "opt-level", default_value = "0")]
    pub opt_level: u8,

    /// Prepend the stdlib source.
    #[arg(long, default_value_t = false)]
    pub with_stdlib: bool,

    /// Not a CLI surface (skipped by clap).
    #[command(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// Not the parser entry struct.
#[derive(Debug)]
pub struct OtherArgs {
    pub hidden: u8,
}
"#;

    #[test]
    fn extracts_cli_flags_from_the_parser_derive() {
        let items = extract_cli_flags(CLI_FLAG_FIXTURE);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "cli.flag.opt-level",
                "cli.flag.output",
                "cli.flag.with-stdlib"
            ],
            "long flags extract (field-name fallback kebab-cased), positional and skip fields stay out"
        );
        assert_eq!(items[0].extractor, InventoryExtractor::CliFlag);
        assert_eq!(InventoryExtractor::CliFlag.as_str(), "cli-flag");
    }

    #[test]
    fn new_cli_flag_without_registration_fails_the_check() {
        // Plan §6 M1.3 DoD (docs/roadmaps/glyim-master-plan.md §6, M1.3
        // row): "adding a MIR variant or CLI flag without registration
        // fails ledger check (tested)". This is that test, on the CLI
        // face: a flag extracted from the derive with no claim and no
        // ignore is a `LEDGER-INVENTORY-UNMAPPED` error, and claiming it
        // through the feature registry clears exactly that flag.
        let extracted = extract_cli_flags(CLI_FLAG_FIXTURE);
        let diags = inventory_diagnostics(&extracted, &[], &[]);
        assert_eq!(diag_code(&diags, CODE_INVENTORY_UNMAPPED).len(), 3);
        let claimed = inventory_diagnostics(
            &extracted,
            &[],
            &[feature("TOOL-CLI-EMIT-FLAGS", &["cli.flag.output"])],
        );
        let unmapped = diag_code(&claimed, CODE_INVENTORY_UNMAPPED);
        assert_eq!(unmapped.len(), 2, "claimed flag clears: {unmapped:?}");
        assert!(unmapped.iter().all(|m| !m.contains("cli.flag.output")));
    }

    const STDLIB_FIXTURE: &str = r#"//! The `Default` trait for the Glyim core library.

/// A trait for giving a type a useful default value.
trait Default: Sized {
    /// Returns the "default value" for a type.
    fn default() -> Self;
}

impl Default for () { fn default() -> Self {} }

struct Vec2 { x: i32 }
"#;

    #[test]
    fn extracts_stdlib_api_top_level_items() {
        let items = extract_stdlib_api(STDLIB_FIXTURE, "core", "default");
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["stdlib.core.default.Default", "stdlib.core.default.Vec2"],
            "top-level trait/struct items extract; impl blocks and their methods stay out"
        );
        assert_eq!(items[0].extractor, InventoryExtractor::StdlibApi);
        assert_eq!(InventoryExtractor::StdlibApi.as_str(), "stdlib-api");
    }

    #[test]
    fn stdlib_api_source_is_parsed_with_glyims_own_parser() {
        // Plan §3.5 row 4 prescribes "Glyim's own parser on
        // glyim-lang-*/lib/*.g"; a source that does not lex is a parse
        // failure the dispatcher surfaces, never a silent empty list.
        let garbage = "fn { broken !!(";
        let items = extract_stdlib_api(garbage, "std", "io");
        assert!(items.is_empty(), "nothing extractable: {items:?}");
        assert!(
            stdlib_api_had_parse_diagnostics(),
            "the parse failure is recorded for the LEDGER-INVENTORY-PARSE gate"
        );
    }

    const ERROR_CODE_FIXTURE: &str = r#"
[[code]]
category = "Lex"
number = 1
title = "lex error"
explanation = "The lexer could not produce a token."

[[code]]
category = "Type"
number = 60
title = "await in loop"
explanation = "Await inside a loop body is rejected."
"#;

    #[test]
    fn extracts_error_codes_from_the_registry() {
        let items = extract_error_codes(ERROR_CODE_FIXTURE);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["diag.error_code.E0001", "diag.error_code.T0060"],
            "codes render as letter + zero-padded number (the diagnostic spelling)"
        );
        assert_eq!(items[0].extractor, InventoryExtractor::ErrorCode);
        assert_eq!(InventoryExtractor::ErrorCode.as_str(), "error-code");
    }

    const LINT_FIXTURE: &str = r"
/// A named compiler lint.
pub enum LintId {
    /// Call to a declared `unsafe fn`.
    UnsafeFnCall,
    /// Unused generic parameters.
    UnusedGenericParams,
}

pub enum NotALint {
    Hidden,
}
";

    #[test]
    fn extracts_lints_from_the_registry_enum() {
        let items = extract_lints(LINT_FIXTURE);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["core.lint.UnsafeFnCall", "core.lint.UnusedGenericParams"]
        );
        assert_eq!(items[0].extractor, InventoryExtractor::Lint);
        assert_eq!(InventoryExtractor::Lint.as_str(), "lint");
    }
}
