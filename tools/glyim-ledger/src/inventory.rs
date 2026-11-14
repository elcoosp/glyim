//! Inventory-ignore write faces (plan §3.5 remainder) —
//! `inventory ignore` / `inventory unignore`.
//!
//! The §3.5 gate (`ledger check` + `ledger inventory`) already fails when
//! an extracted item is neither claimed by a feature nor ignored with a
//! reason in `ledger/inventory-ignore.toml` — but until these faces the
//! registry itself was hand-edited: there was no sanctioned way to file
//! the ignore entry the failing check demands, and no way to retire one
//! once the item gains a claiming feature. The faces close that loop the
//! same way the gap faces closed the witness loop (`gaps witness` /
//! `gaps unwitness`): a typed request, machine-checked validation against
//! the live tree, a canonical write seam, and nothing written on a
//! rejected request.
//!
//! Two faces land in this slice:
//!
//! - **`inventory ignore`** ([`run_inventory_ignore`]): file one
//!   `[[ignore]]` block for an *extracted* item key. The key must be
//!   produced by an extractor right now (exact keys, never globs — a
//!   pre-registered ignore for an item that does not exist would
//!   silently swallow future drift), must not be claimed by a feature
//!   (that is the [`ledger_schema::CODE_INVENTORY_CONFLICT`] failure the
//!   gate would report), and must not already be ignored.
//! - **`inventory unignore`** ([`run_inventory_unignore`]): remove one
//!   `[[ignore]]` block by its exact item key and rewrite the remaining
//!   entries canonically. The item becomes unmapped again the moment the
//!   rewrite lands — the §3.5 gate is the loud follow-up that forces a
//!   feature claim (or a fresh ignore) before `ledger check` passes.
//!
//! Both faces share the [`IgnoreSink`] / [`IgnoreRewriter`] write seams
//! (`File*` for the CLI, `InMemory*` for tests and programmatic
//! consumers) and emit the `glyim.ledger-inventory-ignore/1` /
//! `glyim.ledger-inventory-unignore/1` JSON contracts through the
//! `--format json` flag.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ledger_schema::{
    InventoryIgnore, InventoryIgnoreFile, Severity, extract_inventory, load_inventory_ignore,
    load_project_registry,
};
use serde::Serialize;

use crate::{LedgerCliError, usage};
// ---------------------------------------------------------------------------
// Schema tags
// ---------------------------------------------------------------------------

/// Schema tag of the `inventory ignore` JSON contract.
pub const INVENTORY_IGNORE_SCHEMA: &str = "glyim.ledger-inventory-ignore/1";

/// Schema tag of the `inventory unignore` JSON contract.
pub const INVENTORY_UNIGNORE_SCHEMA: &str = "glyim.ledger-inventory-unignore/1";

/// Committed registry path the write faces target (relative to ROOT).
pub const INVENTORY_IGNORE_TARGET: &str = "ledger/inventory-ignore.toml";

// ---------------------------------------------------------------------------
// Write seams
// ---------------------------------------------------------------------------

/// Where a filed `[[ignore]]` block lands. The write seam mirrors
/// [`crate::WaiverSink`]: the engine consumes the trait, so tests record
/// fragments in memory and the CLI wires the file-backed implementation.
pub trait IgnoreSink {
    /// Append the canonical `[[ignore]]` fragment to the document at
    /// `path` (created when absent); the error detail is the IO failure
    /// message (the CLI turns it into a typed write error).
    fn append(&self, path: &Path, fragment: &str) -> Result<(), String>;
}

/// Appends to `<ROOT>/ledger/inventory-ignore.toml` on disk (creating the
/// `ledger/` directory when it does not exist yet): the committed
/// exemption registry the §3.5 gate reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileIgnoreSink;

