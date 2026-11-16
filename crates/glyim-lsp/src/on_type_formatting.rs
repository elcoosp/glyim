//! On-type-formatting request support (feature tag LSP-ON-TYPE-FORMAT).
//!
//! The full-document formatter (`formatting.rs`) answers
//! `textDocument/formatting` — but nothing answered the *keystroke*
//! question: the editor auto-indents the line a just-typed closing brace
//! lands on by guessing (usually copying the previous line's indent),
//! and for a closer that guess is wrong more often than not — the line
//! should sit at the matching opener's indentation. This module closes
//! that capability gap with `textDocument/onTypeFormatting`: one
//! whitespace run replacement per closing bracket keystroke, computed
//! from the recorded facts.
//!
//! The seam: *which bracket tokens exist and where* is split from *what
//! indent a closer should carry* by [`OnTypeTokenSource`], mirroring
//! LSP-DOC-LINK's `LinkLexSource`. The policy ([`on_type_indent`]) reads
//! through the trait and stays testable without a rowan tree or locks
//! (`tests/on_type_formatting.rs`); the production
//! [`ParseTreeOnTypeSource`] walks the driver's stored parse tree — the
//! same immutable `GreenNode` store document-link, selection-range and
//! semantic-tokens read, so no re-analysis happens on the request path
//! and the capability works on whatever the tree recorded (bracket
//! tokens survive parse recovery).
//!
//! Policy: the request fires *after* a character was typed (the params'
//! position is past it, `ch` names it), so the typed closer is already
//! in the document — the guard that the text right before the position
//! carries exactly `ch` keeps a desynced client from rewriting a line
//! the user did not type. The typed token is located in the bracket
//! stream, and a stack walk pairs it with the nearest unmatched opener
//! of its own family (`{`/`}`, `(`/`)`, `[`/`]`); unbalanced code, a
//! family mismatch (`(` opened, `}` typed) or a closer that is not in
//! the stream answer no edit — guessing an indent for broken shapes
//! would be dishonest. The replacement is the opener line's *leading
//! whitespace*, verbatim (spaces, tabs, any mix); a closer that already
//! sits at that indent, and a closer with code before it on its line,
//! answer no edit — a zero-diff rewrite would churn the buffer.
//!
//! Wire mapping (LSP spec): the answer is one `TextEdit` whose range is
//! the whitespace run between the typed line's start and the typed
//! character, and whose `new_text` is the opener line's indent.
//! Openers (`{`, `(`, `[` — not closers) and unknown characters answer
//! `null`, per the other engines' convention. The capability
//! advertises `}` (first trigger) plus `)` and `]` (more triggers).

use crate::AnalysisDatabase;
use crate::database::FileMap;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use lsp_types::{DocumentOnTypeFormattingParams, TextEdit};
use url::Url;

/// One bracket token, wire-type-free: kind and byte extent. Only the six
/// bracket kinds survive the production source's filter; the policy
/// matches families on the kind pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BracketToken {
    /// Byte offset of the token's first byte.
    pub lo: usize,
    /// Byte offset one past the token's last byte.
    pub hi: usize,
    /// Lexical kind (one of the six bracket kinds).
    pub kind: SyntaxKind,
}

impl BracketToken {
    /// The opener kind this closer kind pairs with; `None` for openers
    /// (and any non-bracket kind, though the source filters those out).
    fn opener_of(kind: SyntaxKind) -> Option<SyntaxKind> {
        match kind {
            SyntaxKind::RBrace => Some(SyntaxKind::LBrace),
            SyntaxKind::RParen => Some(SyntaxKind::LParen),
            SyntaxKind::RBracket => Some(SyntaxKind::LBracket),
            _ => None,
        }
    }

    /// The closer kind for a typed closing character, or `None` when the
    /// character is not a closing bracket (openers and other characters
    /// get no formatting).
    fn closer_of(ch: char) -> Option<SyntaxKind> {
        match ch {
            '}' => Some(SyntaxKind::RBrace),
            ')' => Some(SyntaxKind::RParen),
            ']' => Some(SyntaxKind::RBracket),
            _ => None,
        }
    }
}

