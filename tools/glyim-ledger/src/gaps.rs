//! Gap-lifecycle write faces (`gaps witness`, `gaps retire`).
//!
//! The gap registry (`ledger/gaps/known/*.toml`, parsed by
//! `ledger-schema`) tracks every open gap's witness status, and the
//! `render-gaps` face generates `KNOWN_GAPS.md` from it with a `--check`
//! drift gate — but until now there was no sanctioned way to *change*
//! that status: recording a witness or retiring a closed gap meant
//! hand-editing the curated TOML files and remembering to regenerate the
//! rendered document. These faces are the write side of that lifecycle,
//! mirroring the `waivers grant/revoke` shape: a validated request, a
//! source-of-truth load pass that refuses anything but a clean registry,
//! a rewrite seam the engine consumes (file-backed in the CLI, in-memory
//! in tests and walkthroughs), typed [`LedgerCliError`] rejections that
//! never leave a partial write behind, and text/JSON formatters.
//!
//! The rewriters are deliberately *surgical* rather than canonical: the
//! committed gap files carry curated header prose (the M1.1/M1.7 seed
//! comments) that a full re-serialization would discard, so the rewriters
//! edit only the matched `[[gap]]` block's lines and leave every other
//! byte untouched — the opposite tradeoff to the `waivers` faces, where
//! the document is machine-written and canonicalization is the contract.
//! The generated `KNOWN_GAPS.md` document is refreshed by the dispatch
//! layer after a successful rewrite (the same `run_render_gaps` pipeline
//! the `--check` gate byte-compares against), so the face never leaves
//! the committed document silently drifted from the registry.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ledger_schema::{GapStatus, Severity};

use crate::LedgerCliError;

// -- witness face -------------------------------------------------------------

/// Schema tag of the `gaps witness` JSON contract.
pub const GAPS_WITNESS_SCHEMA: &str = "glyim.ledger-gaps-witness/1";

/// Output format of the `gaps` write faces (`witness`, `retire`); the
/// plain `gaps` read face stays text-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapsFormat {
    /// The human-readable one-liner ([`format_gaps_witness_text`] and
    /// friends).
    Text,
    /// The face's versioned JSON contract
    /// ([`format_gaps_witness_json`] and friends).
    Json,
}

impl GapsFormat {
    /// Canonical registry spelling (the `--format` value).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GapsFormat::Text => "text",
            GapsFormat::Json => "json",
        }
    }
}

/// The `gaps witness` request (the CLI flags, validated by
/// [`run_gaps_witness`]): attach the witness fixture that reproduces one
/// unwitnessed gap — the sanctioned close of the witness debt the plan
/// §3.3 report prints and the `next` queue ranks, which until this face
/// could only be discharged by hand-editing the curated registry files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapsWitnessRequest {
    /// The exact gap ID (`GAP-NNNN`) to witness.
    pub id: String,
    /// The witness fixture path, relative to the project root — the same
    /// repo-relative spelling the registry entries use (`crates/...`,
    /// `fixtures/...`).
    pub witness: String,
}

/// Where a `[[gap]]` block's witness lines are edited. The rewrite seam
/// mirrors [`crate::WaiverSink`] on the write side: the engine consumes
/// the trait, so tests drive an in-memory document and the CLI wires the
/// file-backed implementation.
pub trait GapWitnessRewriter {
    /// Flips the `[[gap]]` block carrying the exact `id` from
    /// `status = "unwitnessed"` to `status = "witnessed"` and appends the
    /// `witness = "..."` line, leaving every other byte of the document
    /// (the curated header prose included) untouched. Returns whether the
    /// edit landed (`false` = no unwitnessed block carried the ID: an
    /// unknown ID, or one whose status drifted after the engine's load
    /// pass); the error detail is the IO or shape failure message (the
    /// CLI turns it into a typed write error).
    fn witness(&self, path: &Path, id: &str, witness: &str) -> Result<bool, String>;
}

/// Edits the registry file on disk exactly where the loaded registry
/// says the gap lives: read, edit the matched block, replace the file.
/// A document whose matched block is not an unwitnessed, witness-less
/// block is refused (`Ok(false)` / `Err`), never silently rewritten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileGapWitnessRewriter;

impl GapWitnessRewriter for FileGapWitnessRewriter {
    fn witness(&self, path: &Path, id: &str, witness: &str) -> Result<bool, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let Some(edited) = edit_witness_block(&text, id, witness) else {
            return Ok(false);
        };
        std::fs::write(path, edited)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(true)
    }
}

/// Operates on owned in-memory documents (tests and programmatic
/// consumers); the in-memory twin of [`FileGapWitnessRewriter`]. Every
/// successful edit is recorded in [`Self::edits`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryGapWitnessRewriter {
    /// The documents the rewriter operates on, keyed by path.
    pub doc: std::cell::RefCell<std::collections::HashMap<PathBuf, String>>,
    /// Every `(path, gap ID, witness path)` edit performed, in order.
    pub edits: std::cell::RefCell<Vec<(PathBuf, String, String)>>,
}

impl GapWitnessRewriter for InMemoryGapWitnessRewriter {
    fn witness(&self, path: &Path, id: &str, witness: &str) -> Result<bool, String> {
        let mut doc = self.doc.borrow_mut();
        let Some(text) = doc.get_mut(path) else {
            return Ok(false);
        };
        let Some(edited) = edit_witness_block(text, id, witness) else {
            return Ok(false);
        };
        *text = edited;
        self.edits
            .borrow_mut()
            .push((path.to_path_buf(), id.to_string(), witness.to_string()));
        Ok(true)
    }
}

/// The bare TOML key a block line declares, or `None` for blank lines,
/// comments, and block/array lines (bare keys never carry spaces,
/// `#`, or `[`).
fn block_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || key.contains([' ', '#', '[']) {
        return None;
    }
    Some(key)
}

/// The quoted string value a block line carries (`id = "GAP-0001"`),
/// or `None` when the line is not a quoted string line.
fn block_value(line: &str) -> Option<&str> {
    let (_, value) = line.split_once('=')?;
    let value = value.trim();
    let value = value.strip_prefix('"')?;
    let value = value.strip_suffix('"')?;
    Some(value)
}