impl IgnoreSink for FileIgnoreSink {
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
/// programmatic consumers; the in-memory twin of [`FileIgnoreSink`].
#[derive(Debug, Default)]
pub struct InMemoryIgnoreSink {
    /// Every `(path, fragment)` the sink was asked to append, in order.
    pub written: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl IgnoreSink for InMemoryIgnoreSink {
    fn append(&self, path: &Path, fragment: &str) -> Result<(), String> {
        self.written
            .borrow_mut()
            .push((path.to_path_buf(), fragment.to_string()));
        Ok(())
    }
}

/// Canonical document carrier for the rewriter: parsing the committed
/// `ledger/inventory-ignore.toml` into the entry array and
/// re-serializing the remaining entries produces the canonical document
/// (field order = struct order, escaped by the `toml` crate — the same
/// discipline [`InventoryIgnoreFile`] applies on the read side).
#[derive(Serialize)]
struct UnignoreDoc {
    ignore: Vec<InventoryIgnore>,
}

/// Where a removed `[[ignore]]` block is taken from. The rewrite seam
/// mirrors [`crate::WaiverRewriter`] on the write side: the engine
/// consumes the trait, so tests drive an in-memory document and the CLI
/// wires the file-backed implementation.
pub trait IgnoreRewriter {
    /// Removes the `[[ignore]]` block carrying the exact `item` from the
    /// document at `path` and rewrites the remaining entries canonically.
    /// Returns the removed entry, or `None` when the document no longer
    /// carries it; the error detail is the IO or serialization failure
    /// message (the CLI turns it into a typed write error).
    fn remove(&self, path: &Path, item: &str) -> Result<Option<InventoryIgnore>, String>;
}

/// Rewrites `<ROOT>/ledger/inventory-ignore.toml` on disk: parse, drop
/// the one matching block, re-serialize the remaining entries
/// canonically, and replace the file. The rewrite never silently
/// reorders or drops other entries — a document that fails to parse is
/// refused (`Err`), not truncated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileIgnoreRewriter;

impl IgnoreRewriter for FileIgnoreRewriter {
    fn remove(&self, path: &Path, item: &str) -> Result<Option<InventoryIgnore>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let doc: InventoryIgnoreFile = toml::from_str(&text)
            .map_err(|err| format!("cannot parse {}: {err}", path.display()))?;
        let Some(position) = doc.ignore.iter().position(|entry| entry.item == item) else {
            return Ok(None);
        };
        let removed = doc.ignore[position].clone();
        let mut remaining = doc.ignore;
        remaining.remove(position);
        let rewritten = toml::to_string(&UnignoreDoc { ignore: remaining })
            .map_err(|err| format!("cannot serialize {}: {err}", path.display()))?;
        std::fs::write(path, &rewritten)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(Some(removed))
    }
}

/// Operates on an owned in-memory document (tests and programmatic
/// consumers); the in-memory twin of [`FileIgnoreRewriter`]. Every
/// successful removal is recorded in [`Self::removed`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryIgnoreRewriter {
    /// The document the rewriter operates on (the file's entries).
    pub doc: std::cell::RefCell<Vec<InventoryIgnore>>,
    /// Every `(path, removed item key)` performed, in order.
    pub removed: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl IgnoreRewriter for InMemoryIgnoreRewriter {
    fn remove(&self, path: &Path, item: &str) -> Result<Option<InventoryIgnore>, String> {
        let mut doc = self.doc.borrow_mut();
        if let Some(position) = doc.iter().position(|entry| entry.item == item) {
            let removed = doc.remove(position);
            self.removed
                .borrow_mut()
                .push((path.to_path_buf(), item.to_string()));
            Ok(Some(removed))
        } else {
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Requests and outcomes
// ---------------------------------------------------------------------------

/// The `inventory ignore` request (the CLI flags, validated by
/// [`run_inventory_ignore`]): file one exemption for an extracted item
/// key that deliberately has no claiming feature — the sanctioned answer
/// to a failing §3.5 check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryIgnoreRequest {
    /// The exact inventory item key, e.g. `runtime.ffi.glyim_alloc`.
    pub item: String,
    /// Why this item needs no feature (audited prose, never a glob).
    pub reason: String,
}

/// Result of an `inventory ignore` run (also the
/// `glyim.ledger-inventory-ignore/1` JSON contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryIgnoreOutcome {
    /// Schema tag, always [`INVENTORY_IGNORE_SCHEMA`].
    pub schema: &'static str,
    /// The exempted item key, echoed verbatim.
    pub item: String,
    /// The filed reason, echoed verbatim.
    pub reason: String,
    /// The document the block landed in.
    pub target: String,
    /// The exact canonical bytes appended (the audit trail).
    pub fragment: String,
    /// Ignore entries in the committed document after the append.
    pub remaining: usize,
}

/// The `inventory unignore` request (the CLI flags, validated by
/// [`run_inventory_unignore`]): retire one exemption by its exact item
/// key — the counterpart of `inventory ignore` for items that gained a
/// claiming feature (or whose reason no longer holds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryUnignoreRequest {
    /// The exact inventory item key to remove.
    pub item: String,
}

/// Result of an `inventory unignore` run (also the
/// `glyim.ledger-inventory-unignore/1` JSON contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryUnignoreOutcome {
    /// Schema tag, always [`INVENTORY_UNIGNORE_SCHEMA`].
    pub schema: &'static str,
    /// The retired item key, echoed verbatim.
    pub item: String,
    /// The removed entry's reason (the audit trail for *why* it existed).
    pub reason: String,
    /// Ignore entries remaining in the committed document after the
    /// removal.
    pub remaining: usize,
    /// The document the block was removed from.
    pub target: String,
}

