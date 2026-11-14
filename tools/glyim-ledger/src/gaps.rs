//! Gap-lifecycle faces: the write side (`gaps open`, `gaps witness`,
//! `gaps retire`) and its trust loop — the `gaps audit` finding face
//! (every registered gap's witness fixture resolved against the tree)
//! and the `gaps unwitness` remedy face (one witness detached, honest
//! witness debt restored).
//!
//! The gap registry (`ledger/gaps/known/*.toml`, parsed by
//! `ledger-schema`) tracks every open gap's witness status, and the
//! `render-gaps` face generates `KNOWN_GAPS.md` from it with a `--check`
//! drift gate — but until now there was no sanctioned way to *change*
//! that status: opening a gap, recording a witness, or retiring a closed
//! gap meant hand-editing the curated TOML files and remembering to
//! regenerate the rendered document. These faces are the write side of
//! that lifecycle, mirroring the `waivers grant/revoke` shape: a
//! validated request, a source-of-truth load pass that refuses anything
//! but a clean registry, a rewrite seam the engine consumes (file-backed
//! in the CLI, in-memory in tests and walkthroughs), typed
//! [`LedgerCliError`] rejections that never leave a partial write behind,
//! and text/JSON formatters.
//!
//! `gaps open` is the one face that *appends* rather than edits: it
//! renders the canonical `[[gap]]` block in the committed registry style
//! (keys aligned to the `opened_in` column, explicit
//! `status = "unwitnessed"` — the serde default for a missing `status`
//! key is `witnessed`, so an omitted line would mint silent debt) and
//! appends it after one blank separator, minting the next numbered
//! `ledger/gaps/known/NNNN-gaps-open.toml` file unless `--file` names an
//! existing registry file to extend. The other two faces are
//! deliberately *surgical* rather than canonical: the committed gap
//! files carry curated header prose (the M1.1/M1.7 seed comments) that a
//! full re-serialization would discard, so the rewriters edit only the
//! matched `[[gap]]` block's lines and leave every other byte untouched
//! — the opposite tradeoff to the `waivers` faces, where the document is
//! machine-written and canonicalization is the contract. The generated
//! `KNOWN_GAPS.md` document is refreshed by the dispatch layer after a
//! successful write (the same `run_render_gaps` pipeline the `--check`
//! gate byte-compares against), so the faces never leave the committed
//! document silently drifted from the registry.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ledger_schema::{Diagnostic, GapEntry, GapStatus, Severity, Sourced};

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

// -- open face -----------------------------------------------------------------

/// Schema tag of the `gaps open` JSON contract.
pub const GAPS_OPEN_SCHEMA: &str = "glyim.ledger-gaps-open/1";

/// The `gaps open` request (the CLI flags, validated by [`run_gaps_open`]):
/// file a NEW gap entry into the registry — the opening stage of the gap
/// lifecycle. Until this face the witness and retire stages had sanctioned
/// commands, but opening a gap meant hand-writing the curated TOML block
/// and remembering the aligned registry style, the explicit
/// `status = "unwitnessed"` line, and the regenerate-`KNOWN_GAPS.md` step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapsOpenRequest {
    /// The new gap ID (`GAP-NNNN`, exactly four digits). The face mints
    /// IDs, so the canonical shape is enforced here, not just against the
    /// loaded registry.
    pub id: String,
    /// The gap class (`a` diagnosed, `b` silent, `c` deferred) — parsed
    /// from `--class` by [`parse_gap_class`].
    pub class: ledger_schema::GapClass,
    /// Human-readable one-liner (single line: control characters are
    /// rejected, `"` and `\` are TOML-escaped on render).
    pub title: String,
    /// Feature IDs this gap limits — every one must exist in the loaded
    /// feature registry (a gap that affects nothing real is silent debt).
    pub affects: Vec<String>,
    /// The readiness-level ceiling this gap imposes on the affected
    /// features — parsed from `--caps` by [`parse_gap_level`].
    pub caps: ledger_schema::Level,
    /// Tag or ref where the gap was first recorded (`v0.4.0`).
    pub opened_in: String,
    /// Explicit registry file to append to, relative to the project root
    /// (`ledger/gaps/known/0001-seed.toml`, `.toml` only, no `..` escape).
    /// The file must already exist — curated registry files are
    /// hand-created with provenance prose, and the face appends blocks, it
    /// does not mint arbitrary files. `None` resolves the default target:
    /// the next numbered file `ledger/gaps/known/NNNN-gaps-open.toml`.
    pub file: Option<String>,
}

/// Parse a `--class` value (`a` | `b` | `c`) into a [`ledger_schema::GapClass`].
#[must_use]
pub fn parse_gap_class(value: &str) -> Option<ledger_schema::GapClass> {
    match value {
        "a" => Some(ledger_schema::GapClass::A),
        "b" => Some(ledger_schema::GapClass::B),
        "c" => Some(ledger_schema::GapClass::C),
        _ => None,
    }
}

/// Parse a `--caps` value (`L0`..`L5`) into a [`ledger_schema::Level`].
#[must_use]
pub fn parse_gap_level(value: &str) -> Option<ledger_schema::Level> {
    ["L0", "L1", "L2", "L3", "L4", "L5"]
        .iter()
        .find(|spelling| **spelling == value)
        .map(|spelling| match *spelling {
            "L0" => ledger_schema::Level::L0,
            "L1" => ledger_schema::Level::L1,
            "L2" => ledger_schema::Level::L2,
            "L3" => ledger_schema::Level::L3,
            "L4" => ledger_schema::Level::L4,
            _ => ledger_schema::Level::L5,
        })
}

/// Where a new `[[gap]]` block is appended. The append seam mirrors
/// [`GapWitnessRewriter`] and [`GapRetirer`]: the engine consumes the
/// trait, so tests drive an in-memory document and the CLI wires the
/// file-backed implementation.
pub trait GapOpener {
    /// Appends the canonical `block` (the exact bytes the engine rendered,
    /// without a trailing newline) to the document at `path`, leaving every
    /// existing byte untouched. Returns whether the append landed (`false`
    /// = the document already carries a `[[gap]]` block with the exact ID:
    /// the drift guard that keeps a load-pass/rewrite race from minting a
    /// duplicate); the error detail is the IO failure message (the CLI
    /// turns it into a typed write error).
    fn open(&self, path: &Path, id: &str, block: &str) -> Result<bool, String>;
}

/// Appends the block to the registry file on disk: read (a missing file is
/// an empty document — the default numbered target is minted on first
/// open), guard the duplicate ID, append after one blank separator,
/// replace the file. A document that already carries the ID is refused
/// (`Ok(false)`), never silently extended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileGapOpener;

impl GapOpener for FileGapOpener {
    fn open(&self, path: &Path, id: &str, block: &str) -> Result<bool, String> {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let Some(edited) = append_gap_block(&text, id, block) else {
            return Ok(false);
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
        }
        std::fs::write(path, edited)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(true)
    }
}

/// Operates on owned in-memory documents (tests and programmatic
/// consumers); the in-memory twin of [`FileGapOpener`]. Every successful
/// append is recorded in [`Self::opened`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryGapOpener {
    /// The documents the opener operates on, keyed by path.
    pub doc: std::cell::RefCell<std::collections::HashMap<PathBuf, String>>,
    /// Every `(path, opened ID)` append performed, in order.
    pub opened: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl GapOpener for InMemoryGapOpener {
    fn open(&self, path: &Path, id: &str, block: &str) -> Result<bool, String> {
        let mut doc = self.doc.borrow_mut();
        let Some(text) = doc.get_mut(path) else {
            return Ok(false);
        };
        let Some(edited) = append_gap_block(text, id, block) else {
            return Ok(false);
        };
        *text = edited;
        self.opened
            .borrow_mut()
            .push((path.to_path_buf(), id.to_string()));
        Ok(true)
    }
}

/// The canonical append behind both openers: find no `[[gap]]` block
/// carrying the exact ID (the drift guard), then append the block after
/// one blank separator — the curated header prose and every sibling block
/// pass through byte-for-byte. Returns `None` when the document already
/// carries the ID; the engine maps that to a typed drift error.
///
/// A new gap is always written with the explicit `status = "unwitnessed"`
/// line ([`render_gap_block`] hard-codes it): the serde default for a
/// missing `status` key is `witnessed`, so an omitted line would mint
/// silent debt — the exact outcome the plan §3.3 rule forbids.
fn append_gap_block(text: &str, id: &str, block: &str) -> Option<String> {
    // The duplicate scan re-uses the block walk the witness edit uses.
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim() != "[[gap]]" {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < lines.len() && lines[end].trim() != "[[gap]]" {
            end += 1;
        }
        let matches_id = lines[index..end]
            .iter()
            .any(|entry| block_key(entry) == Some("id") && block_value(entry) == Some(id));
        if matches_id {
            return None;
        }
        index = end;
    }
    let mut result = String::with_capacity(text.len() + block.len() + 2);
    result.push_str(text);
    if !text.is_empty() && !text.ends_with('\n') {
        result.push('\n');
    }
    if !text.is_empty() {
        result.push('\n');
    }
    result.push_str(block);
    result.push('\n');
    Some(result)
}

/// A registry file an explicit `--file` may target: relative to the
/// project root, backslash-free, no `..` component, a `.toml` file under
/// `ledger/gaps/`. The committed entries live under `ledger/gaps/known/`
/// with `NNNN-slug.toml` names; the face appends to curated files, it
/// does not mint arbitrary user paths (only the default numbered target
/// is created).
fn usable_registry_file(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
        && !file.chars().any(char::is_control)
        && !file.contains('\\')
        && !Path::new(file).is_absolute()
        && Path::new(file)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
        && Path::new(file).starts_with("ledger/gaps/")
}