/// The surgical edit behind both rewriters: find the `[[gap]]` block
/// whose `id` line carries the exact ID, flip its `status` value from
/// `"unwitnessed"` to `"witnessed"`, and append the aligned
/// `witness = "..."` line right after the status line. Every other line
/// (the curated header prose, the sibling blocks) passes through
/// byte-for-byte. Returns `None` when no unwitnessed, witness-less block
/// carries the ID — the drift signal the engine maps to a typed error.
///
/// The `witness` line's `=` column is padded to the `status` line's
/// `=` column (minimum one space), which reproduces the committed
/// registry style (`witness   = "..."` under `status    = "..."`).
fn edit_witness_block(text: &str, id: &str, witness: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut index = 0;
    let mut edited = false;
    while index < lines.len() {
        let line = lines[index];
        if line.trim() != "[[gap]]" {
            out.push(line.to_string());
            index += 1;
            continue;
        }
        // The block runs to the next `[[gap]]` header (or EOF).
        let mut end = index + 1;
        while end < lines.len() && lines[end].trim() != "[[gap]]" {
            end += 1;
        }
        let block = &lines[index..end];
        let matches_id = block
            .iter()
            .any(|entry| block_key(entry) == Some("id") && block_value(entry) == Some(id));
        if matches_id {
            let status_offset = block
                .iter()
                .position(|entry| block_key(entry) == Some("status"))?;
            let status_line = block[status_offset];
            if block_value(status_line) != Some("unwitnessed") {
                // Already witnessed (or a malformed status): the engine's
                // load pass rejects the former and the registry schema
                // rejects the latter — either way, no silent rewrite.
                return None;
            }
            if block
                .iter()
                .any(|entry| block_key(entry) == Some("witness"))
            {
                return None;
            }
            let (key_part, _) = status_line.split_once('=')?;
            for existing in &block[..status_offset] {
                out.push((*existing).to_string());
            }
            out.push(format!("{key_part}= \"witnessed\""));
            let pad = key_part.len().saturating_sub("witness".len()).max(1);
            out.push(format!("witness{}= \"{witness}\"", " ".repeat(pad)));
            for existing in &block[status_offset + 1..] {
                out.push((*existing).to_string());
            }
            edited = true;
        } else {
            for existing in block {
                out.push((*existing).to_string());
            }
        }
        index = end;
    }
    if !edited {
        return None;
    }
    let mut result = out.join("\n");
    if text.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

/// A witness path the registry accepts: a relative, backslash-free,
/// control-character-free path whose components never escape the project
/// root (`..`). The committed entries use plain repo-relative spellings
/// (`crates/...`, `fixtures/...`), and the plan's witness contract is
/// repo-relative by construction.
fn usable_witness_path(witness: &str) -> bool {
    !witness.trim().is_empty()
        && !witness.chars().any(char::is_control)
        && !witness.contains('\\')
        && !Path::new(witness).is_absolute()
        && Path::new(witness).components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

/// Result of a `gaps witness` run (also the
/// `glyim.ledger-gaps-witness/1` JSON face via
/// [`format_gaps_witness_json`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GapsWitnessOutcome {
    /// Schema tag, always [`GAPS_WITNESS_SCHEMA`].
    pub schema: &'static str,
    /// The witnessed gap's ID.
    pub id: String,
    /// The gap's registry title (echoed for the report line).
    pub title: String,
    /// The gap's class (`a` diagnosed, `b` silent, `c` deferred).
    pub class: String,
    /// The witness fixture path recorded into the registry.
    pub witness: String,
    /// The registry file the `[[gap]]` block was edited in.
    pub target: String,
    /// The generated document the dispatch layer refreshes after the
    /// rewrite (`<ROOT>/KNOWN_GAPS.md`) — the face never leaves the
    /// committed render silently drifted from the registry.
    pub doc_target: String,
}

/// Witnesses one gap: validates the request, refuses to touch anything
/// unless the registry loads cleanly, and edits exactly the one matching
/// `[[gap]]` block through `rewriter`.
///
/// # Validation contract
///
/// - `id` must be usable (non-blank, no control characters);
/// - `witness` must be a relative, backslash-free path that never
///   escapes the project root (`..`), and must name an existing file
///   under ROOT (a phantom witness would be silent debt — the plan
///   §3.3 rule is "allowed, but never silent");
/// - the registry must load with zero error-severity diagnostics (a
///   registry that does not parse is never rewritten);
/// - the gap must be present and unwitnessed (an already-witnessed gap
///   is never silently re-witnessed — the grant-face duplicate rule).
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`GapsWitnessBadId`, `GapsWitnessBadWitness`, `GapsWitnessRegistry`,
/// `GapsWitnessUnknownId`, `GapsWitnessAlreadyWitnessed`,
/// `GapsWitnessMissingFixture`) and for a failed rewrite
/// (`GapsWitnessWrite`).
pub fn run_gaps_witness(
    root: &Path,
    request: &GapsWitnessRequest,
    rewriter: &dyn GapWitnessRewriter,
) -> Result<GapsWitnessOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "gaps_witness",
        root = %root.display(),
        id = %request.id,
    );
    let _enter = span.enter();

    if request.id.trim().is_empty() || request.id.chars().any(char::is_control) {
        return Err(LedgerCliError::GapsWitnessBadId {
            id: request.id.clone(),
        });
    }
    if !usable_witness_path(&request.witness) {
        return Err(LedgerCliError::GapsWitnessBadWitness {
            witness: request.witness.clone(),
        });
    }

    // The registry is the source of truth for both the gap lookup and
    // the file the block lives in; a load that does not parse cleanly is
    // refused before anything is written (the revoke-face load rule).
    let loaded = ledger_schema::load_project_registry(root);
    if loaded
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        let detail = loaded
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LedgerCliError::GapsWitnessRegistry { detail });
    }
    let Some(sourced) = loaded
        .registry
        .gaps()
        .iter()
        .find(|sourced| sourced.entry.id == request.id)
    else {
        let candidates = loaded
            .registry
            .gaps()
            .iter()
            .map(|sourced| sourced.entry.id.clone())
            .take(3)
            .collect::<Vec<_>>();
        return Err(LedgerCliError::GapsWitnessUnknownId {
            id: request.id.clone(),
            candidates,
        });
    };
    if sourced.entry.status == GapStatus::Witnessed || sourced.entry.witness.is_some() {
        return Err(LedgerCliError::GapsWitnessAlreadyWitnessed {
            id: request.id.clone(),
            witness: sourced
                .entry
                .witness
                .clone()
                .unwrap_or_else(|| sourced.entry.status.as_str().to_string()),
            path: sourced.path.display().to_string(),
        });
    }

    // A witness pointing at nothing would trade visible debt for silent
    // debt: the fixture must already exist under the project root.
    let fixture = root.join(&request.witness);
    if !fixture.is_file() {
        return Err(LedgerCliError::GapsWitnessMissingFixture {
            witness: request.witness.clone(),
            path: fixture.display().to_string(),
        });
    }

    let target = sourced.path.clone();
    let edited = rewriter
        .witness(&target, &request.id, &request.witness)
        .map_err(|detail| LedgerCliError::GapsWitnessWrite {
            path: target.display().to_string(),
            detail,
        })?;
    if !edited {
        // The loaded registry carried the gap but the document no longer
        // matches the unwitnessed shape: the registry drifted between the
        // load pass and the rewrite. Fail without a partial write.
        return Err(LedgerCliError::GapsWitnessUnknownId {
            id: request.id.clone(),
            candidates: Vec::new(),
        });
    }
    tracing::debug!(
        id = %request.id,
        witness = %request.witness,
        "gap witnessed"
    );

    Ok(GapsWitnessOutcome {
        schema: GAPS_WITNESS_SCHEMA,
        id: request.id.clone(),
        title: sourced.entry.title.clone(),
        class: sourced.entry.class.as_str().to_string(),
        witness: request.witness.clone(),
        target: target.display().to_string(),
        doc_target: root
            .join(crate::render::KNOWN_GAPS_MD_TARGET)
            .display()
            .to_string(),
    })
}