// ---------------------------------------------------------------------------
// Engine — inventory ignore (add face)
// ---------------------------------------------------------------------------

/// Files one §3.5 exemption: validates the request against the live tree
/// (extractors + feature registry + committed ignore registry), composes
/// the canonical `[[ignore]]` block, and appends it through `sink`.
///
/// # Validation contract
///
/// - `item` must be usable (non-blank, no control characters);
/// - `reason` must be usable (non-blank, no control characters);
/// - the tree must load cleanly (feature registry, extractors, and the
///   committed registry itself — a document that does not parse is never
///   appended to);
/// - `item` must be produced by an extractor *right now* (exact keys,
///   never globs: a pre-registered ignore for an item that does not exist
///   would silently swallow future drift, which is the
///   [`ledger_schema::CODE_INVENTORY_STALE_IGNORE`] failure the gate
///   reports);
/// - `item` must not be claimed by a feature (claimed *and* ignored is
///   the [`ledger_schema::CODE_INVENTORY_CONFLICT`] failure);
/// - the block must not already exist (a re-file is a duplicate, never a
///   silent overwrite).
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`InventoryIgnoreBadItem`, `InventoryIgnoreMissingField`,
/// `InventoryIgnoreLoad`, `InventoryIgnoreUnknownItem`,
/// `InventoryIgnoreClaimed`, `InventoryIgnoreDuplicate`) and for a
/// failed append (`InventoryIgnoreWrite`).
pub fn run_inventory_ignore(
    root: &Path,
    request: &InventoryIgnoreRequest,
    sink: &dyn IgnoreSink,
) -> Result<InventoryIgnoreOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "inventory_ignore",
        root = %root.display(),
        item = %request.item,
    );
    let _enter = span.enter();

    if request.item.trim().is_empty() || request.item.chars().any(char::is_control) {
        return Err(LedgerCliError::InventoryIgnoreBadItem {
            item: request.item.clone(),
        });
    }
    if request.reason.trim().is_empty() || request.reason.chars().any(char::is_control) {
        return Err(LedgerCliError::InventoryIgnoreMissingField {
            field: "--reason".to_string(),
            usage: usage(),
        });
    }

    // The exemption is about an item the extractors produce *today*, so
    // the whole tree has to load cleanly first: registry, extraction, and
    // the committed registry itself.
    let loaded = load_project_registry(root);
    let extraction = extract_inventory(root);
    let ignores = load_inventory_ignore(root);
    let load_errors = loaded
        .diagnostics
        .iter()
        .chain(extraction.diagnostics.iter())
        .chain(ignores.diagnostics.iter())
        .filter(|d| d.severity == Severity::Error)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !load_errors.is_empty() {
        return Err(LedgerCliError::InventoryIgnoreLoad {
            detail: load_errors.join("; "),
        });
    }

    if !extraction.items.iter().any(|i| i.key == request.item) {
        let candidates = extraction
            .items
            .iter()
            .take(3)
            .map(|i| i.key.clone())
            .collect::<Vec<_>>();
        return Err(LedgerCliError::InventoryIgnoreUnknownItem {
            item: request.item.clone(),
            candidates,
        });
    }
    let claimants = loaded
        .registry
        .features()
        .iter()
        .filter(|sourced| sourced.entry.inventory.iter().any(|c| c == &request.item))
        .map(|sourced| sourced.entry.id.clone())
        .collect::<Vec<_>>();
    if !claimants.is_empty() {
        return Err(LedgerCliError::InventoryIgnoreClaimed {
            item: request.item.clone(),
            features: claimants,
        });
    }
    if ignores.ignores.iter().any(|i| i.item == request.item) {
        return Err(LedgerCliError::InventoryIgnoreDuplicate {
            item: request.item.clone(),
            path: root.join(INVENTORY_IGNORE_TARGET).display().to_string(),
        });
    }

    let entry = InventoryIgnore {
        item: request.item.clone(),
        reason: request.reason.clone(),
    };
    let fragment = toml::to_string(&InventoryIgnoreFile {
        ignore: vec![entry],
    })
    .map_err(|err| LedgerCliError::InventoryIgnoreWrite {
        path: root.join(INVENTORY_IGNORE_TARGET).display().to_string(),
        detail: err.to_string(),
    })?;
    let path = root.join(INVENTORY_IGNORE_TARGET);
    sink.append(&path, &fragment)
        .map_err(|detail| LedgerCliError::InventoryIgnoreWrite {
            path: path.display().to_string(),
            detail,
        })?;
    let remaining = ignores.ignores.len() + 1;
    tracing::debug!(item = %request.item, remaining, "inventory exemption filed");

    Ok(InventoryIgnoreOutcome {
        schema: INVENTORY_IGNORE_SCHEMA,
        item: request.item.clone(),
        reason: request.reason.clone(),
        target: path.display().to_string(),
        fragment,
        remaining,
    })
}

