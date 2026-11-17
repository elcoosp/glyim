//! Save-time edit support (feature tag LSP-WILL-SAVE).
//!
//! The server already reformats a document on demand (`textDocument/
//! formatting`, LSP-FORMAT; a selected span, LSP-RANGE-FORMAT; a
//! just-typed bracket, LSP-ON-TYPE-FORMAT), but nothing answered the
//! *save* question: `textDocument/willSaveWaitUntil` did not exist, so a
//! client with format-on-save enabled had no wire answer for "which
//! edits should land before this document hits the disk". Every input
//! the request needs was already stored — the driver's `SourceMap`
//! carries the document text and the same UTF-16 line/column mapping
//! every other request maps through (INF-11/INF-12), and the shared
//! [`crate::formatting::format_source`] core defines what "saved form"
//! means. This module closes that capability gap with no re-analysis on
//! the request path: format-on-save composes with the same one formatter
//! the other three wire shapes read, so a saved file and a manually
//! formatted file agree by construction.
//!
//! The seam: *which text the save consults* is split from *which edits
//! the save answers* by [`SaveFormatSource`], mirroring
//! LSP-RANGE-FORMAT's `RangeFormatSource`. The policy
//! ([`compute_save_edits`]) reads through the trait and stays testable
//! without a database, source map, or lock (`tests/will_save.rs`); the
//! production [`StoredSaveFormatSource`] borrows the driver's stored
//! document text — the same store the full formatter reads. A future
//! source over an unsaved editor buffer would slot in behind the same
//! seam without touching the policy.
//!
//! What the policy decides, over the stored text: the whole document is
//! run through the shared formatter core (base depth 0, exactly the
//! full-document request), and the saved form is diffed against the
//! stored form at *line-with-terminator* granularity — each chunk is the
//! line plus its `\n`, so equal chunk runs are byte-equal runs by
//! construction. The common prefix and suffix runs stay outside the
//! edit: a save after a small edit answers one edit over the changed
//! region, never a whole-document rewrite, so cursor, folding state, and
//! undo granularity outside the changed region survive the save. One
//! normalization rides ahead of the diff: the formatter always
//! terminates its last line, so a stored text whose content already
//! matches but lacks the final `\n` answers a zero-width insertion at
//! EOF — never a rewrite of the last line. An already-formatted document
//! answers `None` (the same no-churn contract the full formatter
//! honours), and at most one edit is ever answered.
//!
//! The params' save `reason` is deliberately not consulted: the
//! formatter is reason-agnostic, and refining per reason would make the
//! save shape disagree with the other formatting shapes over the same
//! shared core.
//!
//! Wire mapping (LSP spec): the edit becomes one `TextEdit` whose range
//! maps the claimed byte span through the same `SourceMap` the request
//! offsets mapped from, so the bounds and the new text can never
//! disagree. A span whose bytes no longer map answers `null`, never a
//! guessed position.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::formatting::format_source;
use lsp_types::{TextEdit, WillSaveTextDocumentParams};
use url::Url;

/// One save edit, wire-type-free: the claimed byte span `[start, end)`
/// in the stored text and the replacement text the saved form puts
/// there. An empty span with a non-empty `new_text` is an insertion; a
/// non-empty span with an empty `new_text` is a deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveEdit {
    /// The claimed span's first byte.
    pub start: usize,
    /// One past the claimed span's last byte.
    pub end: usize,
    /// The replacement the saved form puts at the claimed span.
    pub new_text: String,
}

/// The save-time source seam (LSP-WILL-SAVE).
///
/// The policy in [`compute_save_edits`] must stay testable without a
/// live `AnalysisDatabase` (locks, source maps), so it reads the
/// document text through this trait. The production
/// [`StoredSaveFormatSource`] borrows the driver's stored document text;
/// a future source over an unsaved editor buffer would slot in behind
/// the same seam without touching the policy.
pub trait SaveFormatSource {
    /// The whole document text the save would rewrite, in the order the
    /// formatter walks it.
    fn document_text(&self) -> &str;
}

/// Production [`SaveFormatSource`]: borrows one file's stored document
/// text for one request.
///
/// Borrowing (not cloning) keeps the answer allocation-free apart from
/// the returned edit; the `db.source_maps` read guard lives in the
/// caller, exactly like the range-formatting handler.
pub struct StoredSaveFormatSource<'a> {
    text: &'a str,
}

impl<'a> StoredSaveFormatSource<'a> {
    /// Bind the source to one file's stored text for one request.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }
}

impl SaveFormatSource for StoredSaveFormatSource<'_> {
    fn document_text(&self) -> &str {
        self.text
    }
}

/// Split `text` into line-with-terminator chunks: every chunk carries
/// its `\n` except possibly the last, so concatenating the chunks
/// reproduces the text byte-for-byte. Equal chunk strings therefore make
/// byte-equal runs, which is the invariant the prefix/suffix trim below
/// leans on.
fn inclusive_lines(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while let Some(idx) = rest.find('\n') {
        chunks.push(&rest[..=idx]);
        rest = &rest[idx + 1..];
    }
    if !rest.is_empty() {
        chunks.push(rest);
    }
    chunks
}