/// Renders the witness outcome's text face: one summary line (the
/// witnessed gap, its fixture, and both touched paths).
#[must_use]
pub fn format_gaps_witness_text(outcome: &GapsWitnessOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "witnessed {} ({}) with {} -> {} rewritten, refresh {}",
        outcome.id, outcome.class, outcome.witness, outcome.target, outcome.doc_target,
    );
    text
}

/// Renders the `glyim.ledger-gaps-witness/1` JSON contract.
#[must_use]
pub fn format_gaps_witness_json(outcome: &GapsWitnessOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-gaps-witness/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

// -- retire face ---------------------------------------------------------------

/// Schema tag of the `gaps retire` JSON contract.
pub const GAPS_RETIRE_SCHEMA: &str = "glyim.ledger-gaps-retire/1";

/// The `gaps retire` request (the CLI flags, validated by
/// [`run_gaps_retire`]): remove one gap's `[[gap]]` block from the
/// registry entirely — the closure half of the gap lifecycle. The
/// rendered `KNOWN_GAPS.md` states that "closed entries retire to the
/// git history of this file and of the registry"; until this face the
/// removal itself had no sanctioned command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapsRetireRequest {
    /// The exact gap ID (`GAP-NNNN`) to remove.
    pub id: String,
}

/// Where a retired `[[gap]]` block is removed from. The rewrite seam
/// mirrors [`GapWitnessRewriter`] on the write side: the engine consumes
/// the trait, so tests drive an in-memory document and the CLI wires the
/// file-backed implementation.
pub trait GapRetirer {
    /// Removes the `[[gap]]` block carrying the exact `id` from the
    /// document at `path`, leaving every other byte (the curated header
    /// prose, the sibling blocks) untouched. Returns the removed block's
    /// `title`, or `None` when the document no longer carries the block;
    /// the error detail is the IO or shape failure message (the CLI turns
    /// it into a typed write error).
    fn retire(&self, path: &Path, id: &str) -> Result<Option<String>, String>;
}

/// Removes the block from the registry file on disk exactly where the
/// loaded registry says the gap lives: read, drop the one matching block,
/// replace the file. A document without the block is refused
/// (`Ok(None)`), never silently rewritten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileGapRetirer;

impl GapRetirer for FileGapRetirer {
    fn retire(&self, path: &Path, id: &str) -> Result<Option<String>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let Some((edited, title)) = remove_gap_block(&text, id) else {
            return Ok(None);
        };
        std::fs::write(path, edited)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(Some(title))
    }
}

/// Operates on owned in-memory documents (tests and programmatic
/// consumers); the in-memory twin of [`FileGapRetirer`]. Every successful
/// removal is recorded in [`Self::removed`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryGapRetirer {
    /// The documents the retirer operates on, keyed by path.
    pub doc: std::cell::RefCell<std::collections::HashMap<PathBuf, String>>,
    /// Every `(path, removed ID)` performed, in order.
    pub removed: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl GapRetirer for InMemoryGapRetirer {
    fn retire(&self, path: &Path, id: &str) -> Result<Option<String>, String> {
        let mut doc = self.doc.borrow_mut();
        let Some(text) = doc.get_mut(path) else {
            return Ok(None);
        };
        let Some((edited, title)) = remove_gap_block(text, id) else {
            return Ok(None);
        };
        *text = edited;
        self.removed
            .borrow_mut()
            .push((path.to_path_buf(), id.to_string()));
        Ok(Some(title))
    }
}