/// The escape behind [`render_gap_block`]: a TOML basic string safe to
/// embed in the block (`"`, `\`; control characters were rejected at
/// validation, so no `\u` escapes are needed).
fn escape_toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out
}

/// Renders one `[[gap]]` block in the committed registry style: keys
/// padded to the `opened_in` column, the quoted single-line title, the
/// `affects` array, and the explicit `status = "unwitnessed"` line (a
/// fresh gap is witness debt, never silent debt). Pure — the same request
/// always renders the same bytes, which is what the golden bytes in the
/// tests and the `--check` gates rely on.
fn render_gap_block(request: &GapsOpenRequest) -> String {
    let keys = [
        "id",
        "class",
        "title",
        "affects",
        "caps",
        "opened_in",
        "status",
    ];
    let width = keys.iter().map(|key| key.len()).max().unwrap_or_default() + 1;
    let mut block = String::new();
    block.push_str("[[gap]]\n");
    let mut line = |key: &str, value: &str| {
        let _ = writeln!(
            block,
            "{key}{}= {value}",
            " ".repeat(width.saturating_sub(key.len()))
        );
    };
    line("id", &format!("\"{}\"", request.id));
    line("class", &format!("\"{}\"", request.class.as_str()));
    line(
        "title",
        &format!("\"{}\"", escape_toml_string(&request.title)),
    );
    let affects = request
        .affects
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(", ");
    line("affects", &format!("[{affects}]"));
    line("caps", &format!("\"{}\"", request.caps.as_str()));
    line(
        "opened_in",
        &format!("\"{}\"", escape_toml_string(&request.opened_in)),
    );
    line("status", "\"unwitnessed\"");
    block
}

/// The default open target: the next numbered file under
/// `ledger/gaps/known/` (`NNNN-gaps-open.toml`, `NNNN` = the largest
/// leading number across the committed `*.toml` filenames + 1). A pure
/// function of the tree, so repeated opens mint one file per gap with
/// deterministic names; `0001-gaps-open.toml` seeds an empty directory.
fn next_open_registry_file(root: &Path) -> PathBuf {
    let known = root.join("ledger/gaps/known");
    let mut max = 0_u64;
    if let Ok(entries) = std::fs::read_dir(&known) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let digits = name.split('.').next().unwrap_or_default();
            let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse::<u64>() {
                max = max.max(number);
            }
        }
    }
    known.join(format!("{:04}-gaps-open.toml", max + 1))
}

/// Result of a `gaps open` run (also the `glyim.ledger-gaps-open/1` JSON
/// face via [`format_gaps_open_json`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GapsOpenOutcome {
    /// Schema tag, always [`GAPS_OPEN_SCHEMA`].
    pub schema: &'static str,
    /// The opened gap's ID.
    pub id: String,
    /// The gap's class (`a` diagnosed, `b` silent, `c` deferred).
    pub class: String,
    /// The gap's registry title.
    pub title: String,
    /// The feature IDs the gap limits.
    pub affects: Vec<String>,
    /// The readiness-level ceiling (`L2`).
    pub caps: String,
    /// The tag or ref the gap was opened in (`v0.4.0`).
    pub opened_in: String,
    /// The registry file the `[[gap]]` block was appended to.
    pub target: String,
    /// The generated document the dispatch layer refreshes after the
    /// append (`<ROOT>/KNOWN_GAPS.md`) — the face never leaves the
    /// committed render silently drifted from the registry.
    pub doc_target: String,
}

/// Opens one gap: validates the request, refuses to touch anything unless
/// the registry loads cleanly and the ID is free, and appends the
/// canonical `[[gap]]` block through `opener`.
///
/// # Validation contract
///
/// - `id` must match the canonical `GAP-NNNN` shape (exactly four digits —
///   the face mints IDs, so the shape is enforced here, not just matched);
/// - `title` and `opened_in` must be usable one-liners (non-blank, no
///   control characters; `"` and `\` are escaped on render);
/// - `affects` must be non-empty, and every feature ID must exist in the
///   loaded registry (a gap that affects nothing real is silent debt);
/// - the registry must load with zero error-severity diagnostics (a
///   registry that does not parse is never extended — the load rule);
/// - the ID must be free (an existing gap is never silently shadowed —
///   the grant-face duplicate rule);
/// - an explicit `--file` must be a relative, `.toml` path under
///   `ledger/gaps/` that already exists; the default target is the next
///   numbered `ledger/gaps/known/NNNN-gaps-open.toml`.
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`GapsOpenBadId`, `GapsOpenBadTitle`, `GapsOpenBadOpenedIn`,
/// `GapsOpenBadFile`, `GapsOpenAffectsUnknown`, `GapsOpenDuplicateId`,
/// `GapsOpenRegistry`) and for a failed or drifted append
/// (`GapsOpenWrite`).
pub fn run_gaps_open(
    root: &Path,
    request: &GapsOpenRequest,
    opener: &dyn GapOpener,
) -> Result<GapsOpenOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "gaps_open",
        root = %root.display(),
        id = %request.id,
    );
    let _enter = span.enter();

    // The face mints IDs, so the canonical GAP-NNNN shape is enforced
    // here — not just matched against the loaded registry.
    let id_ok = request.id.len() == 8
        && request.id.starts_with("GAP-")
        && request.id[4..].chars().all(|ch| ch.is_ascii_digit());
    if !id_ok {
        return Err(LedgerCliError::GapsOpenBadId {
            id: request.id.clone(),
        });
    }
    if request.title.trim().is_empty() || request.title.chars().any(char::is_control) {
        return Err(LedgerCliError::GapsOpenBadTitle {
            title: request.title.clone(),
        });
    }
    if request.opened_in.trim().is_empty() || request.opened_in.chars().any(char::is_control) {
        return Err(LedgerCliError::GapsOpenBadOpenedIn {
            opened_in: request.opened_in.clone(),
        });
    }
    if let Some(file) = &request.file {
        if !usable_registry_file(file) {
            return Err(LedgerCliError::GapsOpenBadFile { file: file.clone() });
        }
    }

    // The load rule: a registry that does not parse cleanly is never
    // extended, exactly like the witness/retire/waivers faces.
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
        return Err(LedgerCliError::GapsOpenRegistry { detail });
    }

    // Every affected feature must exist: a gap that limits nothing real
    // would cap nothing and stay silently undiscoverable in the reports.
    for feature in &request.affects {
        if loaded.registry.feature_by_id(feature).is_none() {
            return Err(LedgerCliError::GapsOpenAffectsUnknown {
                affects: request.affects.clone(),
                known: loaded
                    .registry
                    .features()
                    .iter()
                    .map(|sourced| sourced.entry.id.clone())
                    .take(3)
                    .collect::<Vec<_>>(),
            });
        }
    }

    // The duplicate rule: an existing gap is never silently shadowed (the
    // grant-face rule; extend the gap or retire it deliberately instead).
    if let Some(sourced) = loaded
        .registry
        .gaps()
        .iter()
        .find(|sourced| sourced.entry.id == request.id)
    {
        return Err(LedgerCliError::GapsOpenDuplicateId {
            id: request.id.clone(),
            path: sourced.path.display().to_string(),
        });
    }

    let target = match &request.file {
        Some(file) => {
            let path = root.join(file);
            if !path.is_file() {
                return Err(LedgerCliError::GapsOpenBadFile { file: file.clone() });
            }
            path
        }
        None => next_open_registry_file(root),
    };

    let block = render_gap_block(request);
    let opened = opener
        .open(&target, &request.id, block.trim_end_matches('\n'))
        .map_err(|detail| LedgerCliError::GapsOpenWrite {
            path: target.display().to_string(),
            detail,
        })?;
    if !opened {
        // The load pass said the ID was free but the document now carries
        // it: the registry drifted between load and rewrite. Fail without
        // a partial write.
        return Err(LedgerCliError::GapsOpenDuplicateId {
            id: request.id.clone(),
            path: target.display().to_string(),
        });
    }
    tracing::debug!(
        id = %request.id,
        target = %target.display(),
        "gap opened"
    );

    Ok(GapsOpenOutcome {
        schema: GAPS_OPEN_SCHEMA,
        id: request.id.clone(),
        class: request.class.as_str().to_string(),
        title: request.title.clone(),
        affects: request.affects.clone(),
        caps: request.caps.as_str().to_string(),
        opened_in: request.opened_in.clone(),
        target: target.display().to_string(),
        doc_target: root
            .join(crate::render::KNOWN_GAPS_MD_TARGET)
            .display()
            .to_string(),
    })
}

/// Renders the open outcome's text face: one summary line (the opened
/// gap, its class, and both touched paths).
#[must_use]
pub fn format_gaps_open_text(outcome: &GapsOpenOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "opened {} ({}) capping {} at {} -> {} appended, refresh {}",
        outcome.id,
        outcome.class,
        outcome.affects.join(", "),
        outcome.caps,
        outcome.target,
        outcome.doc_target,
    );
    text
}

/// Renders the `glyim.ledger-gaps-open/1` JSON contract.
#[must_use]
pub fn format_gaps_open_json(outcome: &GapsOpenOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-gaps-open/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

// -- gaps audit face -----------------------------------------------------------

/// Schema tag of the `gaps audit` JSON contract.
pub const GAPS_AUDIT_SCHEMA: &str = "glyim.ledger-gaps-audit/1";

/// Verdict of one audited gap's witness (the trust-loop closure over the
/// write faces: `gaps witness` checks the fixture at attach time, but
/// nothing re-verified the registry against the tree afterwards — a
/// fixture that is moved, renamed, or deleted left the registry claiming
/// a witness that no longer exists, which is exactly the silent debt
/// plan §3.3 forbids).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum GapAuditVerdict {
    /// The gap is witnessed and its fixture file exists under ROOT.
    #[serde(rename = "witnessed")]
    Witnessed,
    /// The gap is open without a witness — honest witness debt
    /// (displayed, never failed: the gaps-report rule).
    #[serde(rename = "debt")]
    Debt,
    /// The registry claims a witness the tree no longer carries — silent
    /// debt surfaced; the audit gate fails on it.
    #[serde(rename = "missing")]
    Missing,
    /// The witness path resolves to something that is not a readable
    /// regular file (a directory, or a metadata error other than
    /// not-found).
    #[serde(rename = "unreadable")]
    Unreadable,
}