/// The byte offset each chunk starts at (one entry per chunk plus the
/// terminal EOF offset, so the span of chunks `[p, m)` is
/// `[starts[p], starts[m])` for every `m` up to the chunk count).
fn chunk_starts(chunks: &[&str]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(chunks.len() + 1);
    let mut offset = 0;
    for chunk in chunks {
        starts.push(offset);
        offset += chunk.len();
    }
    starts.push(offset);
    starts
}

/// The save-time policy: the edit that brings the stored text to the
/// shared formatter core's saved form, with the unchanged head and tail
/// runs kept outside the claimed span.
///
/// An already-formatted document answers `None` (no edit churn — the
/// full formatter's contract), and a stored text that only lacks the
/// final newline answers a zero-width insertion at EOF. At most one
/// edit is ever answered.
#[must_use]
pub fn compute_save_edits(source: &dyn SaveFormatSource) -> Option<SaveEdit> {
    let text = source.document_text();
    let formatted = format_source(text, 0);
    if formatted == text {
        // Already formatted — no edit churn.
        return None;
    }
    // The formatter always terminates its last line. A stored text whose
    // content already matches but lacks the final terminator differs only
    // by that byte: one zero-width insertion at EOF, never a rewrite of
    // the last line.
    if !text.is_empty() && formatted.as_str() == concat_string(text, "\n") {
        return Some(SaveEdit {
            start: text.len(),
            end: text.len(),
            new_text: "\n".to_string(),
        });
    }
    let stored = inclusive_lines(text);
    let ready = inclusive_lines(&formatted);
    let stored_starts = chunk_starts(&stored);
    let ready_starts = chunk_starts(&ready);

    // The unchanged head: equal line-with-terminator chunks from the top.
    let mut head = 0;
    let max_head = stored.len().min(ready.len());
    while head < max_head && stored[head] == ready[head] {
        head += 1;
    }
    // The unchanged tail: equal chunks from the bottom, never entering
    // the head.
    let mut tail = 0;
    while tail < stored.len() - head
        && tail < ready.len() - head
        && stored[stored.len() - 1 - tail] == ready[ready.len() - 1 - tail]
    {
        tail += 1;
    }

    let start = stored_starts[head];
    let end = stored_starts[stored.len() - tail];
    let new_text = formatted[ready_starts[head]..ready_starts[ready.len() - tail]].to_string();
    if start == end && new_text.is_empty() {
        // Both middles empty means the chunk runs were equal, which the
        // equality check above already answered — belt and braces, never
        // an edit with no effect.
        return None;
    }
    Some(SaveEdit {
        start,
        end,
        new_text,
    })
}

/// `text + suffix` without a second allocation-shaped helper import —
/// the normalization above only ever appends one newline.
fn concat_string(text: &str, suffix: &str) -> String {
    let mut out = String::with_capacity(text.len() + suffix.len());
    out.push_str(text);
    out.push_str(suffix);
    out
}

/// Map one edit to the wire `TextEdit`, with the span→position mapping
/// injected (`None` when the claimed span no longer maps — one unmappable
/// edit must not become a guessed position).
#[must_use]
pub fn wire_save_edit(
    edit: &SaveEdit,
    map: &dyn Fn(usize, usize) -> Option<((usize, usize), (usize, usize))>,
) -> Option<TextEdit> {
    let ((start_line, start_col), (end_line, end_col)) = map(edit.start, edit.end)?;
    Some(TextEdit {
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: start_line as u32,
                character: start_col as u32,
            },
            end: lsp_types::Position {
                line: end_line as u32,
                character: end_col as u32,
            },
        },
        new_text: edit.new_text.clone(),
    })
}

/// `textDocument/willSaveWaitUntil` request entry point (LSP-WILL-SAVE).
///
/// Resolution order mirrors the other formatting shapes: URI → open-file
/// lookup → the stored-text policy over the shared formatter core → the
/// wire edit. Returns `None` when the document is not open or is already
/// formatted; callers answer `null` for that. The save `reason` is
/// deliberately not consulted (see the module docs).
#[must_use]
pub fn will_save_edits(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &WillSaveTextDocumentParams,
) -> Option<Vec<TextEdit>> {
    let path = Url::parse(params.text_document.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file_id = file_map.get_by_path(&path)?;

    // The text borrow and the wire mapping share one read guard: the
    // edit is claimed and mapped while the guard lives, exactly like the
    // range-formatting handler.
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let source = StoredSaveFormatSource::new(sm.source());
    let edit = compute_save_edits(&source)?;
    let wire_edit = wire_save_edit(&edit, &|lo, hi| sm.span_to_position(lo, hi))?;
    drop(source_maps);
    tracing::debug!(file = %file_id.to_raw(), "save-time edits resolved");
    Some(vec![wire_edit])
}
