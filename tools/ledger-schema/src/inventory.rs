//! Inventory extractors (plan M1.3, §3.5) — `ledger inventory`.
//!
//! The registry declares what we claim to build (M1.2); the extractors
//! enumerate, from source, everything that *should* map to a feature.
//! `ledger check` fails when an extracted item is neither claimed by a
//! feature's `inventory` list nor explicitly ignored (with a reason) in
//! `ledger/inventory-ignore.toml`, so adding a MIR variant or a runtime
//! FFI symbol without registration breaks CI — drift between the registry
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
//! The remaining §3.5 families (`SyntaxKind`/`TyKind` enums, stdlib API
//! via the Glyim parser, CLI flags via clap introspection, error/lint
//! registries) plug into the same item/ignore framework and are tracked
//! as M1.3 follow-ups.

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
}

impl InventoryExtractor {
    /// Stable kebab-case spelling used in reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            InventoryExtractor::MirVariant => "mir-variant",
            InventoryExtractor::RuntimeFfi => "runtime-ffi",
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
            InventoryExtractor::MirVariant => mir_items_from_file(&file),
            InventoryExtractor::RuntimeFfi => ffi_items_from_file(&file),
        };
        let display = path.display().to_string();
        items.extend(extracted.into_iter().map(|mut item| {
            item.source.clone_from(&display);
            item
        }));
    }
}

/// MIR enum-variant keys of one parsed file (unsorted; callers sort).
fn mir_items_from_file(file: &syn::File) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    for entry in &file.items {
        let syn::Item::Enum(item_enum) = entry else {
            continue;
        };
        if !matches!(item_enum.vis, syn::Visibility::Public(_)) {
            continue;
        }
        let name = item_enum.ident.to_string();
        if !MIR_VARIANT_ENUMS.contains(&name.as_str()) {
            continue;
        }
        for variant in &item_enum.variants {
            items.push(InventoryItem {
                extractor: InventoryExtractor::MirVariant,
                key: format!("mir.{name}.{}", variant.ident),
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
    mir_items_from_file(&file)
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
}
