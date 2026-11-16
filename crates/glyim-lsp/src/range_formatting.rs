//! Range-formatting request support (feature tag LSP-RANGE-FORMAT).
//!
//! The server already reformats a whole document (`textDocument/formatting`,
//! LSP-FORMAT), but nothing answered the editor's *selection* question:
//! `textDocument/rangeFormatting` did not exist, so reformatting one
//! function or one block either reformatted the whole file or did not
//! happen. Every input the request needs was already recorded — the
//! driver's `SourceMap` carries the document text and the same UTF-16
//! line/column mapping every other request maps through (INF-11/INF-12).
//! This module closes that capability gap with no re-analysis on the
//! request path.
//!
//! The seam: *what the formatter does to a slice of text* is split from
//! *which text the request claims* by [`RangeFormatSource`], mirroring
//! LSP-INLINE-VALUE's `InlineValueSource`. The policy
//! ([`compute_range_edits`]) reads through the trait and stays testable
//! without a database, source map, or lock (`tests/range_formatting.rs`);
//! the production [`StoredRangeFormatSource`] borrows the driver's stored
//! document text — the same store the full formatter reads — so both wire
//! shapes are one formatter by construction. A future source over unsaved
//! buffers or diff fragments would slot in behind the same seam without
//! touching the policy.
//!
//! What the range decides, over the stored text: the requested byte range
//! (clamped, never panicking) expands to the *whole lines that strictly
//! overlap it* — a half-open overlap, so a selection ending at a line
//! start never claims that line — and the shared
//! [`crate::formatting::format_source`] core re-indents exactly that line
//! span, seeded with the brace balance of the unselected prefix. The
//! honest skips: an empty range claims no line, a range whose lines are
//! already formatted answers `None` (the same no-churn contract the full
//! formatter honours), and the edit always covers whole lines because a
//! mid-line splice would corrupt the surrounding indentation.
//!
//! Wire mapping (LSP spec): the edit becomes one `TextEdit` whose range
//! maps the claimed byte span through the same `SourceMap` the request
//! offsets mapped from, so the bounds and the new text can never
//! disagree. A range whose bytes no longer map answers `null`, never a
//! guessed position.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::formatting::format_source;
use lsp_types::{DocumentRangeFormattingParams, TextEdit};
use url::Url;

/// One line-span edit the request can answer with, wire-type-free: the
/// claimed byte span `[start, end)` and the re-indented replacement text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeEdit {
    /// The claimed span's first byte (a line start).
    pub start: usize,
    /// One past the claimed span's last byte (a line start or EOF).
    pub end: usize,
    /// The formatted replacement for the claimed span.
    pub new_text: String,
}

/// The range-formatting fact seam (LSP-RANGE-FORMAT).
///
/// The policy in [`compute_range_edits`] must stay testable without a
/// live `AnalysisDatabase` (locks, source maps), so it reads the document
/// text through this trait. The production [`StoredRangeFormatSource`]
/// borrows the driver's stored document text; a future source over
/// unsaved buffers would slot in behind the same seam without touching
/// the policy.
pub trait RangeFormatSource {
    /// The whole document text the request reformats, in the order the
    /// formatter walks it.
    fn document_text(&self) -> &str;
}

/// Production [`RangeFormatSource`]: borrows one file's stored document
/// text for one request.
///
/// Borrowing (not cloning) keeps the claim allocation-free apart from the
/// returned edit; the `db.source_maps` read guard lives in the caller,
/// exactly like the inline-value handler.
pub struct StoredRangeFormatSource<'a> {
    text: &'a str,
}

impl<'a> StoredRangeFormatSource<'a> {
    /// Bind the source to one file's stored text for one request.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }
}

impl RangeFormatSource for StoredRangeFormatSource<'_> {
    fn document_text(&self) -> &str {
        self.text
    }
}