/// Renders the add outcome's text face: one summary line plus the
/// canonical appended block (the audit trail).
#[must_use]
pub fn format_inventory_ignore_text(outcome: &InventoryIgnoreOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "exempted {} -> {} ({} exemption(s) remain)",
        outcome.item, outcome.target, outcome.remaining,
    );
    text.push_str(&outcome.fragment);
    text
}

/// Renders the `glyim.ledger-inventory-ignore/1` JSON contract.
#[must_use]
pub fn format_inventory_ignore_json(outcome: &InventoryIgnoreOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-inventory-ignore/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------
// Engine — inventory unignore (removal face)
// ---------------------------------------------------------------------------

/// Retires one §3.5 exemption: validates the request, refuses to touch a
/// registry that does not load cleanly, and removes exactly the one
/// matching block through `rewriter`.
///
/// # Validation contract
///
/// - `item` must be usable (non-blank, no control characters);
/// - the committed registry must load with zero error-severity
///   diagnostics (the rewriter refuses to rewrite what it cannot fully
///   parse);
/// - the item key must be present (an unknown key writes nothing and
///   names a few registered keys for the help line).
///
/// The item becomes unmapped again the moment the rewrite lands — the
/// §3.5 gate is the loud follow-up that forces a feature claim before
/// `ledger check` passes.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`InventoryUnignoreBadItem`, `InventoryUnignoreLoad`,
/// `InventoryUnignoreUnknownItem`) and for a failed rewrite
/// (`InventoryUnignoreWrite`).
pub fn run_inventory_unignore(
    root: &Path,
    request: &InventoryUnignoreRequest,
    rewriter: &dyn IgnoreRewriter,
) -> Result<InventoryUnignoreOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "inventory_unignore",
        root = %root.display(),
        item = %request.item,
    );
    let _enter = span.enter();

    if request.item.trim().is_empty() || request.item.chars().any(char::is_control) {
        return Err(LedgerCliError::InventoryUnignoreBadItem {
            item: request.item.clone(),
        });
    }

    let ignores = load_inventory_ignore(root);
    let load_errors = ignores
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !load_errors.is_empty() {
        return Err(LedgerCliError::InventoryUnignoreLoad {
            detail: load_errors.join("; "),
        });
    }
    let Some(registered) = ignores
        .ignores
        .iter()
        .find(|entry| entry.item == request.item)
    else {
        let candidates = ignores
            .ignores
            .iter()
            .take(3)
            .map(|entry| entry.item.clone())
            .collect::<Vec<_>>();
        return Err(LedgerCliError::InventoryUnignoreUnknownItem {
            item: request.item.clone(),
            candidates,
        });
    };
    let reason = registered.reason.clone();

    let path = root.join(INVENTORY_IGNORE_TARGET);
    let removed = rewriter.remove(&path, &request.item).map_err(|detail| {
        LedgerCliError::InventoryUnignoreWrite {
            path: path.display().to_string(),
            detail,
        }
    })?;
    let Some(removed) = removed else {
        // The loaded registry carried the key but the document no longer
        // does: the registry drifted between the load pass and the
        // rewrite. Fail without a partial write.
        return Err(LedgerCliError::InventoryUnignoreUnknownItem {
            item: request.item.clone(),
            candidates: Vec::new(),
        });
    };
    let remaining = ignores.ignores.len().saturating_sub(1);
    tracing::debug!(item = %removed.item, remaining, "inventory exemption retired");

    Ok(InventoryUnignoreOutcome {
        schema: INVENTORY_UNIGNORE_SCHEMA,
        item: removed.item,
        reason,
        remaining,
        target: path.display().to_string(),
    })
}

/// Renders the removal outcome's text face: one summary line (the
/// removed entry's item, reason, and the remaining count).
#[must_use]
pub fn format_inventory_unignore_text(outcome: &InventoryUnignoreOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "unignored {} (was: {}) -> {} exemption(s) remain at {}",
        outcome.item, outcome.reason, outcome.remaining, outcome.target,
    );
    text
}