/// The surgical removal behind both retirers: find the `[[gap]]` block
/// whose `id` line carries the exact ID and drop its lines, returning the
/// edited document and the removed block's `title`. Every other line
/// (the curated header prose, the sibling blocks) passes through
/// byte-for-byte. Returns `None` when no block carries the ID — the
/// drift signal the engine maps to a typed error.
///
/// When the removed block is followed by a blank line and then another
/// `[[gap]]` header, that one blank line goes with it, so the remaining
/// entries stay separated without a leading or doubled blank.
fn remove_gap_block(text: &str, id: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0;
    let mut removed_title: Option<String> = None;
    while index < lines.len() {
        let line = lines[index];
        if line.trim() != "[[gap]]" {
            out.push(line.to_string());
            index += 1;
            continue;
        }
        // The block runs to the next `[[gap]]` header (or EOF).
        let mut end = index + 1;
        while end < lines.len() && lines[end].trim() != "[[gap]]" {
            end += 1;
        }
        let block = &lines[index..end];
        let matches_id = block
            .iter()
            .any(|entry| block_key(entry) == Some("id") && block_value(entry) == Some(id));
        if matches_id {
            let title = block
                .iter()
                .find(|entry| block_key(entry) == Some("title"))
                .and_then(|entry| block_value(entry))
                .unwrap_or_default()
                .to_string();
            removed_title = Some(title);
            index = end;
            // A blank line between the removed block and the next header
            // is the removed block's separator, not the next block's.
            if index < lines.len()
                && lines[index].trim().is_empty()
                && lines
                    .get(index + 1)
                    .is_some_and(|next| next.trim() == "[[gap]]")
            {
                index += 1;
            }
        } else {
            for existing in block {
                out.push((*existing).to_string());
            }
            index = end;
        }
    }
    let title = removed_title?;
    let mut result = out.join("\n");
    if text.ends_with('\n') && !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    Some((result, title))
}

/// Result of a `gaps retire` run (also the
/// `glyim.ledger-gaps-retire/1` JSON face via
/// [`format_gaps_retire_json`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GapsRetireOutcome {
    /// Schema tag, always [`GAPS_RETIRE_SCHEMA`].
    pub schema: &'static str,
    /// The retired gap's ID.
    pub id: String,
    /// The removed block's registry title.
    pub title: String,
    /// The registry file the `[[gap]]` block was removed from.
    pub target: String,
    /// Open gaps remaining in the registry after the removal.
    pub remaining: usize,
}

/// Retires one gap: validates the request, refuses to touch anything
/// unless the registry loads cleanly, and removes exactly the one
/// matching `[[gap]]` block through `retirer`.
///
/// # Validation contract
///
/// - `id` must be usable (non-blank, no control characters);
/// - the registry must load with zero error-severity diagnostics (a
///   registry that does not parse is never rewritten — the revoke-face
///   load rule);
/// - the gap must be present (an unknown ID writes nothing and names a
///   few existing candidates for the help line). Both witnessed and
///   unwitnessed gaps retire: the closure change retires the witness
///   with the gap it proved.
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`GapsRetireBadId`, `GapsRetireUnknownId`, `GapsRetireLoad`) and for
/// a failed removal (`GapsRetireWrite`).
pub fn run_gaps_retire(
    root: &Path,
    request: &GapsRetireRequest,
    retirer: &dyn GapRetirer,
) -> Result<GapsRetireOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "gaps_retire",
        root = %root.display(),
        id = %request.id,
    );
    let _enter = span.enter();

    if request.id.trim().is_empty() || request.id.chars().any(char::is_control) {
        return Err(LedgerCliError::GapsRetireBadId {
            id: request.id.clone(),
        });
    }

    // The registry is the source of truth for both the gap lookup and
    // the file the block lives in; a load that does not parse cleanly is
    // refused before anything is written (the revoke-face load rule).
    let loaded = ledger_schema::load_project_registry(root);
    if loaded
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        let detail = loaded
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(LedgerCliError::GapsRetireLoad { detail });
    }
    let Some(sourced) = loaded
        .registry
        .gaps()
        .iter()
        .find(|sourced| sourced.entry.id == request.id)
    else {
        let candidates = loaded
            .registry
            .gaps()
            .iter()
            .map(|sourced| sourced.entry.id.clone())
            .take(3)
            .collect::<Vec<_>>();
        return Err(LedgerCliError::GapsRetireUnknownId {
            id: request.id.clone(),
            candidates,
        });
    };

    let target = sourced.path.clone();
    let removed =
        retirer
            .retire(&target, &request.id)
            .map_err(|detail| LedgerCliError::GapsRetireWrite {
                path: target.display().to_string(),
                detail,
            })?;
    if removed.is_none() {
        // The loaded registry carried the gap but the document no longer
        // does: the registry drifted between the load pass and the
        // rewrite. Fail without a partial write.
        return Err(LedgerCliError::GapsRetireUnknownId {
            id: request.id.clone(),
            candidates: Vec::new(),
        });
    }
    tracing::debug!(id = %request.id, "gap retired");

    Ok(GapsRetireOutcome {
        schema: GAPS_RETIRE_SCHEMA,
        id: request.id.clone(),
        title: sourced.entry.title.clone(),
        target: target.display().to_string(),
        remaining: loaded.registry.gap_count().saturating_sub(1),
    })
}

/// Renders the retire outcome's text face: one summary line (the retired
/// gap, its title, and the remaining count).
#[must_use]
pub fn format_gaps_retire_text(outcome: &GapsRetireOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "retired {} ({}) -> {} gap(s) remain at {}",
        outcome.id, outcome.title, outcome.remaining, outcome.target,
    );
    text
}