/// The nesting depth the text before a claim leaves: the brace balance of
/// the unselected prefix, saturated at zero (an unbalanced prefix never
/// panics the request path).
fn prefix_depth(prefix: &str) -> usize {
    prefix.chars().fold(0usize, |depth, c| match c {
        '{' => depth + 1,
        '}' => depth.saturating_sub(1),
        _ => depth,
    })
}

/// The range-formatting policy: one edit per requested range — the whole
/// lines that strictly overlap `[lo, hi)`, re-indented at the depth the
/// unselected prefix leaves.
///
/// The range is clamped to the document and never panics; a reversed
/// request clamps to `[lo, lo)` and an empty range claims no line. An
/// already-formatted claim answers `None` (no edit churn), the same
/// contract the full-document formatter honours.
#[must_use]
pub fn compute_range_edits(
    source: &dyn RangeFormatSource,
    lo: usize,
    hi: usize,
) -> Option<RangeEdit> {
    let text = source.document_text();
    let lo = lo.min(text.len());
    let hi = hi.max(lo).min(text.len());
    if lo == hi {
        // An empty range claims no line — nothing to reformat.
        return None;
    }
    // First claimed line: the line containing `lo` (its start is always
    // < hi, and its end > lo by construction).
    let start = text[..lo].rfind('\n').map_or(0, |idx| idx + 1);
    // Last claimed line: the line containing `hi - 1` (every later line
    // starts at or after `hi`, so it cannot strictly overlap). Its end is
    // the byte after its newline: the newline sits at `hi - 1 + idx`, so
    // the claim ends at `hi + idx` — or at EOF for an unterminated line.
    let end = text[hi - 1..].find('\n').map_or(text.len(), |idx| hi + idx);
    let slice = &text[start..end];
    let new_text = format_source(slice, prefix_depth(&text[..start]));
    if new_text == slice {
        // Already formatted — no edit churn.
        return None;
    }
    Some(RangeEdit {
        start,
        end,
        new_text,
    })
}

/// Map one edit to the wire `TextEdit`, with the span→position mapping
/// injected (`None` when the claimed span no longer maps — one unmappable
/// edit must not become a guessed position).
#[must_use]
pub fn wire_range_formatting(
    edit: &RangeEdit,
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

/// `textDocument/rangeFormatting` request entry point (LSP-RANGE-FORMAT).
///
/// Resolution order mirrors the other range requests: URI → open-file
/// lookup → range offsets (byte-safe, INF-12) → the claimed-line policy
/// over the stored text → the wire edit. Returns `None` when the document
/// is not open, the range maps nowhere, or the claimed lines are already
/// formatted; callers answer `null` for that.
#[must_use]
pub fn range_formatting(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentRangeFormattingParams,
) -> Option<Vec<TextEdit>> {
    let path = Url::parse(params.text_document.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file_id = file_map.get_by_path(&path)?;

    // The request range's byte extents (INF-11: the same UTF-16 mapping
    // the responses map back through, so the offsets and the wire ranges
    // can never disagree).
    let (lo, hi) = {
        let source_maps = db.source_maps.read();
        let sm = source_maps.get(&file_id)?;
        let lo = sm.line_col_to_offset(
            params.range.start.line as usize,
            params.range.start.character as usize,
        )?;
        let hi = sm.line_col_to_offset(
            params.range.end.line as usize,
            params.range.end.character as usize,
        )?;
        (lo, hi)
    };

    // The text borrow and the wire mapping share one read guard: the edit
    // is claimed and mapped while the guard lives, exactly like the
    // inline-value handler.
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let source = StoredRangeFormatSource::new(sm.source());
    // An already-formatted claim answers `None` — the same no-churn
    // contract the full-document formatter honours.
    let edit = compute_range_edits(&source, lo, hi)?;
    let wire_edit = wire_range_formatting(&edit, &|lo, hi| sm.span_to_position(lo, hi))?;
    let claimed_lines = edit.new_text.lines().count();
    drop(source_maps);
    tracing::debug!(
        file = %file_id.to_raw(),
        lines = claimed_lines,
        "range formatting resolved"
    );
    Some(vec![wire_edit])
}