/// Renders the `glyim.ledger-inventory-unignore/1` JSON contract.
#[must_use]
pub fn format_inventory_unignore_json(outcome: &InventoryUnignoreOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-inventory-unignore/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- fixtures ------------------------------------------------------------

    /// Writes `contents` at `<root>/<rel>` (creating parent directories),
    /// the shared tempdir-seeding shape of the face tests.
    fn write_file(root: &Path, rel: &Path, contents: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir fixture parent");
        }
        std::fs::write(path, contents).expect("write fixture");
    }

    /// A minimal `#[unsafe(no_mangle)]` FFI source tree fixture (the
    /// `write_runtime_symbol` shape the `run_inventory` tests pin): the
    /// runtime FFI extractor turns `symbol` into the item key
    /// `runtime.ffi.<symbol>`.
    fn write_runtime_symbol(root: &Path, symbol: &str) {
        write_file(
            root,
            Path::new("crates/glyim-runtime/src/lib.rs"),
            &format!(
                "#[unsafe(no_mangle)]\npub extern \"C\" fn {symbol}(size: usize) -> usize {{ size }}\n"
            ),
        );
    }

    /// Minimal claim-bearing feature entry (the `feature_toml_with_claims`
    /// shape the inventory gate tests pin).
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

    /// Seeds the minimal registry layout `load_project_registry` requires
    /// (empty `ledger/features/` + `ledger/gaps/` directories): an
    /// exemption is filed *into* a registry, so a root without one is not
    /// a valid target.
    fn seed_registry(root: &Path) {
        std::fs::create_dir_all(root.join("ledger/features")).expect("mkdir features");
        std::fs::create_dir_all(root.join("ledger/gaps")).expect("mkdir gaps");
    }

    /// The canonical one-entry `[[ignore]]` fragment (what the sink is
    /// handed), asserted byte-for-byte by the happy-path tests.
    fn canonical_fragment(item: &str, reason: &str) -> String {
        toml::to_string(&InventoryIgnoreFile {
            ignore: vec![InventoryIgnore {
                item: item.to_string(),
                reason: reason.to_string(),
            }],
        })
        .expect("the canonical [[ignore]] fragment serializes")
    }

    const DEMO_ITEM: &str = "runtime.ffi.glyim_face_demo";
    const DEMO_REASON: &str = "per-variant coverage rows are the backend-parity matrix";

    // -- inventory ignore ----------------------------------------------------

    #[test]
    fn inventory_ignore_appends_the_canonical_block() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        seed_registry(root);
        let request = InventoryIgnoreRequest {
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
        };
        let outcome = run_inventory_ignore(root, &request, &FileIgnoreSink)
            .expect("a valid exemption must land");
        assert_eq!(outcome.schema, INVENTORY_IGNORE_SCHEMA);
        assert_eq!(outcome.item, DEMO_ITEM);
        assert_eq!(outcome.reason, DEMO_REASON);
        assert_eq!(outcome.remaining, 1);
        assert_eq!(outcome.fragment, canonical_fragment(DEMO_ITEM, DEMO_REASON));
        assert!(outcome.target.ends_with(INVENTORY_IGNORE_TARGET));
        let text = std::fs::read_to_string(root.join(INVENTORY_IGNORE_TARGET))
            .expect("the committed registry must exist after the append");
        assert_eq!(text, canonical_fragment(DEMO_ITEM, DEMO_REASON));
        // The §3.5 gate now passes for the item: no unmapped, no stale.
        let outcome = crate::run_inventory(root);
        assert!(
            outcome.is_ok(),
            "the filed exemption must satisfy the gate: {:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn inventory_ignore_appends_without_disturbing_existing_entries() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        // A second symbol in its own file (the helper writes `lib.rs`,
        // which would overwrite the first fixture).
        write_file(
            root,
            Path::new("crates/glyim-runtime/src/two.rs"),
            "#[unsafe(no_mangle)]\npub extern \"C\" fn glyim_face_demo_two(size: usize) -> usize { size }\n",
        );
        seed_registry(root);
        let request = InventoryIgnoreRequest {
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
        };
        run_inventory_ignore(root, &request, &FileIgnoreSink).expect("first exemption lands");
        let request = InventoryIgnoreRequest {
            item: "runtime.ffi.glyim_face_demo_two".to_string(),
            reason: "second exemption".to_string(),
        };
        let outcome =
            run_inventory_ignore(root, &request, &FileIgnoreSink).expect("second exemption lands");
        assert_eq!(outcome.remaining, 2);
        let text = std::fs::read_to_string(root.join(INVENTORY_IGNORE_TARGET))
            .expect("the committed registry must exist");
        assert_eq!(
            text,
            format!(
                "{}{}",
                canonical_fragment(DEMO_ITEM, DEMO_REASON),
                canonical_fragment("runtime.ffi.glyim_face_demo_two", "second exemption")
            )
        );
    }

    #[test]
    fn inventory_ignore_rejects_items_no_extractor_produces() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        seed_registry(root);
        let request = InventoryIgnoreRequest {
            item: "mir.StatementKind.Nope".to_string(),
            reason: DEMO_REASON.to_string(),
        };
        let err = run_inventory_ignore(root, &request, &FileIgnoreSink)
            .expect_err("a phantom item must be rejected (exact keys, never globs)");
        match &err {
            LedgerCliError::InventoryIgnoreUnknownItem { item, candidates } => {
                assert_eq!(item, "mir.StatementKind.Nope");
                assert!(
                    candidates.iter().any(|c| c == DEMO_ITEM),
                    "the extracted item must be named as a candidate: {candidates:?}"
                );
            }
            other => panic!("typed rejection expected, got: {other:?}"),
        }
        assert!(
            !root.join(INVENTORY_IGNORE_TARGET).exists(),
            "a rejected request must not write anything"
        );
    }

    #[test]
    fn inventory_ignore_rejects_claimed_items() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        write_file(
            root,
            Path::new("ledger/features/runtime/ffi.toml"),
            &feature_toml_with_claims("RUNTIME-FFI-DEMO", &[DEMO_ITEM]),
        );
        let request = InventoryIgnoreRequest {
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
        };
        let err = run_inventory_ignore(root, &request, &FileIgnoreSink)
            .expect_err("claiming the item and ignoring it cannot both hold");
        match &err {
            LedgerCliError::InventoryIgnoreClaimed { item, features } => {
                assert_eq!(item, DEMO_ITEM);
                assert_eq!(features, &["RUNTIME-FFI-DEMO".to_string()]);
            }
            other => panic!("typed rejection expected, got: {other:?}"),
        }
    }

    #[test]
    fn inventory_ignore_rejects_duplicates() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        seed_registry(root);
        let request = InventoryIgnoreRequest {
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
        };
        run_inventory_ignore(root, &request, &FileIgnoreSink).expect("first exemption lands");
        let before =
            std::fs::read_to_string(root.join(INVENTORY_IGNORE_TARGET)).expect("registry read");
        let err = run_inventory_ignore(root, &request, &FileIgnoreSink)
            .expect_err("a same-item re-file is a duplicate, never a silent overwrite");
        match &err {
            LedgerCliError::InventoryIgnoreDuplicate { item, path } => {
                assert_eq!(item, DEMO_ITEM);
                assert!(path.ends_with(INVENTORY_IGNORE_TARGET));
            }
            other => panic!("typed rejection expected, got: {other:?}"),
        }
        let after =
            std::fs::read_to_string(root.join(INVENTORY_IGNORE_TARGET)).expect("registry read");
        assert_eq!(
            before, after,
            "a rejected request must not rewrite the file"
        );
    }

    #[test]
    fn inventory_ignore_rejects_unusable_item_and_reason() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        let blank = InventoryIgnoreRequest {
            item: "   ".to_string(),
            reason: DEMO_REASON.to_string(),
        };
        assert!(matches!(
            run_inventory_ignore(root, &blank, &FileIgnoreSink),
            Err(LedgerCliError::InventoryIgnoreBadItem { .. })
        ));
        let control = InventoryIgnoreRequest {
            item: format!("{DEMO_ITEM}\u{7}"),
            reason: DEMO_REASON.to_string(),
        };
        assert!(matches!(
            run_inventory_ignore(root, &control, &FileIgnoreSink),
            Err(LedgerCliError::InventoryIgnoreBadItem { .. })
        ));
        let no_reason = InventoryIgnoreRequest {
            item: DEMO_ITEM.to_string(),
            reason: "  ".to_string(),
        };
        assert!(matches!(
            run_inventory_ignore(root, &no_reason, &FileIgnoreSink),
            Err(LedgerCliError::InventoryIgnoreMissingField { field, .. })
                if field == "--reason"
        ));
        assert!(
            !root.join(INVENTORY_IGNORE_TARGET).exists(),
            "rejected requests must not create the registry"
        );
    }

    #[test]
    fn inventory_ignore_refuses_a_registry_that_does_not_load() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        write_file(
            root,
            Path::new("ledger/features/runtime/broken.toml"),
            "not [[valid toml",
        );
        let request = InventoryIgnoreRequest {
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
        };
        assert!(matches!(
            run_inventory_ignore(root, &request, &FileIgnoreSink),
            Err(LedgerCliError::InventoryIgnoreLoad { .. })
        ));
        assert!(
            !root.join(INVENTORY_IGNORE_TARGET).exists(),
            "nothing is written when the tree does not load cleanly"
        );
    }

    /// A sink that always fails: the typed write-error path the file
    /// seam's IO failures surface through (the engine maps the seam's
    /// `Err` to `InventoryIgnoreWrite`, never a panic or a partial write).
    struct FailingSink;

    impl IgnoreSink for FailingSink {
        fn append(&self, _path: &Path, _fragment: &str) -> Result<(), String> {
            Err("denied".to_string())
        }
    }

    #[test]
    fn inventory_ignore_surfaces_sink_write_failures() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        seed_registry(root);
        let request = InventoryIgnoreRequest {
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
        };
        assert!(matches!(
            run_inventory_ignore(root, &request, &FailingSink),
            Err(LedgerCliError::InventoryIgnoreWrite { .. })
        ));
        assert!(
            !root.join(INVENTORY_IGNORE_TARGET).exists(),
            "a failed append must leave no partial registry behind"
        );
    }

    #[test]
    fn inventory_ignore_json_contract_round_trips_the_outcome() {
        let outcome = InventoryIgnoreOutcome {
            schema: INVENTORY_IGNORE_SCHEMA,
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
            target: "ledger/inventory-ignore.toml".to_string(),
            fragment: canonical_fragment(DEMO_ITEM, DEMO_REASON),
            remaining: 1,
        };
        let json = format_inventory_ignore_json(&outcome);
        assert!(json.contains("\"schema\": \"glyim.ledger-inventory-ignore/1\""));
        assert!(json.contains("\"item\": \"runtime.ffi.glyim_face_demo\""));
        let text = format_inventory_ignore_text(&outcome);
        assert!(text.contains("exempted runtime.ffi.glyim_face_demo"));
        assert!(text.contains(&outcome.fragment));
    }

    // -- inventory unignore --------------------------------------------------

    #[test]
    fn inventory_unignore_removes_one_entry_and_rewrites_canonically() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_runtime_symbol(root, "glyim_face_demo");
        write_file(
            root,
            Path::new(INVENTORY_IGNORE_TARGET),
            &format!(
                "{}{}",
                canonical_fragment(DEMO_ITEM, DEMO_REASON),
                canonical_fragment("runtime.ffi.glyim_face_demo_two", "second exemption reason")
            ),
        );
        let request = InventoryUnignoreRequest {
            item: DEMO_ITEM.to_string(),
        };
        let outcome = run_inventory_unignore(root, &request, &FileIgnoreRewriter)
            .expect("a registered exemption must retire");
        assert_eq!(outcome.schema, INVENTORY_UNIGNORE_SCHEMA);
        assert_eq!(outcome.item, DEMO_ITEM);
        assert_eq!(outcome.reason, DEMO_REASON);
        assert_eq!(outcome.remaining, 1);
        assert!(outcome.target.ends_with(INVENTORY_IGNORE_TARGET));
        let text = std::fs::read_to_string(root.join(INVENTORY_IGNORE_TARGET))
            .expect("the rewritten registry must exist");
        assert_eq!(
            text,
            canonical_fragment("runtime.ffi.glyim_face_demo_two", "second exemption reason")
        );
    }

    #[test]
    fn inventory_unignore_rejects_items_the_registry_does_not_carry() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_file(
            root,
            Path::new(INVENTORY_IGNORE_TARGET),
            &canonical_fragment(DEMO_ITEM, DEMO_REASON),
        );
        let request = InventoryUnignoreRequest {
            item: "mir.StatementKind.Nope".to_string(),
        };
        let err = run_inventory_unignore(root, &request, &FileIgnoreRewriter)
            .expect_err("an unregistered item is a rejection, not a silent no-op");
        match &err {
            LedgerCliError::InventoryUnignoreUnknownItem { item, candidates } => {
                assert_eq!(item, "mir.StatementKind.Nope");
                assert_eq!(candidates, &[DEMO_ITEM.to_string()]);
            }
            other => panic!("typed rejection expected, got: {other:?}"),
        }
        let text = std::fs::read_to_string(root.join(INVENTORY_IGNORE_TARGET))
            .expect("the committed registry must be untouched");
        assert_eq!(text, canonical_fragment(DEMO_ITEM, DEMO_REASON));
    }

    #[test]
    fn inventory_unignore_on_a_missing_registry_rejects() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        let request = InventoryUnignoreRequest {
            item: DEMO_ITEM.to_string(),
        };
        assert!(matches!(
            run_inventory_unignore(root, &request, &FileIgnoreRewriter),
            Err(LedgerCliError::InventoryUnignoreUnknownItem { candidates, .. })
                if candidates.is_empty()
        ));
    }

    #[test]
    fn inventory_unignore_rejects_unusable_items() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        let blank = InventoryUnignoreRequest {
            item: String::new(),
        };
        assert!(matches!(
            run_inventory_unignore(root, &blank, &FileIgnoreRewriter),
            Err(LedgerCliError::InventoryUnignoreBadItem { .. })
        ));
    }

    #[test]
    fn inventory_unignore_refuses_a_registry_that_does_not_parse() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        write_file(
            root,
            Path::new(INVENTORY_IGNORE_TARGET),
            "[bogus]\nfield = 1\n",
        );
        let request = InventoryUnignoreRequest {
            item: DEMO_ITEM.to_string(),
        };
        assert!(matches!(
            run_inventory_unignore(root, &request, &FileIgnoreRewriter),
            Err(LedgerCliError::InventoryUnignoreLoad { .. })
        ));
    }

    #[test]
    fn inventory_unignore_json_contract_round_trips_the_outcome() {
        let outcome = InventoryUnignoreOutcome {
            schema: INVENTORY_UNIGNORE_SCHEMA,
            item: DEMO_ITEM.to_string(),
            reason: DEMO_REASON.to_string(),
            remaining: 1,
            target: "ledger/inventory-ignore.toml".to_string(),
        };
        let json = format_inventory_unignore_json(&outcome);
        assert!(json.contains("\"schema\": \"glyim.ledger-inventory-unignore/1\""));
        let text = format_inventory_unignore_text(&outcome);
        assert!(text.contains("unignored runtime.ffi.glyim_face_demo"));
        assert!(text.contains(DEMO_REASON));
    }

    // -- shared seams --------------------------------------------------------

    #[test]
    fn the_in_memory_seams_record_every_write() {
        let sink = InMemoryIgnoreSink::default();
        let fragment = canonical_fragment(DEMO_ITEM, DEMO_REASON);
        let path = Path::new("/tmp/registry/inventory-ignore.toml");
        sink.append(path, &fragment).expect("in-memory append");
        assert_eq!(sink.written.borrow().len(), 1);
        assert_eq!(sink.written.borrow()[0].1, fragment);

        let rewriter = InMemoryIgnoreRewriter {
            doc: std::cell::RefCell::new(vec![InventoryIgnore {
                item: DEMO_ITEM.to_string(),
                reason: DEMO_REASON.to_string(),
            }]),
            removed: std::cell::RefCell::new(Vec::new()),
        };
        let removed = rewriter.remove(path, DEMO_ITEM).expect("in-memory removal");
        assert_eq!(removed.map(|entry| entry.item), Some(DEMO_ITEM.to_string()));
        assert_eq!(rewriter.doc.borrow().len(), 0);
        assert_eq!(rewriter.removed.borrow().len(), 1);
        let again = rewriter.remove(path, DEMO_ITEM).expect("in-memory removal");
        assert!(again.is_none(), "a second removal must report absence");
    }

    // -- parse surface -------------------------------------------------------

    #[test]
    fn parse_args_routes_the_two_inventory_write_faces() {
        let cmd = crate::parse_args(
            [
                "inventory",
                "ignore",
                "--item",
                DEMO_ITEM,
                "--reason",
                DEMO_REASON,
                "--format",
                "json",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter(),
        )
        .expect("parse");
        assert_eq!(
            cmd,
            crate::Command::InventoryIgnore {
                root: PathBuf::from("."),
                item: DEMO_ITEM.to_string(),
                reason: DEMO_REASON.to_string(),
                format: crate::IgnoresFormat::Json,
            }
        );
        let cmd = crate::parse_args(
            ["inventory", "unignore", "/tmp/reg", "--item", DEMO_ITEM]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .into_iter(),
        )
        .expect("parse");
        assert_eq!(
            cmd,
            crate::Command::InventoryUnignore {
                root: PathBuf::from("/tmp/reg"),
                item: DEMO_ITEM.to_string(),
                format: crate::IgnoresFormat::Text,
            }
        );
    }

    #[test]
    fn parse_args_still_routes_the_inventory_read_face() {
        let cmd = crate::parse_args(["inventory", "/tmp/src"].iter().map(|s| s.to_string()))
            .expect("parse");
        assert_eq!(
            cmd,
            crate::Command::Inventory {
                root: PathBuf::from("/tmp/src")
            }
        );
    }

    #[test]
    fn parse_args_rejects_the_new_faces_without_required_flags() {
        let err = crate::parse_args(["inventory", "ignore"].iter().map(|s| s.to_string()))
            .expect_err("missing flags must fail");
        assert!(matches!(
            err,
            LedgerCliError::InventoryIgnoreMissingField { field, .. } if field == "--item"
        ));
    }
}