impl GapAuditVerdict {
    /// Canonical wire spelling (also the text-face column).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Witnessed => "witnessed",
            Self::Debt => "debt",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
        }
    }
}

/// One audited gap (the JSON contract's row shape).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GapAuditRow {
    /// The audited gap's ID (echoed verbatim).
    pub id: String,
    /// The gap's class (`a` diagnosed, `b` silent, `c` deferred).
    pub class: String,
    /// The gap's registry title (echoed for the report line).
    pub title: String,
    /// The witness path the registry records (echoed verbatim; `None`
    /// for unwitnessed debt and for a witnessed block that carries no
    /// witness line).
    pub witness: Option<String>,
    /// The computed verdict.
    pub verdict: GapAuditVerdict,
    /// Human-readable detail for the non-`witnessed`/non-`debt` verdicts
    /// (rendered in the text report).
    pub detail: Option<String>,
}

/// Result of a `gaps audit` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapAuditOutcome {
    /// Schema tag, always [`GAPS_AUDIT_SCHEMA`].
    pub schema: &'static str,
    /// Gaps audited (equal to [`Self::rows`] length).
    pub gaps: usize,
    /// Rows whose verdict is [`GapAuditVerdict::Witnessed`].
    pub witnessed: usize,
    /// Rows whose verdict is [`GapAuditVerdict::Debt`].
    pub debt: usize,
    /// Rows whose verdict is [`GapAuditVerdict::Missing`].
    pub missing: usize,
    /// Rows whose verdict is [`GapAuditVerdict::Unreadable`].
    pub unreadable: usize,
    /// One row per audited gap, sorted by gap ID.
    pub rows: Vec<GapAuditRow>,
    /// Load diagnostics (registry), sorted and de-duplicated — the same
    /// diagnostics the `gaps` report face would emit.
    pub diagnostics: Vec<Diagnostic>,
}

impl GapAuditOutcome {
    /// Number of error-severity load diagnostics.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .count()
    }

    /// True when the audit gate passes: no error-severity load
    /// diagnostic and no row whose verdict is [`GapAuditVerdict::Missing`]
    /// or [`GapAuditVerdict::Unreadable`]. Witness debt does not fail the
    /// gate — the plan §3.3 rule is "allowed, but never silent", and the
    /// audit face renders every debt row in the same run.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors() == 0 && self.missing == 0 && self.unreadable == 0
    }
}

/// The pure verdict pass over already-loaded gaps: fixture paths resolve
/// against `root` (the same repo-relative spelling the write faces
/// record), rows render in gap-ID order so the face is deterministic for
/// every input path.
#[must_use]
pub fn audit_gaps(root: &Path, gaps: &[Sourced<GapEntry>]) -> Vec<GapAuditRow> {
    let mut rows = Vec::with_capacity(gaps.len());
    for sourced in gaps {
        let entry = &sourced.entry;
        let (verdict, detail) = match (&entry.status, &entry.witness) {
            (_, Some(witness)) => {
                let path = root.join(witness);
                match std::fs::metadata(&path) {
                    Ok(metadata) if metadata.is_file() => (GapAuditVerdict::Witnessed, None),
                    Ok(_) => (
                        GapAuditVerdict::Unreadable,
                        Some(format!("{} is not a regular file", path.display())),
                    ),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => (
                        GapAuditVerdict::Missing,
                        Some(format!("{} is gone", path.display())),
                    ),
                    Err(err) => (
                        GapAuditVerdict::Unreadable,
                        Some(format!("{} is unreadable: {err}", path.display())),
                    ),
                }
            }
            (GapStatus::Witnessed, None) => (
                GapAuditVerdict::Missing,
                Some(
                    "no witness path recorded (the serde default for a \
                      missing status key is witnessed)"
                        .to_string(),
                ),
            ),
            (GapStatus::Unwitnessed, None) => (GapAuditVerdict::Debt, None),
        };
        rows.push(GapAuditRow {
            id: entry.id.clone(),
            class: entry.class.as_str().to_string(),
            title: entry.title.clone(),
            witness: entry.witness.clone(),
            verdict,
            detail,
        });
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

/// Audits the gap registry under `ROOT`: every registered gap's witness
/// is resolved against the tree, one row per gap (sorted by ID), load
/// diagnostics sorted and de-duplicated. A witnessed gap whose fixture
/// is missing or unreadable fails the gate — the same trust-loop closure
/// the `evidence audit` face performs over the evidence store, applied
/// to the witness paths the gap-lifecycle write faces record.
#[must_use]
pub fn run_gaps_audit(root: &Path) -> GapAuditOutcome {
    let span = tracing::info_span!("gaps_audit", root = %root.display());
    let _enter = span.enter();

    let loaded = ledger_schema::load_project_registry(root);
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(loaded.registry.validate());

    let mut gaps: Vec<_> = loaded.registry.gaps().to_vec();
    gaps.sort_by(|a, b| a.id().cmp(b.id()));
    let rows = audit_gaps(root, &gaps);

    let (mut witnessed, mut debt, mut missing, mut unreadable) = (0usize, 0usize, 0usize, 0usize);
    for row in &rows {
        match row.verdict {
            GapAuditVerdict::Witnessed => witnessed += 1,
            GapAuditVerdict::Debt => debt += 1,
            GapAuditVerdict::Missing => missing += 1,
            GapAuditVerdict::Unreadable => unreadable += 1,
        }
    }

    diagnostics.sort_by(|a, b| a.code.cmp(b.code).then(a.message.cmp(&b.message)));
    diagnostics.dedup();
    tracing::debug!(
        gaps = rows.len(),
        witnessed,
        debt,
        missing,
        unreadable,
        diagnostics = diagnostics.len(),
        "gaps audit pass finished"
    );
    GapAuditOutcome {
        schema: GAPS_AUDIT_SCHEMA,
        gaps: rows.len(),
        witnessed,
        debt,
        missing,
        unreadable,
        rows,
        diagnostics,
    }
}

/// Renders the audit outcome's text face (the `evidence audit` shape: a
/// summary line, one line per non-witnessed/non-debt row, the witness
/// debt reminder, diagnostics last).
#[must_use]
pub fn format_gaps_audit_text(outcome: &GapAuditOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "gaps audit: {} gap(s), {} witnessed, {} debt, {} missing, {} unreadable, \
         {} diagnostic(s), {}",
        outcome.gaps,
        outcome.witnessed,
        outcome.debt,
        outcome.missing,
        outcome.unreadable,
        outcome.diagnostics.len(),
        if outcome.is_ok() { "passed" } else { "FAILED" },
    );
    for row in &outcome.rows {
        if matches!(
            row.verdict,
            GapAuditVerdict::Witnessed | GapAuditVerdict::Debt
        ) {
            continue;
        }
        let _ = writeln!(
            text,
            "  {:<8} {:<10} {}",
            row.id,
            row.verdict.as_str(),
            row.detail.as_deref().unwrap_or_default(),
        );
    }
    if outcome.debt > 0 {
        let _ = writeln!(
            text,
            "witness debt: {} gap(s) without a strict-xfail witness (plan §3.3); \
             per-entry reasons live in ledger/gaps/known/*.toml",
            outcome.debt
        );
    }
    for diagnostic in &outcome.diagnostics {
        let _ = writeln!(text, "{diagnostic}");
    }
    text
}

/// Renders the `glyim.ledger-gaps-audit/1` JSON contract.
#[must_use]
pub fn format_gaps_audit_json(outcome: &GapAuditOutcome) -> String {
    #[derive(serde::Serialize)]
    struct AuditJson<'a> {
        schema: &'static str,
        gaps: usize,
        witnessed: usize,
        debt: usize,
        missing: usize,
        unreadable: usize,
        passed: bool,
        rows: &'a [GapAuditRow],
    }
    let document = AuditJson {
        schema: GAPS_AUDIT_SCHEMA,
        gaps: outcome.gaps,
        witnessed: outcome.witnessed,
        debt: outcome.debt,
        missing: outcome.missing,
        unreadable: outcome.unreadable,
        passed: outcome.is_ok(),
        rows: &outcome.rows,
    };
    let mut text = serde_json::to_string_pretty(&document)
        .expect("the glyim.ledger-gaps-audit/1 document is a serialisation-only struct");
    text.push('\n');
    text
}

// -- gaps unwitness face -------------------------------------------------------

/// Schema tag of the `gaps unwitness` JSON contract.
pub const GAPS_UNWITNESS_SCHEMA: &str = "glyim.ledger-gaps-unwitness/1";

/// The `gaps unwitness` request (the CLI flags, validated by
/// [`run_gaps_unwitness`]): detach one witnessed gap's witness — the
/// remedy half of the trust loop. The audit face's `Missing` verdict is
/// the finding (the registry claims a fixture the tree no longer
/// carries); until this face the sanctioned remedy did not exist — the
/// witness face refuses an already-witnessed gap, so fixing a stale or
/// wrong witness path meant hand-editing the curated TOML block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapsUnwitnessRequest {
    /// The exact gap ID (`GAP-NNNN`) to detach.
    pub id: String,
}