/// The bracket-token seam for on-type-formatting requests
/// (LSP-ON-TYPE-FORMAT).
///
/// The indent policy in [`on_type_indent`] must stay testable without a
/// live `AnalysisDatabase` (parse trees, locks, source maps), so it reads
/// bracket facts through this trait. The production
/// [`ParseTreeOnTypeSource`] walks the driver's stored parse tree; a
/// future macro-expanded token source would slot in behind the same seam
/// without touching the policy.
pub trait OnTypeTokenSource {
    /// Every bracket token of one file in source order (whitespace and
    /// non-bracket kinds skipped).
    fn bracket_tokens(&self) -> Vec<BracketToken>;
}

/// Production [`OnTypeTokenSource`]: borrows the driver's stored parse
/// tree for one file.
///
/// The store keeps the immutable [`GreenNode`] (rowan cursor nodes carry
/// raw parent pointers and are not `Send`, so they cannot live in the
/// shared database); each request materializes its own cursor root with
/// one `Arc` clone — the same trick LSP-SELECTION and LSP-DOC-LINK use —
/// instead of re-parsing the file.
pub struct ParseTreeOnTypeSource<'a> {
    green: &'a GreenNode,
}

impl<'a> ParseTreeOnTypeSource<'a> {
    /// Borrows `green` as the bracket source for one file's requests.
    #[must_use]
    pub fn new(green: &'a GreenNode) -> Self {
        Self { green }
    }
}

impl OnTypeTokenSource for ParseTreeOnTypeSource<'_> {
    fn bracket_tokens(&self) -> Vec<BracketToken> {
        let root = SyntaxNode::new_root(self.green.clone());
        root.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::LBrace
                        | SyntaxKind::RBrace
                        | SyntaxKind::LParen
                        | SyntaxKind::RParen
                        | SyntaxKind::LBracket
                        | SyntaxKind::RBracket
                )
            })
            .map(|token| {
                let range = token.text_range();
                BracketToken {
                    lo: u32::from(range.start()) as usize,
                    hi: u32::from(range.end()) as usize,
                    kind: token.kind(),
                }
            })
            .collect()
    }
}

/// One computed indent fix, wire-type-free: the whitespace run to
/// replace and the replacement indent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnTypeIndent {
    /// Byte offset of the first byte of the typed line's whitespace run.
    pub indent_lo: usize,
    /// Byte offset one past the run's last byte (the typed character's
    /// first byte).
    pub indent_hi: usize,
    /// The replacement: the opener line's leading whitespace, verbatim.
    pub indent: String,
}

/// The leading whitespace of the line `offset` sits on: the run between
/// the previous line break (or the start of the text) and the first
/// non-whitespace byte at or before `offset`. Empty when the line starts
/// with content.
fn line_indent_before(text: &str, offset: usize) -> &str {
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let prefix = &text[line_start..offset];
    let content_start = prefix
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map_or(prefix.len(), |(index, _)| index);
    &prefix[..content_start]
}