/// Renders the `glyim.ledger-gaps-retire/1` JSON contract.
#[must_use]
pub fn format_gaps_retire_json(outcome: &GapsRetireOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-gaps-retire/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    //! `gaps witness` contract: the rewriter edits exactly the matched
    //! `[[gap]]` block (curated comment prose preserved byte-for-byte),
    //! the engine validates the request against the loaded registry and
    //! the fixture tree and writes nothing on any rejection, the formatters
    //! render twice byte-identically (text names the witnessed gap; JSON
    //! parses back into the `glyim.ledger-gaps-witness/1` contract), and
    //! the CLI parses the face with the shared write-face shape.

    use std::path::{Path, PathBuf};

    use ledger_schema::GapStatus;

    use super::{
        FileGapRetirer, FileGapWitnessRewriter, GapRetirer, GapWitnessRewriter, GapsRetireRequest,
        GapsWitnessRequest, InMemoryGapRetirer, run_gaps_retire, run_gaps_witness,
    };
    use crate::{LedgerCliError, parse_args, usage};

    // -- fixtures ------------------------------------------------------------

    /// A curated gap-registry file in the committed `ledger/gaps/known/`
    /// style: header prose the rewriter must preserve, one witnessed
    /// entry, one unwitnessed entry.
    const CURATED_SEED: &str = "# Curated header prose (M1.1 seed) the rewriter must preserve\n\
         # byte-for-byte: witness debt stays honest — GAP-0002 is\n\
         # unwitnessed until a fixture reproduces it.\n\
         \n\
         [[gap]]\n\
         id        = \"GAP-0001\"\n\
         class     = \"a\"\n\
         title     = \"await in a loop body is diagnosed (T0060)\"\n\
         affects   = [\"LANG-DEMO-AWAIT\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status    = \"witnessed\"\n\
         witness   = \"fixtures/await-loop-known-gap.g\"\n\
         \n\
         [[gap]]\n\
         id        = \"GAP-0002\"\n\
         class     = \"b\"\n\
         title     = \"no lowering site sets cleanup\"\n\
         affects   = [\"LANG-DEMO-CLEANUP\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status    = \"unwitnessed\"\n";

    /// The exact expected bytes after witnessing GAP-0002 with the
    /// `fixtures/cleanup-known-gap.g` fixture: only the matched block's
    /// `status` value changed and the aligned `witness` line appended.
    const CURATED_WITNESSED: &str = "# Curated header prose (M1.1 seed) the rewriter must preserve\n\
         # byte-for-byte: witness debt stays honest — GAP-0002 is\n\
         # unwitnessed until a fixture reproduces it.\n\
         \n\
         [[gap]]\n\
         id        = \"GAP-0001\"\n\
         class     = \"a\"\n\
         title     = \"await in a loop body is diagnosed (T0060)\"\n\
         affects   = [\"LANG-DEMO-AWAIT\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status    = \"witnessed\"\n\
         witness   = \"fixtures/await-loop-known-gap.g\"\n\
         \n\
         [[gap]]\n\
         id        = \"GAP-0002\"\n\
         class     = \"b\"\n\
         title     = \"no lowering site sets cleanup\"\n\
         affects   = [\"LANG-DEMO-CLEANUP\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status    = \"witnessed\"\n\
         witness   = \"fixtures/cleanup-known-gap.g\"\n";

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

    /// A tempdir project whose registry mirrors the committed seed: two
    /// features, the spec they resolve against, and the curated gap file
    /// with both fixture files present. Returns `(root, gap_file_path)`.
    fn seeded_root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        write_file(&root, "docs/specs/v0.1.0.md", "# spec fixture\n");
        write_file(
            &root,
            "ledger/features/lang/a.toml",
            &feature_toml("LANG-DEMO-AWAIT"),
        );
        write_file(
            &root,
            "ledger/features/lang/b.toml",
            &feature_toml("LANG-DEMO-CLEANUP"),
        );
        write_file(&root, "ledger/gaps/known/0001-seed.toml", CURATED_SEED);
        write_file(
            &root,
            "fixtures/await-loop-known-gap.g",
            "//@ known-gap: GAP-0001\nfn main() {}\n",
        );
        write_file(
            &root,
            "fixtures/cleanup-known-gap.g",
            "//@ known-gap: GAP-0002\nfn main() {}\n",
        );
        let gap_file = root.join("ledger/gaps/known/0001-seed.toml");
        (dir, gap_file)
    }

    fn request(id: &str, witness: &str) -> GapsWitnessRequest {
        GapsWitnessRequest {
            id: id.to_string(),
            witness: witness.to_string(),
        }
    }

    // -- rewrite seam ----------------------------------------------------------

    #[test]
    fn file_rewriter_edits_only_the_matched_block_bytes() {
        let (dir, gap_file) = seeded_root();
        let _ = dir;
        let edited = FileGapWitnessRewriter
            .witness(&gap_file, "GAP-0002", "fixtures/cleanup-known-gap.g")
            .expect("the rewrite succeeds");
        assert!(edited, "the unwitnessed block was edited");
        let on_disk = std::fs::read_to_string(&gap_file).expect("read back");
        assert_eq!(on_disk, CURATED_WITNESSED, "only the block changed");
    }

    #[test]
    fn file_rewriter_returns_false_for_an_unknown_id_and_writes_nothing() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let edited = FileGapWitnessRewriter
            .witness(&gap_file, "GAP-9999", "fixtures/none.g")
            .expect("an unknown ID is not an IO error");
        assert!(!edited, "no block carried the ID");
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
        let _ = dir;
    }

    #[test]
    fn file_rewriter_returns_false_for_an_already_witnessed_block() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let edited = FileGapWitnessRewriter
            .witness(&gap_file, "GAP-0001", "fixtures/other.g")
            .expect("a witnessed block is not an IO error");
        assert!(!edited, "the witnessed block must not be re-edited");
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
        let _ = dir;
    }

    // -- engine ----------------------------------------------------------------

    #[test]
    fn run_gaps_witness_rewrites_the_committed_registry() {
        let (dir, gap_file) = seeded_root();
        let root = dir.path().to_path_buf();
        let outcome = run_gaps_witness(
            &root,
            &request("GAP-0002", "fixtures/cleanup-known-gap.g"),
            &FileGapWitnessRewriter,
        )
        .expect("the witness lands");
        assert_eq!(outcome.schema, "glyim.ledger-gaps-witness/1");
        assert_eq!(outcome.id, "GAP-0002");
        assert_eq!(outcome.class, "b");
        assert_eq!(outcome.witness, "fixtures/cleanup-known-gap.g");
        assert_eq!(
            outcome.target,
            gap_file.display().to_string(),
            "the outcome names the registry file that was rewritten"
        );
        assert_eq!(
            outcome.doc_target,
            root.join("KNOWN_GAPS.md").display().to_string(),
            "the outcome names the generated document the dispatch refreshes"
        );
        let on_disk = std::fs::read_to_string(&gap_file).expect("read back");
        assert_eq!(on_disk, CURATED_WITNESSED, "only the block changed");
        // The rewritten file re-loads cleanly and reports the gap as
        // witnessed: the write face keeps the registry the source of truth.
        let loaded = ledger_schema::load_project_registry(&root);
        assert!(
            loaded
                .diagnostics
                .iter()
                .all(|d| !matches!(d.severity, ledger_schema::Severity::Error)),
            "rewritten registry must load cleanly: {:?}",
            loaded.diagnostics
        );
        let gap = loaded
            .registry
            .gaps()
            .iter()
            .map(|sourced| &sourced.entry)
            .find(|gap| gap.id == "GAP-0002")
            .expect("GAP-0002 still present");
        assert_eq!(gap.status, GapStatus::Witnessed);
        assert_eq!(gap.witness.as_deref(), Some("fixtures/cleanup-known-gap.g"));
    }

    #[test]
    fn run_gaps_witness_rejects_unknown_id_without_writing() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let err = run_gaps_witness(
            dir.path(),
            &request("GAP-9999", "fixtures/cleanup-known-gap.g"),
            &FileGapWitnessRewriter,
        )
        .expect_err("unknown ID is a typed rejection");
        assert!(matches!(err, LedgerCliError::GapsWitnessUnknownId { .. }));
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
    }

    #[test]
    fn run_gaps_witness_rejects_an_already_witnessed_gap() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let err = run_gaps_witness(
            dir.path(),
            &request("GAP-0001", "fixtures/other.g"),
            &FileGapWitnessRewriter,
        )
        .expect_err("a witnessed gap is never silently re-witnessed");
        assert!(matches!(
            err,
            LedgerCliError::GapsWitnessAlreadyWitnessed { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
    }

    #[test]
    fn run_gaps_witness_rejects_a_missing_fixture_file() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let err = run_gaps_witness(
            dir.path(),
            &request("GAP-0002", "fixtures/does-not-exist.g"),
            &FileGapWitnessRewriter,
        )
        .expect_err("a phantom witness path is a rejection");
        assert!(matches!(
            err,
            LedgerCliError::GapsWitnessMissingFixture { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
        let _ = gap_file;
    }

    #[test]
    fn run_gaps_witness_rejects_escape_paths() {
        let (dir, _gap_file) = seeded_root();
        for witness in [
            "/absolute/path.g",
            "../escape.g",
            "fixtures/../secrets.g",
            "",
            "\t",
        ] {
            let err = run_gaps_witness(
                dir.path(),
                &request("GAP-0002", witness),
                &FileGapWitnessRewriter,
            )
            .expect_err("escape and unusable paths are rejections");
            assert!(
                matches!(err, LedgerCliError::GapsWitnessBadWitness { .. }),
                "path {witness:?} must be rejected"
            );
        }
    }

    #[test]
    fn run_gaps_witness_rejects_a_blank_id() {
        let (dir, _gap_file) = seeded_root();
        let err = run_gaps_witness(
            dir.path(),
            &request("", "fixtures/cleanup-known-gap.g"),
            &FileGapWitnessRewriter,
        )
        .expect_err("a blank ID can never match");
        assert!(matches!(err, LedgerCliError::GapsWitnessBadId { .. }));
    }

    #[test]
    fn run_gaps_witness_rejects_a_registry_that_does_not_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(
            dir.path(),
            "ledger/gaps/broken.toml",
            "[[gap]]\nid = \"nope\",\n",
        );
        let err = run_gaps_witness(
            dir.path(),
            &request("GAP-0002", "fixtures/x.g"),
            &FileGapWitnessRewriter,
        )
        .expect_err("a non-clean registry is refused before any write");
        assert!(matches!(err, LedgerCliError::GapsWitnessRegistry { .. }));
    }

    // -- formatters --------------------------------------------------------------

    #[test]
    fn format_gaps_witness_text_names_the_witnessed_gap() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_witness(
            dir.path(),
            &request("GAP-0002", "fixtures/cleanup-known-gap.g"),
            &FileGapWitnessRewriter,
        )
        .expect("the witness lands");
        let text = super::format_gaps_witness_text(&outcome);
        assert!(
            text.contains("witnessed GAP-0002"),
            "text names the gap: {text}"
        );
        assert!(
            text.contains("fixtures/cleanup-known-gap.g"),
            "text names the fixture: {text}"
        );
        assert!(
            text.contains("KNOWN_GAPS.md"),
            "text names the refreshed document: {text}"
        );
    }

    #[test]
    fn format_gaps_witness_json_parses_back_with_schema() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_witness(
            dir.path(),
            &request("GAP-0002", "fixtures/cleanup-known-gap.g"),
            &FileGapWitnessRewriter,
        )
        .expect("the witness lands");
        let json = super::format_gaps_witness_json(&outcome);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("the JSON contract parses");
        assert_eq!(parsed["schema"], "glyim.ledger-gaps-witness/1");
        assert_eq!(parsed["id"], "GAP-0002");
        assert_eq!(parsed["class"], "b");
        assert_eq!(parsed["witness"], "fixtures/cleanup-known-gap.g");
        assert!(parsed["target"].is_string());
        assert!(parsed["doc_target"].is_string());
    }

    #[test]
    fn both_gaps_witness_formatters_render_twice_byte_identical() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_witness(
            dir.path(),
            &request("GAP-0002", "fixtures/cleanup-known-gap.g"),
            &FileGapWitnessRewriter,
        )
        .expect("the witness lands");
        assert_eq!(
            super::format_gaps_witness_text(&outcome),
            super::format_gaps_witness_text(&outcome)
        );
        assert_eq!(
            super::format_gaps_witness_json(&outcome),
            super::format_gaps_witness_json(&outcome)
        );
    }

    // -- CLI wiring -----------------------------------------------------------

    #[test]
    fn gaps_witness_error_variants_carry_code_and_help() {
        for err in [
            LedgerCliError::GapsWitnessMissingField {
                field: "--id".to_string(),
                usage: usage(),
            },
            LedgerCliError::GapsWitnessBadId { id: String::new() },
            LedgerCliError::GapsWitnessBadWitness {
                witness: "../escape.g".to_string(),
            },
            LedgerCliError::GapsWitnessUnknownId {
                id: "GAP-9999".to_string(),
                candidates: vec!["GAP-0002".to_string()],
            },
            LedgerCliError::GapsWitnessRegistry {
                detail: "nope".to_string(),
            },
            LedgerCliError::GapsWitnessAlreadyWitnessed {
                id: "GAP-0001".to_string(),
                witness: "fixtures/await-loop-known-gap.g".to_string(),
                path: "ledger/gaps/known/0001-seed.toml".to_string(),
            },
            LedgerCliError::GapsWitnessMissingFixture {
                witness: "fixtures/none.g".to_string(),
                path: "fixtures/none.g".to_string(),
            },
            LedgerCliError::GapsWitnessWrite {
                path: "ledger/gaps/known/0001-seed.toml".to_string(),
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
    fn usage_lists_gaps_witness() {
        assert!(usage().contains("gaps witness"));
    }

    #[test]
    fn parse_args_accepts_gaps_witness() {
        let command = parse_args(args(&[
            "gaps",
            "witness",
            "project-root",
            "--id",
            "GAP-0002",
            "--witness",
            "fixtures/cleanup-known-gap.g",
            "--format",
            "json",
        ]))
        .expect("flags in any order parse");
        assert!(matches!(
            command,
            crate::Command::GapsWitness { ref root, ref id, ref witness, format, .. }
            if root == &PathBuf::from("project-root")
                && id == "GAP-0002"
                && witness == "fixtures/cleanup-known-gap.g"
                && format == super::GapsFormat::Json
        ));
    }

    #[test]
    fn parse_args_accepts_gaps_witness_with_default_root() {
        let command = parse_args(args(&[
            "gaps",
            "witness",
            "--id",
            "GAP-0002",
            "--witness",
            "fixtures/cleanup-known-gap.g",
        ]))
        .expect("ROOT defaults to .");
        assert!(matches!(
            command,
            crate::Command::GapsWitness { root, .. } if root.as_path() == Path::new(".")
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_witness_missing_id() {
        let err = parse_args(args(&[
            "gaps",
            "witness",
            "--witness",
            "fixtures/cleanup-known-gap.g",
        ]))
        .expect_err("missing --id");
        assert!(matches!(
            err,
            LedgerCliError::GapsWitnessMissingField { ref field, .. } if field == "--id"
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_witness_missing_witness() {
        let err = parse_args(args(&["gaps", "witness", "--id", "GAP-0002"]))
            .expect_err("missing --witness");
        assert!(matches!(
            err,
            LedgerCliError::GapsWitnessMissingField { ref field, .. } if field == "--witness"
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_witness_bad_format() {
        let err = parse_args(args(&[
            "gaps",
            "witness",
            "--id",
            "GAP-0002",
            "--witness",
            "fixtures/x.g",
            "--format",
            "yaml",
        ]))
        .expect_err("yaml is not a face format");
        assert!(matches!(err, LedgerCliError::GapsBadFormat { .. }));
    }

    #[test]
    fn parse_args_rejects_gaps_witness_unknown_flag() {
        let err = parse_args(args(&[
            "gaps",
            "witness",
            "--id",
            "GAP-0002",
            "--witness",
            "fixtures/x.g",
            "--force",
        ]))
        .expect_err("no force flag exists");
        assert!(matches!(err, LedgerCliError::UnknownFlag { .. }));
    }

    #[test]
    fn parse_args_keeps_the_plain_gaps_read_face() {
        // A ROOT positional that is not a sub-word routes to the read face
        // unchanged (the `waivers grant` dispatch shape).
        let command = parse_args(args(&["gaps", "some-root"])).expect("read face parses");
        assert!(matches!(
            command,
            crate::Command::Gaps { root } if root.as_path() == Path::new("some-root")
        ));
    }

    // -- retire face ------------------------------------------------------------

    /// The exact expected bytes after retiring GAP-0002 from the curated
    /// seed: the whole block is gone, the header prose and the sibling
    /// block pass through untouched.
    const CURATED_RETIRED: &str = "# Curated header prose (M1.1 seed) the rewriter must preserve\n\
         # byte-for-byte: witness debt stays honest — GAP-0002 is\n\
         # unwitnessed until a fixture reproduces it.\n\
         \n\
         [[gap]]\n\
         id        = \"GAP-0001\"\n\
         class     = \"a\"\n\
         title     = \"await in a loop body is diagnosed (T0060)\"\n\
         affects   = [\"LANG-DEMO-AWAIT\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status    = \"witnessed\"\n\
         witness   = \"fixtures/await-loop-known-gap.g\"\n";

    #[test]
    fn file_retirer_removes_exactly_the_target_block() {
        let (dir, gap_file) = seeded_root();
        let _ = dir;
        let removed = FileGapRetirer
            .retire(&gap_file, "GAP-0002")
            .expect("the retire succeeds");
        assert_eq!(
            removed.as_deref(),
            Some("no lowering site sets cleanup"),
            "the removed block's title comes back for the report"
        );
        let on_disk = std::fs::read_to_string(&gap_file).expect("read back");
        assert_eq!(on_disk, CURATED_RETIRED, "only the block is gone");
    }

    #[test]
    fn file_retirer_returns_none_for_an_unknown_id_and_writes_nothing() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let removed = FileGapRetirer
            .retire(&gap_file, "GAP-9999")
            .expect("an unknown ID is not an IO error");
        assert!(removed.is_none(), "no block carried the ID");
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
        let _ = dir;
    }

    #[test]
    fn in_memory_retirer_records_the_removal() {
        let retirer = InMemoryGapRetirer::default();
        retirer
            .doc
            .borrow_mut()
            .insert(PathBuf::from("gaps.toml"), CURATED_SEED.to_string());
        let removed = retirer
            .retire(Path::new("gaps.toml"), "GAP-0002")
            .expect("the in-memory retire lands");
        assert!(removed.is_some());
        assert_eq!(
            *retirer
                .doc
                .borrow()
                .get(Path::new("gaps.toml"))
                .expect("doc"),
            CURATED_RETIRED
        );
        assert_eq!(retirer.removed.borrow().len(), 1);
    }

    #[test]
    fn run_gaps_retire_removes_the_gap_and_reports_remaining() {
        let (dir, gap_file) = seeded_root();
        let root = dir.path().to_path_buf();
        let outcome = run_gaps_retire(
            &root,
            &GapsRetireRequest {
                id: "GAP-0002".to_string(),
            },
            &FileGapRetirer,
        )
        .expect("the retire lands");
        assert_eq!(outcome.schema, "glyim.ledger-gaps-retire/1");
        assert_eq!(outcome.id, "GAP-0002");
        assert_eq!(outcome.title, "no lowering site sets cleanup");
        assert_eq!(
            outcome.target,
            gap_file.display().to_string(),
            "the outcome names the registry file that was rewritten"
        );
        assert_eq!(outcome.remaining, 1, "one gap remains");
        let on_disk = std::fs::read_to_string(&gap_file).expect("read back");
        assert_eq!(on_disk, CURATED_RETIRED, "only the block is gone");
        // The rewritten file re-loads cleanly with one gap left: the write
        // face keeps the registry the source of truth.
        let loaded = ledger_schema::load_project_registry(&root);
        assert!(
            loaded
                .diagnostics
                .iter()
                .all(|d| !matches!(d.severity, ledger_schema::Severity::Error)),
            "rewritten registry must load cleanly: {:?}",
            loaded.diagnostics
        );
        assert_eq!(loaded.registry.gap_count(), 1);
        assert!(
            loaded
                .registry
                .gaps()
                .iter()
                .all(|sourced| sourced.entry.id != "GAP-0002"),
            "the retired gap is gone from the registry"
        );
    }

    #[test]
    fn run_gaps_retire_rejects_unknown_id_without_writing() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let err = run_gaps_retire(
            dir.path(),
            &GapsRetireRequest {
                id: "GAP-9999".to_string(),
            },
            &FileGapRetirer,
        )
        .expect_err("unknown ID is a typed rejection");
        assert!(matches!(err, LedgerCliError::GapsRetireUnknownId { .. }));
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
    }

    #[test]
    fn run_gaps_retire_rejects_a_registry_that_does_not_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(
            dir.path(),
            "ledger/gaps/broken.toml",
            "[[gap]]\nid = \"nope\",\n",
        );
        let err = run_gaps_retire(
            dir.path(),
            &GapsRetireRequest {
                id: "GAP-0002".to_string(),
            },
            &FileGapRetirer,
        )
        .expect_err("a non-clean registry is refused before any write");
        assert!(matches!(err, LedgerCliError::GapsRetireLoad { .. }));
    }

    #[test]
    fn run_gaps_retire_rejects_a_blank_id() {
        let (dir, _gap_file) = seeded_root();
        let err = run_gaps_retire(
            dir.path(),
            &GapsRetireRequest { id: String::new() },
            &FileGapRetirer,
        )
        .expect_err("a blank ID can never match");
        assert!(matches!(err, LedgerCliError::GapsRetireBadId { .. }));
    }

    #[test]
    fn format_gaps_retire_text_names_the_retired_gap() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_retire(
            dir.path(),
            &GapsRetireRequest {
                id: "GAP-0002".to_string(),
            },
            &FileGapRetirer,
        )
        .expect("the retire lands");
        let text = super::format_gaps_retire_text(&outcome);
        assert!(
            text.contains("retired GAP-0002"),
            "text names the gap: {text}"
        );
        assert!(
            text.contains("1 gap(s) remain"),
            "text names the remaining count: {text}"
        );
    }

    #[test]
    fn format_gaps_retire_json_parses_back_with_schema() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_retire(
            dir.path(),
            &GapsRetireRequest {
                id: "GAP-0002".to_string(),
            },
            &FileGapRetirer,
        )
        .expect("the retire lands");
        let json = super::format_gaps_retire_json(&outcome);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("the JSON contract parses");
        assert_eq!(parsed["schema"], "glyim.ledger-gaps-retire/1");
        assert_eq!(parsed["id"], "GAP-0002");
        assert_eq!(parsed["title"], "no lowering site sets cleanup");
        assert_eq!(parsed["remaining"], 1);
        assert!(parsed["target"].is_string());
    }

    #[test]
    fn both_gaps_retire_formatters_render_twice_byte_identical() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_retire(
            dir.path(),
            &GapsRetireRequest {
                id: "GAP-0002".to_string(),
            },
            &FileGapRetirer,
        )
        .expect("the retire lands");
        assert_eq!(
            super::format_gaps_retire_text(&outcome),
            super::format_gaps_retire_text(&outcome)
        );
        assert_eq!(
            super::format_gaps_retire_json(&outcome),
            super::format_gaps_retire_json(&outcome)
        );
    }

    #[test]
    fn gaps_retire_error_variants_carry_code_and_help() {
        for err in [
            LedgerCliError::GapsRetireMissingField {
                field: "--id".to_string(),
                usage: usage(),
            },
            LedgerCliError::GapsRetireBadId { id: String::new() },
            LedgerCliError::GapsRetireUnknownId {
                id: "GAP-9999".to_string(),
                candidates: vec!["GAP-0002".to_string()],
            },
            LedgerCliError::GapsRetireLoad {
                detail: "nope".to_string(),
            },
            LedgerCliError::GapsRetireWrite {
                path: "ledger/gaps/known/0001-seed.toml".to_string(),
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
    fn usage_lists_gaps_retire() {
        assert!(usage().contains("gaps retire"));
    }

    #[test]
    fn parse_args_accepts_gaps_retire() {
        let command = parse_args(args(&[
            "gaps",
            "retire",
            "project-root",
            "--id",
            "GAP-0002",
            "--format",
            "json",
        ]))
        .expect("flags in any order parse");
        assert!(matches!(
            command,
            crate::Command::GapsRetire { ref root, ref id, format, .. }
            if root == &PathBuf::from("project-root")
                && id == "GAP-0002"
                && format == super::GapsFormat::Json
        ));
    }

    #[test]
    fn parse_args_accepts_gaps_retire_with_default_root() {
        let command =
            parse_args(args(&["gaps", "retire", "--id", "GAP-0002"])).expect("ROOT defaults to .");
        assert!(matches!(
            command,
            crate::Command::GapsRetire { root, .. } if root.as_path() == Path::new(".")
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_retire_missing_id() {
        let err = parse_args(args(&["gaps", "retire"])).expect_err("missing --id");
        assert!(matches!(
            err,
            LedgerCliError::GapsRetireMissingField { ref field, .. } if field == "--id"
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_retire_bad_format() {
        let err = parse_args(args(&[
            "gaps", "retire", "--id", "GAP-0002", "--format", "yaml",
        ]))
        .expect_err("yaml is not a face format");
        assert!(matches!(err, LedgerCliError::GapsBadFormat { .. }));
    }

    /// The parse-args helper every CLI test in the crate shares (the
    /// `lib.rs` tests' `args` shape).
    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items.iter().map(|s| (*s).to_string())
    }
}