/// Where a `[[gap]]` block's witness lines are detached. The seam mirrors
/// [`GapWitnessRewriter`] on the reverse edit: the engine consumes the
/// trait, so tests drive an in-memory document and the CLI wires the
/// file-backed implementation.
pub trait GapWitnessDetacher {
    /// Flips the `[[gap]]` block carrying the exact `id` from
    /// `status = "witnessed"` back to `status = "unwitnessed"` and removes
    /// the `witness = "..."` line, leaving every other byte of the
    /// document (the curated header prose included) untouched. Returns
    /// whether the edit landed (`false` = no witnessed (or
    /// witness-carrying) block carried the ID: an unknown ID, or one
    /// whose status drifted after the engine's load pass); the error
    /// detail is the IO or shape failure message (the CLI turns it into a
    /// typed write error).
    fn detach(&self, path: &Path, id: &str) -> Result<bool, String>;
}

/// Edits the registry file on disk exactly where the loaded registry
/// says the gap lives: read, edit the matched block, replace the file.
/// A document whose matched block is not witnessed (and carries no
/// witness line) is refused (`Ok(false)` / `Err`), never silently
/// rewritten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileGapWitnessDetacher;

impl GapWitnessDetacher for FileGapWitnessDetacher {
    fn detach(&self, path: &Path, id: &str) -> Result<bool, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let Some(edited) = detach_witness_block(&text, id) else {
            return Ok(false);
        };
        std::fs::write(path, edited)
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        Ok(true)
    }
}

/// Operates on owned in-memory documents (tests and programmatic
/// consumers); the in-memory twin of [`FileGapWitnessDetacher`]. Every
/// successful edit is recorded in [`Self::edits`] for the audit.
#[derive(Debug, Default)]
pub struct InMemoryGapWitnessDetacher {
    /// The documents the detacher operates on, keyed by path.
    pub doc: std::cell::RefCell<std::collections::HashMap<PathBuf, String>>,
    /// Every `(path, gap ID)` detach performed, in order.
    pub edits: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl GapWitnessDetacher for InMemoryGapWitnessDetacher {
    fn detach(&self, path: &Path, id: &str) -> Result<bool, String> {
        let mut doc = self.doc.borrow_mut();
        let Some(text) = doc.get_mut(path) else {
            return Ok(false);
        };
        let Some(edited) = detach_witness_block(text, id) else {
            return Ok(false);
        };
        *text = edited;
        self.edits
            .borrow_mut()
            .push((path.to_path_buf(), id.to_string()));
        Ok(true)
    }
}

/// The surgical edit behind both detachers: find the `[[gap]]` block
/// whose `id` line carries the exact ID, flip its `status` value from
/// `"witnessed"` to `"unwitnessed"`, and remove the aligned
/// `witness = "..."` line. Every other line (the curated header prose,
/// the sibling blocks) passes through byte-for-byte. Returns `None` when
/// no witnessed (or witness-carrying) block carries the ID — the drift
/// signal the engine maps to a typed error.
///
/// The `status` line's key column is preserved exactly (`status    = "unwitnessed"`
/// keeps the padded key spelling the registry style carries), so a
/// `gaps witness` followed by `gaps unwitness` round-trips the registry
/// bytes.
fn detach_witness_block(text: &str, id: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
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
            let witnessed = block.iter().any(|entry| {
                block_key(entry) == Some("status") && block_value(entry) == Some("witnessed")
            });
            let carries_witness = block
                .iter()
                .any(|entry| block_key(entry) == Some("witness"));
            if !witnessed && !carries_witness {
                // Already unwitnessed (or a malformed status): the
                // engine's load pass rejects the former and the registry
                // schema rejects the latter — either way, no silent
                // rewrite.
                return None;
            }
            for existing in block {
                if block_key(existing) == Some("witness") {
                    // The witness line is dropped entirely: the detached
                    // path retires to the git history of the registry,
                    // the same contract the retire face states for
                    // closed entries.
                    continue;
                }
                if block_key(existing) == Some("status")
                    && block_value(existing) == Some("witnessed")
                {
                    let (key_part, _) = existing.split_once('=')?;
                    out.push(format!("{key_part}= \"unwitnessed\""));
                    continue;
                }
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

/// Result of a `gaps unwitness` run (also the
/// `glyim.ledger-gaps-unwitness/1` JSON face via
/// [`format_gaps_unwitness_json`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GapsUnwitnessOutcome {
    /// Schema tag, always [`GAPS_UNWITNESS_SCHEMA`].
    pub schema: &'static str,
    /// The detached gap's ID.
    pub id: String,
    /// The gap's registry title (echoed for the report line).
    pub title: String,
    /// The gap's class (`a` diagnosed, `b` silent, `c` deferred).
    pub class: String,
    /// The witness path the block carried (now removed from the
    /// registry); `None` when the block claimed `status = "witnessed"`
    /// without a witness line (the audit face's bare-block finding).
    pub witness: Option<String>,
    /// The registry file the `[[gap]]` block was edited in.
    pub target: String,
    /// The generated document the dispatch layer refreshes after the
    /// rewrite (`<ROOT>/KNOWN_GAPS.md`) — the face never leaves the
    /// committed render silently drifted from the registry.
    pub doc_target: String,
}

/// Detaches one gap's witness: validates the request, refuses to touch
/// anything unless the registry loads cleanly, and edits exactly the one
/// matching `[[gap]]` block through `detacher`.
///
/// # Validation contract
///
/// - `id` must be usable (non-blank, no control characters);
/// - the registry must load with zero error-severity diagnostics (a
///   registry that does not parse is never rewritten);
/// - the gap must be present and witnessed (an unwitnessed gap has
///   nothing to detach — the retire-face unknown-ID rule for a no-op
///   request).
///
/// Nothing is written when validation fails.
///
/// # Errors
///
/// Typed [`LedgerCliError`] variants for every rejected request
/// (`GapsUnwitnessBadId`, `GapsUnwitnessRegistry`,
/// `GapsUnwitnessUnknownId`, `GapsUnwitnessNotWitnessed`) and for a
/// failed rewrite (`GapsUnwitnessWrite`).
pub fn run_gaps_unwitness(
    root: &Path,
    request: &GapsUnwitnessRequest,
    detacher: &dyn GapWitnessDetacher,
) -> Result<GapsUnwitnessOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "gaps_unwitness",
        root = %root.display(),
        id = %request.id,
    );
    let _enter = span.enter();

    if request.id.trim().is_empty() || request.id.chars().any(char::is_control) {
        return Err(LedgerCliError::GapsUnwitnessBadId {
            id: request.id.clone(),
        });
    }

    // The registry is the source of truth for both the gap lookup and
    // the file the block lives in; a load that does not parse cleanly is
    // refused before anything is written (the witness-face load rule).
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
        return Err(LedgerCliError::GapsUnwitnessRegistry { detail });
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
        return Err(LedgerCliError::GapsUnwitnessUnknownId {
            id: request.id.clone(),
            candidates,
        });
    };
    if sourced.entry.status != GapStatus::Witnessed && sourced.entry.witness.is_none() {
        return Err(LedgerCliError::GapsUnwitnessNotWitnessed {
            id: request.id.clone(),
            path: sourced.path.display().to_string(),
        });
    }

    let target = sourced.path.clone();
    let detached = detacher.detach(&target, &request.id).map_err(|detail| {
        LedgerCliError::GapsUnwitnessWrite {
            path: target.display().to_string(),
            detail,
        }
    })?;
    if !detached {
        // The loaded registry carried the gap but the document no longer
        // matches the witnessed shape: the registry drifted between the
        // load pass and the rewrite. Fail without a partial write.
        return Err(LedgerCliError::GapsUnwitnessUnknownId {
            id: request.id.clone(),
            candidates: Vec::new(),
        });
    }
    tracing::debug!(
        id = %request.id,
        witness = ?sourced.entry.witness,
        "gap witness detached"
    );

    Ok(GapsUnwitnessOutcome {
        schema: GAPS_UNWITNESS_SCHEMA,
        id: request.id.clone(),
        title: sourced.entry.title.clone(),
        class: sourced.entry.class.as_str().to_string(),
        witness: sourced.entry.witness.clone(),
        target: target.display().to_string(),
        doc_target: root
            .join(crate::render::KNOWN_GAPS_MD_TARGET)
            .display()
            .to_string(),
    })
}

/// Renders the unwitness outcome's text face: one summary line (the
/// detached gap, its former fixture, and both touched paths).
#[must_use]
pub fn format_gaps_unwitness_text(outcome: &GapsUnwitnessOutcome) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "unwitnessed {} ({}) detaching {} -> {} rewritten, refresh {}",
        outcome.id,
        outcome.class,
        outcome.witness.as_deref().unwrap_or("no witness line"),
        outcome.target,
        outcome.doc_target,
    );
    text
}