/// On-type-formatting policy: the indent fix for a just-typed closing
/// bracket, or `None` when no honest fix exists.
///
/// `typed_lo` is the byte offset the typed character starts at, `ch` the
/// character itself. The walk pairs the typed closer with its opener
/// through a bracket stack (same family only); the replacement is the
/// opener line's leading whitespace. Answers `None` when `ch` is not a
/// closing bracket, the text before `typed_lo` does not carry `ch`, the
/// typed token is not in the stream, the stack is unbalanced, the
/// closer already sits at the right indent, or code precedes it on its
/// line.
#[must_use]
pub fn on_type_indent(
    source: &dyn OnTypeTokenSource,
    text: &str,
    typed_lo: usize,
    ch: char,
) -> Option<OnTypeIndent> {
    // Gate 1: the character must be a closer. Openers and other typed
    // characters get no formatting.
    let typed_kind = BracketToken::closer_of(ch)?;
    // Gate 2: the typed char must actually sit right before the reported
    // offset — a desynced client must not have us rewrite a line the
    // user did not type.
    let typed_hi = typed_lo.checked_add(ch.len_utf8())?;
    if text.get(typed_lo..typed_hi) != Some(ch.to_string().as_str()) {
        return None;
    }
    // Gate 3: the typed closer must exist in the token stream, as a
    // token of the character's own family.
    let brackets = source.bracket_tokens();
    let typed_index = brackets.iter().position(|token| token.lo == typed_lo)?;
    if brackets[typed_index].kind != typed_kind {
        return None;
    }
    // Gate 4: stack-pair every closer up to and including the typed one.
    // A closer with an empty stack, or whose family mismatches the stack
    // top, leaves the typed token's opener unvouchable — honest `None`.
    // The typed token's opener is the one its own pop hands back, so it
    // is captured inside the loop (`token.lo == typed_lo`).
    let mut stack: Vec<(SyntaxKind, usize)> = Vec::new();
    let mut opener_lo: Option<usize> = None;
    for token in brackets.iter().take(typed_index + 1) {
        if let Some(expected) = BracketToken::opener_of(token.kind) {
            // A closer token: it must pair with the stack top.
            let (kind, lo) = stack.pop()?;
            if kind != expected {
                return None;
            }
            if token.lo == typed_lo {
                opener_lo = Some(lo);
            }
        } else {
            // An opener token: push its kind and position.
            stack.push((token.kind, token.lo));
        }
    }
    let opener_lo = opener_lo?;
    // Gate 5: the typed line must carry only whitespace before the
    // closer — code before it means there is no indent run to fix.
    let line_start = text[..typed_lo].rfind('\n').map_or(0, |index| index + 1);
    if text[line_start..typed_lo]
        .chars()
        .any(|typed_line_ch| !typed_line_ch.is_whitespace())
    {
        return None;
    }
    // The replacement: the opener line's leading whitespace, verbatim.
    let indent = line_indent_before(text, opener_lo).to_string();
    if text[line_start..typed_lo] == indent {
        // Already correct: no edit (a zero-diff rewrite would churn the
        // buffer).
        return None;
    }
    Some(OnTypeIndent {
        indent_lo: line_start,
        indent_hi: typed_lo,
        indent,
    })
}

/// `textDocument/onTypeFormatting` request entry point
/// (LSP-ON-TYPE-FORMAT).
///
/// Resolution order mirrors `document_links`: URI → open-file lookup →
/// stored parse tree (no re-analysis) → the typed position mapped back
/// to bytes through the document's source map → indent policy → wire
/// edit. Returns `None` when the document is not open, its parse tree is
/// not stored, or the policy finds no honest fix; callers answer `null`
/// for that.
#[must_use]
pub fn on_type_formatting(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentOnTypeFormattingParams,
) -> Option<Vec<TextEdit>> {
    let uri = &params.text_document_position.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    // The position is *after* the typed character; the character itself
    // is the `ch` the client echoes.
    let ch = params.ch.chars().next()?;
    let green = {
        let syntaxes = db.syntaxes.read();
        syntaxes.get(&file_id)?.clone()
    };

    let (typed_lo, fix) = {
        let source_maps = db.source_maps.read();
        let sm = source_maps.get(&file_id)?;
        let offset = sm.line_col_to_offset(
            params.text_document_position.position.line as usize,
            params.text_document_position.position.character as usize,
        )?;
        let typed_lo = offset.checked_sub(ch.len_utf8())?;
        let fix = on_type_indent(
            &ParseTreeOnTypeSource::new(&green),
            sm.source(),
            typed_lo,
            ch,
        )?;
        (typed_lo, fix)
    };

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let ((start_line, start_col), (end_line, end_col)) =
        sm.span_to_position(fix.indent_lo, typed_lo)?;
    let edit = TextEdit {
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
        new_text: fix.indent,
    };
    tracing::debug!(
        line = edit.range.start.line,
        indent = %edit.new_text,
        "on-type indent fix computed"
    );
    Some(vec![edit])
}