/// Renders the `glyim.ledger-gaps-unwitness/1` JSON contract.
#[must_use]
pub fn format_gaps_unwitness_json(outcome: &GapsUnwitnessOutcome) -> String {
    let mut text = serde_json::to_string_pretty(outcome)
        .expect("the glyim.ledger-gaps-unwitness/1 document is a serialisation-only struct");
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
        FileGapOpener, FileGapRetirer, FileGapWitnessDetacher, FileGapWitnessRewriter,
        GAPS_AUDIT_SCHEMA, GAPS_OPEN_SCHEMA, GapAuditVerdict, GapOpener, GapRetirer,
        GapWitnessDetacher, GapWitnessRewriter, GapsFormat, GapsOpenOutcome, GapsOpenRequest,
        GapsRetireRequest, GapsUnwitnessRequest, GapsWitnessRequest, InMemoryGapOpener,
        InMemoryGapRetirer, InMemoryGapWitnessDetacher, format_gaps_audit_json,
        format_gaps_audit_text, format_gaps_open_json, format_gaps_open_text,
        format_gaps_unwitness_json, format_gaps_unwitness_text, run_gaps_audit, run_gaps_open,
        run_gaps_retire, run_gaps_unwitness, run_gaps_witness,
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

    /// The exact expected bytes after detaching GAP-0001's witness: the
    /// matched block's `status` value flipped back and the `witness`
    /// line removed (GAP-0001 carries honest witness debt again, GAP-0002
    /// untouched).
    const CURATED_UNWITNESSED: &str = "# Curated header prose (M1.1 seed) the rewriter must preserve\n\
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
         status    = \"unwitnessed\"\n\
         \n\
         [[gap]]\n\
         id        = \"GAP-0002\"\n\
         class     = \"b\"\n\
         title     = \"no lowering site sets cleanup\"\n\
         affects   = [\"LANG-DEMO-CLEANUP\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status    = \"unwitnessed\"\n";

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

    fn unwitness_request(id: &str) -> GapsUnwitnessRequest {
        GapsUnwitnessRequest { id: id.to_string() }
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

    // -- open face ---------------------------------------------------------------

    /// The canonical `[[gap]]` block the open face appends for a b-class
    /// gap capping two demo features: keys aligned to the committed
    /// registry style (`opened_in` sets the column), explicit
    /// `unwitnessed` status (a fresh gap is never silent debt).
    const OPEN_BLOCK: &str = "[[gap]]\n\
         id        = \"GAP-0003\"\n\
         class     = \"b\"\n\
         title     = \"no lowering site sets cleanup edges\"\n\
         affects   = [\"LANG-DEMO-AWAIT\", \"LANG-DEMO-CLEANUP\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.4.0\"\n\
         status    = \"unwitnessed\"\n";

    fn open_request(id: &str, title: &str) -> GapsOpenRequest {
        GapsOpenRequest {
            id: id.to_string(),
            class: ledger_schema::GapClass::B,
            title: title.to_string(),
            affects: vec!["LANG-DEMO-AWAIT".to_string()],
            caps: ledger_schema::Level::L2,
            opened_in: "v0.4.0".to_string(),
            file: None,
        }
    }

    #[test]
    fn file_opener_appends_the_canonical_block_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("0001-seed.toml");
        std::fs::write(&path, CURATED_SEED).expect("write seed");
        let block = OPEN_BLOCK.trim_end_matches('\n').to_string();
        let opened = FileGapOpener
            .open(&path, "GAP-0003", &block)
            .expect("the append lands");
        assert!(opened);
        let text = std::fs::read_to_string(&path).expect("read back");
        // The curated prose and both sibling blocks pass through
        // byte-for-byte; the new block lands after one blank separator.
        assert_eq!(text, format!("{CURATED_SEED}\n{OPEN_BLOCK}"));
    }

    #[test]
    fn file_opener_rejects_an_existing_id_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("0001-seed.toml");
        std::fs::write(&path, CURATED_SEED).expect("write seed");
        let before = std::fs::read_to_string(&path).expect("read seed");
        let opened = FileGapOpener
            .open(&path, "GAP-0002", "irrelevant")
            .expect("the guard is not an error");
        assert!(!opened);
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), before);
    }

    #[test]
    fn in_memory_opener_records_the_append() {
        let in_memory = InMemoryGapOpener::default();
        in_memory
            .doc
            .borrow_mut()
            .insert(PathBuf::from("memory.toml"), CURATED_SEED.to_string());
        let opened = in_memory
            .open(Path::new("memory.toml"), "GAP-0003", "block")
            .expect("the append lands");
        assert!(opened);
        assert_eq!(
            in_memory.opened.borrow().clone(),
            vec![(PathBuf::from("memory.toml"), "GAP-0003".to_string())]
        );
        // A document that already carries the block (the canonical block
        // the engine renders always opens with the [[gap]] header and the
        // id line) is the drift signal, not a second append.
        let drifted = in_memory
            .open(
                Path::new("memory.toml"),
                "GAP-0001",
                "[[gap]]\nid        = \"GAP-0001\"\n",
            )
            .expect("the guard is not an error");
        assert!(!drifted);
        assert_eq!(in_memory.opened.borrow().len(), 1);
    }

    #[test]
    fn run_gaps_open_appends_the_gap_and_reports_the_target() {
        let (dir, _seed) = seeded_root();
        let root = dir.path().to_path_buf();
        let request = open_request("GAP-0003", "no lowering site sets cleanup edges");
        let outcome = run_gaps_open(&root, &request, &FileGapOpener).expect("the open lands");
        assert_eq!(outcome.schema, GAPS_OPEN_SCHEMA);
        assert_eq!(outcome.id, "GAP-0003");
        assert_eq!(outcome.class, "b");
        assert_eq!(outcome.affects, vec!["LANG-DEMO-AWAIT"]);
        assert_eq!(outcome.caps, "L2");
        assert_eq!(outcome.opened_in, "v0.4.0");
        assert!(
            outcome
                .target
                .ends_with("ledger/gaps/known/0002-gaps-open.toml"),
            "the default target is the next numbered registry file, got {}",
            outcome.target
        );
        assert_eq!(
            outcome.doc_target,
            root.join(crate::render::KNOWN_GAPS_MD_TARGET)
                .display()
                .to_string()
        );
        // The registry is the source of truth: it re-loads cleanly with
        // the new entry, still unwitnessed (witness debt is explicit).
        let loaded = ledger_schema::load_project_registry(&root);
        assert_eq!(loaded.registry.gap_count(), 3);
        let opened = loaded
            .registry
            .gaps()
            .iter()
            .find(|sourced| sourced.entry.id == "GAP-0003")
            .expect("the opened gap is in the registry");
        assert_eq!(opened.entry.class, ledger_schema::GapClass::B);
        assert_eq!(opened.entry.status, GapStatus::Unwitnessed);
        assert!(opened.entry.witness.is_none());
        assert_eq!(opened.entry.affects, vec!["LANG-DEMO-AWAIT"]);
    }

    #[test]
    fn run_gaps_open_rejects_a_duplicate_id_without_writing() {
        let (dir, seed) = seeded_root();
        let root = dir.path().to_path_buf();
        let before = std::fs::read_to_string(&seed).expect("read seed");
        let request = open_request("GAP-0001", "duplicate");
        let err = run_gaps_open(&root, &request, &FileGapOpener).expect_err("duplicate id");
        assert!(matches!(
            err,
            LedgerCliError::GapsOpenDuplicateId { ref id, .. } if id == "GAP-0001"
        ));
        if let Some(help) = miette::Diagnostic::help(&err) {
            assert!(help.to_string().contains("GAP-0001"));
        }
        assert_eq!(std::fs::read_to_string(&seed).expect("read back"), before);
    }

    #[test]
    fn run_gaps_open_rejects_an_unknown_affects_feature() {
        let (dir, _seed) = seeded_root();
        let root = dir.path().to_path_buf();
        let mut request = open_request("GAP-0003", "phantom affects");
        request.affects = vec!["LANG-NOPE".to_string()];
        let err = run_gaps_open(&root, &request, &FileGapOpener).expect_err("unknown affects");
        assert!(matches!(err, LedgerCliError::GapsOpenAffectsUnknown { .. }));
    }

    #[test]
    fn run_gaps_open_rejects_a_bad_id_shape() {
        let (dir, _seed) = seeded_root();
        let root = dir.path().to_path_buf();
        for id in ["GAP-3", "OPEN-0003", "gap-0003", "GAP-00031", ""] {
            let request = open_request(id, "bad shape");
            let err = run_gaps_open(&root, &request, &FileGapOpener)
                .expect_err("a non-canonical ID is rejected");
            assert!(matches!(err, LedgerCliError::GapsOpenBadId { .. }));
        }
    }

    #[test]
    fn run_gaps_open_rejects_a_blank_or_multiline_title() {
        let (dir, _seed) = seeded_root();
        let root = dir.path().to_path_buf();
        for title in ["", "   ", "line one\nline two"] {
            let request = open_request("GAP-0003", title);
            let err = run_gaps_open(&root, &request, &FileGapOpener)
                .expect_err("a unusable title is rejected");
            assert!(matches!(err, LedgerCliError::GapsOpenBadTitle { .. }));
        }
    }

    #[test]
    fn run_gaps_open_rejects_a_registry_that_does_not_load() {
        let (dir, _seed) = seeded_root();
        let root = dir.path().to_path_buf();
        write_file(
            &root,
            "ledger/gaps/known/0002-broken.toml",
            "[[gap]]\nunknown_key = 1\n",
        );
        let request = open_request("GAP-0003", "broken registry");
        let err =
            run_gaps_open(&root, &request, &FileGapOpener).expect_err("broken registry refused");
        assert!(matches!(err, LedgerCliError::GapsOpenRegistry { .. }));
    }

    #[test]
    fn run_gaps_open_appends_to_an_explicit_registry_file() {
        let (dir, seed) = seeded_root();
        let root = dir.path().to_path_buf();
        let mut request = open_request("GAP-0003", "explicit target");
        request.file = Some("ledger/gaps/known/0001-seed.toml".to_string());
        let outcome = run_gaps_open(&root, &request, &FileGapOpener).expect("the open lands");
        assert_eq!(outcome.target, seed.display().to_string());
        let text = std::fs::read_to_string(&seed).expect("read back");
        // The curated prose and both sibling blocks survive byte-for-byte;
        // the new block is appended after one blank separator.
        assert!(text.starts_with(CURATED_SEED));
        assert!(text.contains("id        = \"GAP-0003\""));
        assert!(
            !root.join("ledger/gaps/known/0002-gaps-open.toml").exists(),
            "the default numbered file is not minted when --file names one"
        );
    }

    #[test]
    fn run_gaps_open_rejects_an_unusable_explicit_file() {
        let (dir, _seed) = seeded_root();
        let root = dir.path().to_path_buf();
        let cases = [
            "ledger/features/lang/a.toml",     // outside ledger/gaps/
            "../escape.toml",                  // .. escape
            "ledger/gaps/known/missing.toml",  // not an existing file
            "ledger/gaps/known/0001-seed.txt", // not a .toml
        ];
        for file in cases {
            let mut request = open_request("GAP-0003", "bad file");
            request.file = Some(file.to_string());
            let err = run_gaps_open(&root, &request, &FileGapOpener)
                .expect_err("an unusable --file is rejected");
            assert!(matches!(err, LedgerCliError::GapsOpenBadFile { .. }));
        }
    }

    #[test]
    fn run_gaps_open_escapes_quotes_in_the_title() {
        let (dir, _seed) = seeded_root();
        let root = dir.path().to_path_buf();
        let request = open_request("GAP-0003", "quotes \" and backslash \\ stay");
        run_gaps_open(&root, &request, &FileGapOpener).expect("the open lands");
        let loaded = ledger_schema::load_project_registry(&root);
        let opened = loaded
            .registry
            .gaps()
            .iter()
            .find(|sourced| sourced.entry.id == "GAP-0003")
            .expect("the opened gap re-parses");
        assert_eq!(opened.entry.title, "quotes \" and backslash \\ stay");
    }

    #[test]
    fn format_gaps_open_text_names_the_opened_gap() {
        let outcome = GapsOpenOutcome {
            schema: GAPS_OPEN_SCHEMA,
            id: "GAP-0003".to_string(),
            class: "b".to_string(),
            title: "no lowering site sets cleanup edges".to_string(),
            affects: vec!["LANG-DEMO-AWAIT".to_string()],
            caps: "L2".to_string(),
            opened_in: "v0.4.0".to_string(),
            target: "ledger/gaps/known/0002-gaps-open.toml".to_string(),
            doc_target: "KNOWN_GAPS.md".to_string(),
        };
        let text = format_gaps_open_text(&outcome);
        assert!(text.contains("opened GAP-0003"));
        assert!(text.contains("(b)"));
        assert!(text.contains("0002-gaps-open.toml"));
        assert!(text.contains("KNOWN_GAPS.md"));
    }

    #[test]
    fn format_gaps_open_json_parses_back_with_schema() {
        let outcome = GapsOpenOutcome {
            schema: GAPS_OPEN_SCHEMA,
            id: "GAP-0003".to_string(),
            class: "b".to_string(),
            title: "no lowering site sets cleanup edges".to_string(),
            affects: vec!["LANG-DEMO-AWAIT".to_string()],
            caps: "L2".to_string(),
            opened_in: "v0.4.0".to_string(),
            target: "ledger/gaps/known/0002-gaps-open.toml".to_string(),
            doc_target: "KNOWN_GAPS.md".to_string(),
        };
        let json = format_gaps_open_json(&outcome);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("the JSON contract parses");
        assert_eq!(parsed["schema"].as_str(), Some("glyim.ledger-gaps-open/1"));
        assert_eq!(parsed["id"].as_str(), Some("GAP-0003"));
        assert_eq!(parsed["affects"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn both_gaps_open_formatters_render_twice_byte_identical() {
        let outcome = GapsOpenOutcome {
            schema: GAPS_OPEN_SCHEMA,
            id: "GAP-0003".to_string(),
            class: "b".to_string(),
            title: "no lowering site sets cleanup edges".to_string(),
            affects: vec!["LANG-DEMO-AWAIT".to_string()],
            caps: "L2".to_string(),
            opened_in: "v0.4.0".to_string(),
            target: "ledger/gaps/known/0002-gaps-open.toml".to_string(),
            doc_target: "KNOWN_GAPS.md".to_string(),
        };
        assert_eq!(
            format_gaps_open_text(&outcome),
            format_gaps_open_text(&outcome)
        );
        assert_eq!(
            format_gaps_open_json(&outcome),
            format_gaps_open_json(&outcome)
        );
    }

    #[test]
    fn gaps_open_error_variants_carry_code_and_help() {
        let variants = [
            LedgerCliError::GapsOpenBadId {
                id: "GAP-3".to_string(),
            },
            LedgerCliError::GapsOpenBadTitle {
                title: String::new(),
            },
            LedgerCliError::GapsOpenBadOpenedIn {
                opened_in: String::new(),
            },
            LedgerCliError::GapsOpenBadFile {
                file: "../escape.toml".to_string(),
            },
            LedgerCliError::GapsOpenAffectsUnknown {
                affects: vec!["LANG-NOPE".to_string()],
                known: Vec::new(),
            },
            LedgerCliError::GapsOpenDuplicateId {
                id: "GAP-0001".to_string(),
                path: "ledger/gaps/known/0001-seed.toml".to_string(),
            },
            LedgerCliError::GapsOpenRegistry {
                detail: "boom".to_string(),
            },
            LedgerCliError::GapsOpenWrite {
                path: "ledger/gaps/known/0002-gaps-open.toml".to_string(),
                detail: "io".to_string(),
            },
        ];
        for err in &variants {
            let code = miette::Diagnostic::code(err)
                .expect("every typed error carries a code")
                .to_string();
            assert!(
                code.starts_with("glyim-ledger::gaps_open_"),
                "unexpected code {code}"
            );
            assert!(
                miette::Diagnostic::help(err).is_some(),
                "missing help() on {code}"
            );
        }
    }

    #[test]
    fn usage_lists_gaps_open() {
        assert!(usage().contains("gaps open"));
    }

    #[test]
    fn parse_args_accepts_gaps_open() {
        let command = parse_args(args(&[
            "gaps",
            "open",
            "--id",
            "GAP-0003",
            "--class",
            "b",
            "--title",
            "no cleanup",
            "--affects",
            "LANG-DEMO-AWAIT,LANG-DEMO-CLEANUP",
            "--caps",
            "L2",
            "--opened-in",
            "v0.4.0",
        ]))
        .expect("the flags parse");
        assert!(matches!(
            command,
            crate::Command::GapsOpen {
                root,
                ref id,
                class,
                ref title,
                ref affects,
                caps,
                ref opened_in,
                file: None,
                format: GapsFormat::Text,
            } if root.as_path() == Path::new(".")
                && id == "GAP-0003"
                && class == ledger_schema::GapClass::B
                && title == "no cleanup"
                && affects
                    == &[
                        "LANG-DEMO-AWAIT".to_string(),
                        "LANG-DEMO-CLEANUP".to_string()
                    ]
                && caps == ledger_schema::Level::L2
                && opened_in == "v0.4.0"
        ));
    }

    #[test]
    fn parse_args_accepts_gaps_open_with_root_and_file_and_format() {
        let command = parse_args(args(&[
            "gaps",
            "open",
            "/tmp/root",
            "--id",
            "GAP-0003",
            "--class",
            "c",
            "--title",
            "t",
            "--affects",
            "LANG-DEMO-AWAIT",
            "--caps",
            "L2",
            "--opened-in",
            "v0.4.0",
            "--file",
            "known/0001-seed.toml",
            "--format",
            "json",
        ]))
        .expect("the flags parse");
        assert!(matches!(
            command,
            crate::Command::GapsOpen {
                root,
                file: Some(ref file),
                format: GapsFormat::Json,
                ..
            } if root.as_path() == Path::new("/tmp/root") && file == "known/0001-seed.toml"
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_open_missing_flags() {
        for argv in [
            vec!["gaps", "open"], // everything missing
            vec![
                "gaps",
                "open",
                "--id",
                "GAP-0003",
                "--class",
                "b",
                "--title",
                "t",
                "--affects",
                "LANG-DEMO-AWAIT",
                "--caps",
                "L2",
            ], // --opened-in missing
        ] {
            let err = parse_args(args(&argv)).expect_err("a required flag is missing");
            assert!(matches!(err, LedgerCliError::GapsOpenMissingField { .. }));
        }
    }

    #[test]
    fn parse_args_rejects_gaps_open_bad_class_and_caps() {
        let err = parse_args(args(&[
            "gaps",
            "open",
            "--id",
            "GAP-0003",
            "--class",
            "d",
            "--title",
            "t",
            "--affects",
            "LANG-DEMO-AWAIT",
            "--caps",
            "L2",
            "--opened-in",
            "v0.4.0",
        ]))
        .expect_err("d is not a gap class");
        assert!(matches!(err, LedgerCliError::GapsOpenBadClass { .. }));
        let err = parse_args(args(&[
            "gaps",
            "open",
            "--id",
            "GAP-0003",
            "--class",
            "b",
            "--title",
            "t",
            "--affects",
            "LANG-DEMO-AWAIT",
            "--caps",
            "L9",
            "--opened-in",
            "v0.4.0",
        ]))
        .expect_err("L9 is not a readiness level");
        assert!(matches!(err, LedgerCliError::GapsOpenBadCaps { .. }));
    }

    #[test]
    fn parse_args_rejects_gaps_open_bad_format() {
        let err = parse_args(args(&[
            "gaps",
            "open",
            "--id",
            "GAP-0003",
            "--class",
            "b",
            "--title",
            "t",
            "--affects",
            "LANG-DEMO-AWAIT",
            "--caps",
            "L2",
            "--opened-in",
            "v0.4.0",
            "--format",
            "yaml",
        ]))
        .expect_err("yaml is not a face format");
        assert!(matches!(err, LedgerCliError::GapsBadFormat { .. }));
    }

    // -- gaps audit face -------------------------------------------------------

    /// A witnessed `[[gap]]` entry whose fixture does not exist: the
    /// silent-debt shape the audit face exists to surface (the registry
    /// claims a witness the tree no longer carries).
    const GONE_WITNESS_SEED: &str = "[[gap]]\n\
         id        = \"GAP-0003\"\n\
         class     = \"c\"\n\
         title     = \"witnessed against a fixture the tree no longer carries\"\n\
         affects   = [\"LANG-DEMO-AWAIT\"]\n\
         caps      = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status    = \"witnessed\"\n\
         witness   = \"fixtures/gone-known-gap.g\"\n";

    /// Seeds the standard root plus a third, witness-less-on-disk gap in
    /// a second registry file (the audit face reads the whole registry,
    /// not one file).
    fn seeded_root_with_gone_witness() -> tempfile::TempDir {
        let (dir, _gap_file) = seeded_root();
        write_file(
            dir.path(),
            "ledger/gaps/known/0002-gone.toml",
            GONE_WITNESS_SEED,
        );
        dir
    }

    #[test]
    fn run_gaps_audit_classifies_witnessed_debt_and_missing() {
        let dir = seeded_root_with_gone_witness();
        let outcome = run_gaps_audit(dir.path());
        assert_eq!(outcome.schema, GAPS_AUDIT_SCHEMA);
        assert_eq!(outcome.gaps, 3, "every registered gap is audited");
        assert_eq!(outcome.witnessed, 1, "GAP-0001's fixture exists");
        assert_eq!(outcome.debt, 1, "GAP-0002 is honest witness debt");
        assert_eq!(outcome.missing, 1, "GAP-0003's fixture is gone");
        assert_eq!(outcome.unreadable, 0);
        let verdicts: Vec<&str> = outcome
            .rows
            .iter()
            .map(|row| row.verdict.as_str())
            .collect();
        assert_eq!(
            verdicts,
            vec!["witnessed", "debt", "missing"],
            "rows are sorted by gap ID: {:?}",
            outcome.rows
        );
        assert!(
            !outcome.is_ok(),
            "a missing witness fixture fails the audit gate"
        );
    }

    #[test]
    fn run_gaps_audit_passes_when_witness_debt_is_the_only_finding() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_audit(dir.path());
        assert_eq!(outcome.missing, 0);
        assert_eq!(outcome.unreadable, 0);
        assert_eq!(outcome.debt, 1, "GAP-0002 is unwitnessed");
        assert!(
            outcome.is_ok(),
            "witness debt is displayed, never failed (the gaps-report rule)"
        );
    }

    #[test]
    fn run_gaps_audit_rows_echo_registry_metadata_and_name_the_gone_path() {
        let dir = seeded_root_with_gone_witness();
        let outcome = run_gaps_audit(dir.path());
        let witnessed = outcome
            .rows
            .iter()
            .find(|row| row.id == "GAP-0001")
            .expect("GAP-0001 is audited");
        assert_eq!(witnessed.verdict, GapAuditVerdict::Witnessed);
        assert_eq!(
            witnessed.witness.as_deref(),
            Some("fixtures/await-loop-known-gap.g")
        );
        assert!(witnessed.detail.is_none(), "an ok row carries no detail");
        let debt = outcome
            .rows
            .iter()
            .find(|row| row.id == "GAP-0002")
            .expect("GAP-0002 is audited");
        assert_eq!(debt.verdict, GapAuditVerdict::Debt);
        assert!(debt.witness.is_none(), "an unwitnessed gap has no path");
        let missing = outcome
            .rows
            .iter()
            .find(|row| row.id == "GAP-0003")
            .expect("GAP-0003 is audited");
        assert_eq!(missing.verdict, GapAuditVerdict::Missing);
        assert_eq!(
            missing.witness.as_deref(),
            Some("fixtures/gone-known-gap.g")
        );
        let detail = missing
            .detail
            .as_deref()
            .expect("a missing row names the path that vanished");
        assert!(
            detail.contains("fixtures/gone-known-gap.g"),
            "detail names the vanished fixture: {detail}"
        );
    }

    #[test]
    fn run_gaps_audit_surfaces_a_witnessed_status_without_a_witness_line() {
        // The serde default for a missing `status` key is `witnessed`, so
        // a block that carries neither a `witness` line nor an explicit
        // `status` silently claims a witness. The audit face is where
        // that silent debt becomes visible.
        let (dir, _gap_file) = seeded_root();
        write_file(
            dir.path(),
            "ledger/gaps/known/0002-bare.toml",
            "[[gap]]\n\
                 id        = \"GAP-0004\"\n\
                 class     = \"c\"\n\
                 title     = \"claims witnessed with no witness line\"\n\
                 affects   = [\"LANG-DEMO-AWAIT\"]\n\
                 caps      = \"L2\"\n\
                 opened_in = \"v0.3.0\"\n",
        );
        let outcome = run_gaps_audit(dir.path());
        let bare = outcome
            .rows
            .iter()
            .find(|row| row.id == "GAP-0004")
            .expect("GAP-0004 is audited");
        assert_eq!(bare.verdict, GapAuditVerdict::Missing);
        assert!(
            bare.detail
                .as_deref()
                .expect("the bare block is explained")
                .contains("no witness path recorded"),
            "detail explains the bare witnessed block: {:?}",
            bare.detail
        );
        assert!(!outcome.is_ok());
    }

    #[test]
    fn run_gaps_audit_flags_a_witness_path_that_is_not_a_file() {
        let (dir, _gap_file) = seeded_root();
        write_file(
            dir.path(),
            "ledger/gaps/known/0002-dir.toml",
            &GONE_WITNESS_SEED.replace("fixtures/gone-known-gap.g", "fixtures/a-directory"),
        );
        write_file(dir.path(), "fixtures/a-directory/keep.txt", "x\n");
        let outcome = run_gaps_audit(dir.path());
        let row = outcome
            .rows
            .iter()
            .find(|row| row.id == "GAP-0003")
            .expect("GAP-0003 is audited");
        assert_eq!(row.verdict, GapAuditVerdict::Unreadable);
        assert!(!outcome.is_ok());
    }

    #[test]
    fn run_gaps_audit_reports_load_diagnostics_and_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(
            dir.path(),
            "ledger/gaps/broken.toml",
            "[[gap]]\nid = \"nope\",\n",
        );
        let outcome = run_gaps_audit(dir.path());
        assert!(
            !outcome.diagnostics.is_empty(),
            "the load diagnostics surface in the audit face"
        );
        assert!(!outcome.is_ok());
    }

    #[test]
    fn format_gaps_audit_text_renders_summary_missing_rows_and_debt() {
        let dir = seeded_root_with_gone_witness();
        let outcome = run_gaps_audit(dir.path());
        let text = format_gaps_audit_text(&outcome);
        assert!(
            text.contains("gaps audit: 3 gap(s), 1 witnessed, 1 debt, 1 missing"),
            "summary line: {text}"
        );
        assert!(
            text.contains("FAILED"),
            "a missing fixture fails the gate: {text}"
        );
        assert!(
            text.contains("GAP-0003") && text.contains("missing"),
            "the missing row is rendered: {text}"
        );
        assert!(
            text.contains("witness debt"),
            "debt stays displayed, never silent: {text}"
        );
    }

    #[test]
    fn format_gaps_audit_json_parses_back_with_schema_and_counts() {
        let dir = seeded_root_with_gone_witness();
        let outcome = run_gaps_audit(dir.path());
        let text = format_gaps_audit_json(&outcome);
        let back: serde_json::Value = serde_json::from_str(&text).expect("json parses");
        assert_eq!(
            back["schema"].as_str(),
            Some("glyim.ledger-gaps-audit/1"),
            "the contract tag round-trips"
        );
        assert_eq!(back["gaps"].as_u64(), Some(3));
        assert_eq!(back["witnessed"].as_u64(), Some(1));
        assert_eq!(back["debt"].as_u64(), Some(1));
        assert_eq!(back["missing"].as_u64(), Some(1));
        assert_eq!(back["passed"].as_bool(), Some(false));
        let rows = back["rows"].as_array().expect("rows render");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2]["verdict"].as_str(), Some("missing"));
        let rendered = format_gaps_audit_json(&outcome);
        assert_eq!(rendered, text, "the JSON face is deterministic");
    }

    #[test]
    fn parse_args_accepts_gaps_audit() {
        let command = parse_args(args(&["gaps", "audit", "/tmp/reg", "--format", "json"]))
            .expect("the flags parse");
        assert!(matches!(
            command,
            crate::Command::GapsAudit { root, format: GapsFormat::Json }
            if root.as_path() == Path::new("/tmp/reg")
        ));
        let command = parse_args(args(&["gaps", "audit"])).expect("the bare face parses");
        assert!(matches!(
            command,
            crate::Command::GapsAudit { ref root, format: GapsFormat::Text }
            if root.as_path() == Path::new(".")
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_audit_bad_format() {
        let err = parse_args(args(&["gaps", "audit", "--format", "yaml"]))
            .expect_err("yaml is not a face format");
        assert!(matches!(err, LedgerCliError::GapsBadFormat { .. }));
    }

    #[test]
    fn usage_lists_the_gaps_audit_face() {
        let text = usage();
        assert!(
            text.contains("gaps audit [ROOT]"),
            "usage must list the audit face: {text}"
        );
    }

    // -- gaps unwitness face ---------------------------------------------------

    #[test]
    fn file_detacher_edits_only_the_matched_block_bytes() {
        let (dir, gap_file) = seeded_root();
        let _ = dir;
        let edited = FileGapWitnessDetacher
            .detach(&gap_file, "GAP-0001")
            .expect("the rewrite succeeds");
        assert!(edited, "the witnessed block was edited");
        let on_disk = std::fs::read_to_string(&gap_file).expect("read back");
        // The detach is the exact inverse of the witness face on this
        // seed: GAP-0001 returns to `status = "unwitnessed"` with the
        // `witness` line removed — honest witness debt again.
        assert_eq!(on_disk, CURATED_UNWITNESSED, "only the block changed");
    }

    #[test]
    fn file_detacher_returns_false_for_an_unknown_id_and_writes_nothing() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let edited = FileGapWitnessDetacher
            .detach(&gap_file, "GAP-9999")
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
    fn file_detacher_returns_false_for_an_already_unwitnessed_block() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let edited = FileGapWitnessDetacher
            .detach(&gap_file, "GAP-0002")
            .expect("an unwitnessed block is not an IO error");
        assert!(!edited, "the unwitnessed block must not be re-edited");
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
        let _ = dir;
    }

    #[test]
    fn in_memory_detacher_records_edits() {
        let in_memory = InMemoryGapWitnessDetacher::default();
        in_memory
            .doc
            .borrow_mut()
            .insert(PathBuf::from("memory.toml"), CURATED_SEED.to_string());
        let edited = in_memory
            .detach(Path::new("memory.toml"), "GAP-0001")
            .expect("the in-memory edit lands");
        assert!(edited);
        assert_eq!(
            in_memory.edits.borrow().len(),
            1,
            "the audit trail records the edit"
        );
        let detached = in_memory.doc.borrow()[std::path::Path::new("memory.toml")].clone();
        assert!(
            detached.contains("status    = \"unwitnessed\""),
            "the status flipped in the owned document: {detached}"
        );
        assert!(
            !detached.contains("witness   = "),
            "the witness line is gone from the owned document: {detached}"
        );
    }

    #[test]
    fn run_gaps_unwitness_restores_witness_debt() {
        let (dir, gap_file) = seeded_root();
        let root = dir.path().to_path_buf();
        let outcome = run_gaps_unwitness(
            &root,
            &unwitness_request("GAP-0001"),
            &FileGapWitnessDetacher,
        )
        .expect("the detach lands");
        assert_eq!(outcome.schema, "glyim.ledger-gaps-unwitness/1");
        assert_eq!(outcome.id, "GAP-0001");
        assert_eq!(outcome.class, "a");
        assert_eq!(
            outcome.witness.as_deref(),
            Some("fixtures/await-loop-known-gap.g"),
            "the outcome names the detached witness"
        );
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
        assert_eq!(on_disk, CURATED_UNWITNESSED, "only the block changed");
        // The rewritten file re-loads cleanly and reports the gap as
        // unwitnessed: the remedy restores honest witness debt.
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
            .find(|gap| gap.id == "GAP-0001")
            .expect("GAP-0001 still present");
        assert_eq!(gap.status, GapStatus::Unwitnessed);
        assert_eq!(gap.witness.as_deref(), None, "the witness line is gone");
    }

    #[test]
    fn run_gaps_unwitness_detaches_a_witnessed_status_without_a_witness_line() {
        let (dir, _gap_file) = seeded_root();
        write_file(
            dir.path(),
            "ledger/gaps/known/0002-bare.toml",
            "[[gap]]\n\
                 id        = \"GAP-0004\"\n\
                 class     = \"c\"\n\
                 title     = \"claims witnessed with no witness line\"\n\
                 affects   = [\"LANG-DEMO-AWAIT\"]\n\
                 caps      = \"L2\"\n\
                 opened_in = \"v0.3.0\"\n\
                 status    = \"witnessed\"\n",
        );
        let outcome = run_gaps_unwitness(
            dir.path(),
            &unwitness_request("GAP-0004"),
            &FileGapWitnessDetacher,
        )
        .expect("the detach lands");
        assert_eq!(
            outcome.witness, None,
            "a bare witnessed block detaches to plain debt"
        );
        let loaded = ledger_schema::load_project_registry(dir.path());
        let gap = loaded
            .registry
            .gaps()
            .iter()
            .map(|sourced| &sourced.entry)
            .find(|gap| gap.id == "GAP-0004")
            .expect("GAP-0004 still present");
        assert_eq!(gap.status, GapStatus::Unwitnessed);
    }

    #[test]
    fn witness_then_unwitness_round_trips_the_registry_bytes() {
        let (dir, gap_file) = seeded_root();
        let root = dir.path().to_path_buf();
        run_gaps_witness(
            &root,
            &request("GAP-0002", "fixtures/cleanup-known-gap.g"),
            &FileGapWitnessRewriter,
        )
        .expect("the witness lands");
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("witnessed"),
            CURATED_WITNESSED,
            "the witness face rewrote the block"
        );
        run_gaps_unwitness(
            &root,
            &unwitness_request("GAP-0002"),
            &FileGapWitnessDetacher,
        )
        .expect("the detach lands");
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("detached"),
            CURATED_SEED,
            "the two faces are exact inverses on this registry"
        );
    }

    #[test]
    fn run_gaps_unwitness_rejects_unknown_id_without_writing() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let err = run_gaps_unwitness(
            dir.path(),
            &unwitness_request("GAP-9999"),
            &FileGapWitnessDetacher,
        )
        .expect_err("unknown ID is a typed rejection");
        assert!(matches!(err, LedgerCliError::GapsUnwitnessUnknownId { .. }));
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
    }

    #[test]
    fn run_gaps_unwitness_rejects_an_unwitnessed_gap() {
        let (dir, gap_file) = seeded_root();
        let before = std::fs::read_to_string(&gap_file).expect("read before");
        let err = run_gaps_unwitness(
            dir.path(),
            &unwitness_request("GAP-0002"),
            &FileGapWitnessDetacher,
        )
        .expect_err("an unwitnessed gap has nothing to detach");
        assert!(matches!(
            err,
            LedgerCliError::GapsUnwitnessNotWitnessed { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(&gap_file).expect("read after"),
            before,
            "nothing was written"
        );
    }

    #[test]
    fn run_gaps_unwitness_rejects_a_blank_id() {
        let (dir, _gap_file) = seeded_root();
        let err = run_gaps_unwitness(dir.path(), &unwitness_request(""), &FileGapWitnessDetacher)
            .expect_err("a blank ID can never match");
        assert!(matches!(err, LedgerCliError::GapsUnwitnessBadId { .. }));
    }

    #[test]
    fn run_gaps_unwitness_rejects_a_registry_that_does_not_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(
            dir.path(),
            "ledger/gaps/broken.toml",
            "[[gap]]\nid = \"nope\",\n",
        );
        let err = run_gaps_unwitness(
            dir.path(),
            &unwitness_request("GAP-0001"),
            &FileGapWitnessDetacher,
        )
        .expect_err("a non-clean registry is refused before any write");
        assert!(matches!(err, LedgerCliError::GapsUnwitnessRegistry { .. }));
    }

    #[test]
    fn format_gaps_unwitness_text_names_the_detached_witness() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_unwitness(
            dir.path(),
            &unwitness_request("GAP-0001"),
            &FileGapWitnessDetacher,
        )
        .expect("the detach lands");
        let text = format_gaps_unwitness_text(&outcome);
        assert!(
            text.contains("unwitnessed GAP-0001"),
            "text names the gap: {text}"
        );
        assert!(
            text.contains("fixtures/await-loop-known-gap.g"),
            "text names the detached fixture: {text}"
        );
        assert!(
            text.contains("KNOWN_GAPS.md"),
            "text names the refreshed document: {text}"
        );
    }

    #[test]
    fn format_gaps_unwitness_json_parses_back_with_schema() {
        let (dir, _gap_file) = seeded_root();
        let outcome = run_gaps_unwitness(
            dir.path(),
            &unwitness_request("GAP-0001"),
            &FileGapWitnessDetacher,
        )
        .expect("the detach lands");
        let text = format_gaps_unwitness_json(&outcome);
        let back: serde_json::Value = serde_json::from_str(&text).expect("json parses");
        assert_eq!(
            back["schema"].as_str(),
            Some("glyim.ledger-gaps-unwitness/1"),
            "the contract tag round-trips"
        );
        assert_eq!(back["id"].as_str(), Some("GAP-0001"));
        assert_eq!(
            back["witness"].as_str(),
            Some("fixtures/await-loop-known-gap.g")
        );
        let rendered = format_gaps_unwitness_json(&outcome);
        assert_eq!(rendered, text, "the JSON face is deterministic");
    }

    #[test]
    fn parse_args_accepts_gaps_unwitness() {
        let command = parse_args(args(&["gaps", "unwitness", "--id", "GAP-0001", "/tmp/reg"]))
            .expect("the flags parse");
        assert!(matches!(
            command,
            crate::Command::GapsUnwitness { root, id, format: GapsFormat::Text }
            if root.as_path() == Path::new("/tmp/reg") && id == "GAP-0001"
        ));
        let command = parse_args(args(&[
            "gaps",
            "unwitness",
            "--id",
            "GAP-0002",
            "--format",
            "json",
        ]))
        .expect("the default root parses");
        assert!(matches!(
            command,
            crate::Command::GapsUnwitness { ref root, format: GapsFormat::Json, .. }
            if root.as_path() == Path::new(".")
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_unwitness_missing_id() {
        let err = parse_args(args(&["gaps", "unwitness"]))
            .expect_err("the required --id flag is missing");
        assert!(matches!(
            err,
            LedgerCliError::GapsUnwitnessMissingField { .. }
        ));
    }

    #[test]
    fn parse_args_rejects_gaps_unwitness_bad_format() {
        let err = parse_args(args(&[
            "gaps",
            "unwitness",
            "--id",
            "GAP-0001",
            "--format",
            "yaml",
        ]))
        .expect_err("yaml is not a face format");
        assert!(matches!(err, LedgerCliError::GapsBadFormat { .. }));
    }

    #[test]
    fn usage_lists_the_gaps_unwitness_face() {
        let text = usage();
        assert!(
            text.contains("gaps unwitness [ROOT]"),
            "usage must list the unwitness face: {text}"
        );
    }

    /// The parse-args helper every CLI test in the crate shares (the
    /// `lib.rs` tests' `args` shape).
    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items.iter().map(|s| (*s).to_string())
    }
}
