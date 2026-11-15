//! Semantic-tokens request support (feature tag LSP-SEMANTIC-TOKENS).
//!
//! The server already resolves names, occurrences, and types, but never
//! answered `textDocument/semanticTokens/full` — an editor could only
//! render the file through a static grammar, which knows keywords and
//! literals but cannot know that `render` is a function, `Rect` a type,
//! or `frame.w` a property read. This module closes that capability gap
//! on top of the parse trees the analysis driver already stores (the
//! same store LSP-SELECTION walks), so highlighting rides the real token
//! extents without a re-parse per request.
//!
//! The seam: *which legend class a token carries* is split from *which
//! tokens the file contains* by [`TokenLexSource`], mirroring
//! LSP-SELECTION's `AncestorSpanSource`. The policy
//! ([`token_type_for`]) reads raw lexical facts — span, token kind,
//! enclosing node kinds, the next token's kind — and stays testable
//! without a rowan tree, lock, or source map (`tests/semantic_tokens.rs`);
//! the production [`ParseTreeTokenSource`] walks the stored `GreenNode`
//! and yields exactly those facts. The delta encoding that turns
//! classified tokens into the spec's flat `u32` wire array lives in
//! [`encode_delta`]; `tests/semantic_tokens_tests.rs` covers the whole
//! chain end-to-end.
//!
//! The next-token fact earns its slot in the seam: paths nest as
//! `PathExpr`/`PathType` → `UsePath` → `Ident`, and the enclosing nodes
//! alone cannot separate a call's callee from its arguments (`paint(c)`
//! nests *both* under `CallExpr` → `PathExpr` → `UsePath`) — but the
//! callee is the one followed by `LParen`. The same fact separates a
//! struct's name from its field names (`Rect` is followed by `LBrace`,
//! each field by `Colon`), which matters because the parser nests
//! definition fields directly under `StructDef`.
//!
//! Wire mapping (LSP spec): tokens are encoded strictly in document
//! order as five `u32` slots each — delta line (relative to the previous
//! token's line, the first relative to line 0), delta start character
//! (relative to the previous token on the same line, otherwise relative
//! to the line start), UTF-16 length (INF-11: code units, not bytes),
//! legend index, and modifier bitset (always 0 — the syntactic policy
//! has no modifiers to claim). The legend is pinned by [`token_legend`];
//! the capability advertisement in `handler::build_router` and the
//! response indices must agree with it, which the conformance and
//! end-to-end tests pin from both sides.
//!
//! Classification honesty: only facts a syntactic walk can vouch for are
//! classified. Keywords, literals, comments, and declaration/binding
//! names take their class from the declaring node; a bare path is a
//! variable unless it is a call callee (followed by `(`) or a struct
//! literal's constructor name (followed by `{`). Punctuation, operators,
//! lifetimes, and identifier positions with no trustworthy rule are
//! *skipped* rather than guessed — the client's static grammar keeps
//! rendering those, so a skipped token degrades to existing behavior,
//! never to a wrong color.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::database::SourceMap;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use lsp_types::{
    SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensLegend, SemanticTokensParams,
    SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult,
};
use url::Url;

/// The legend indices, named so the policy reads as a table instead of
/// bare literals. The order must match [`TOKEN_TYPES`] exactly; the
/// policy-level tests pin both directions.
mod legend {
    pub(super) const KEYWORD: usize = 0;
    pub(super) const COMMENT: usize = 1;
    pub(super) const STRING: usize = 2;
    pub(super) const NUMBER: usize = 3;
    pub(super) const TYPE: usize = 4;
    pub(super) const FUNCTION: usize = 5;
    pub(super) const METHOD: usize = 6;
    pub(super) const PROPERTY: usize = 7;
    pub(super) const VARIABLE: usize = 8;
    pub(super) const PARAMETER: usize = 9;
    pub(super) const ENUM_MEMBER: usize = 10;
    pub(super) const MACRO: usize = 11;
}

/// The token-type legend in classification order — the single source of
/// truth for both the capability advertisement (`handler::build_router`
/// reads it through [`token_legend`]) and the indices the policy
/// returns. Appending is backward-compatible; reordering or inserting
/// would shift every response, so the policy tests pin the order.
pub const TOKEN_TYPES: [SemanticTokenType; 12] = [
    SemanticTokenType::KEYWORD,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::TYPE,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::MACRO,
];

/// The wire legend for the capability advertisement: [`TOKEN_TYPES`] and
/// no modifiers (the syntactic policy claims none).
#[must_use]
pub fn token_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: Vec::new(),
    }
}

/// The raw lexical facts one token contributes to the policy: its byte
/// span, its own kind, the kinds of the two enclosing nodes, and the
/// kind of the token that follows it.
///
/// `grandparent` carries the second enclosing node and `next_kind` the
/// following token (the sentinel [`SyntaxKind::SourceFile`] when the
/// walk runs out in either direction). Both exist because one policy
/// level cannot separate shapes the enclosing kinds leave ambiguous:
/// call position (`paint(`) versus a plain reference reads the *next*
/// token, and a struct's field names versus its own name reads the
/// enclosing nodes plus the next token together.
#[derive(Debug, Clone, Copy)]
pub struct LexedToken {
    /// Byte offset of the first byte of the token.
    pub lo: usize,
    /// Byte offset one past the last byte of the token.
    pub hi: usize,
    /// The token's own kind (never whitespace in practice; the
    /// production source skips it).
    pub kind: SyntaxKind,
    /// Kind of the immediate enclosing node.
    pub parent: SyntaxKind,
    /// Kind of the grandparent node (`SourceFile` sentinel at the root).
    pub grandparent: SyntaxKind,
    /// Kind of the following token (`SourceFile` sentinel at EOF). Note
    /// the lexer folds the trailing newline into line comments, so a
    /// comment's next token is the first token of the *next* line.
    pub next_kind: SyntaxKind,
}

/// The token-lexing seam for semantic-tokens requests
/// (LSP-SEMANTIC-TOKENS).
///
/// The classification policy in [`token_type_for`] must stay testable
/// without a live `AnalysisDatabase` (parse trees, locks, source maps),
/// so it reads raw lexical facts through this trait. The production
/// [`ParseTreeTokenSource`] walks the driver's stored parse tree; a
/// future partially-expanded or macro-aware token source would slot in
/// behind the same seam without touching the policy.
pub trait TokenLexSource {
    /// Every classified candidate of one file in source order, with
    /// whitespace skipped. Comments are *kept* — they are trivia for
    /// selection and navigation, but editors highlight them, so the
    /// semantic-tokens policy must see them.
    fn lexical_tokens(&self) -> Vec<LexedToken>;
}

/// Production [`TokenLexSource`]: borrows the driver's stored parse
/// tree for one file.
///
/// The store keeps the immutable [`GreenNode`] (rowan cursor nodes carry
/// raw parent pointers and are not `Send`, so they cannot live in the
/// shared database); each request materializes its own cursor root with
/// one `Arc` clone — the same trick LSP-SELECTION uses — instead of
/// re-parsing the file. `next_kind` comes from rowan's linear
/// `next_token()` walk, which crosses node boundaries (so a declaration
/// name sees the `(` that opens its parameter list).
pub struct ParseTreeTokenSource<'a> {
    green: &'a GreenNode,
}

impl<'a> ParseTreeTokenSource<'a> {
    /// Borrows `green` as the token source for one file's requests.
    #[must_use]
    pub fn new(green: &'a GreenNode) -> Self {
        Self { green }
    }
}

impl TokenLexSource for ParseTreeTokenSource<'_> {
    fn lexical_tokens(&self) -> Vec<LexedToken> {
        let root = SyntaxNode::new_root(self.green.clone());
        let tokens: Vec<_> = root
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() != SyntaxKind::Whitespace)
            .collect();
        tokens
            .iter()
            .enumerate()
            .map(|(i, token)| {
                let range = token.text_range();
                let parent = token.parent();
                let parent_kind = parent
                    .as_ref()
                    .map_or(SyntaxKind::SourceFile, |node| node.kind());
                let grandparent_kind = parent
                    .as_ref()
                    .and_then(|node| node.parent())
                    .map_or(SyntaxKind::SourceFile, |node| node.kind());
                let next_kind = tokens
                    .get(i + 1)
                    .map_or(SyntaxKind::SourceFile, |next| next.kind());
                LexedToken {
                    lo: u32::from(range.start()) as usize,
                    hi: u32::from(range.end()) as usize,
                    kind: token.kind(),
                    parent: parent_kind,
                    grandparent: grandparent_kind,
                    next_kind,
                }
            })
            .collect()
    }
}

/// Classification policy: the legend index for one lexed token, or
/// `None` when the walk cannot vouch for a class.
///
/// Lexical kinds classify without context (keyword, comment, string,
/// number). Identifiers take their class from the enclosing nodes and
/// the following token — the declaring node names the declaration, the
/// access node names the member, and call versus reference position is
/// the next token's shape. Everything else is skipped: punctuation,
/// operators, lifetimes, and identifier positions with no trustworthy
/// rule (use paths, macro token trees) fall back to the client's static
/// grammar instead of a wrong color.
#[must_use]
pub fn token_type_for(
    kind: SyntaxKind,
    parent: SyntaxKind,
    grandparent: SyntaxKind,
    next_kind: SyntaxKind,
) -> Option<usize> {
    use legend::{COMMENT, KEYWORD, NUMBER, STRING};
    match kind {
        SyntaxKind::LineComment | SyntaxKind::BlockComment | SyntaxKind::DocComment => {
            Some(COMMENT)
        }
        SyntaxKind::StringLit | SyntaxKind::CharLit | SyntaxKind::ByteLit => Some(STRING),
        SyntaxKind::IntLit | SyntaxKind::FloatLit => Some(NUMBER),
        // `true`/`false` lex as keywords (FE-7), so the boolean class is
        // covered here rather than by a literal arm.
        kind if kind.is_keyword() => Some(KEYWORD),
        SyntaxKind::Ident => ident_class(parent, grandparent, next_kind),
        _ => None,
    }
}

/// Identifier rule: the class an `Ident` inherits from its context.
///
/// Path idents sit under `UsePath` (the parser wraps every path segment
/// chain in one), so their rules read the grandparent — `PathType` is a
/// type position, `PathExpr` an expression position where the *next*
/// token tells call (`(`) and struct-literal (`{`) position apart from a
/// plain reference.
fn ident_class(
    parent: SyntaxKind,
    grandparent: SyntaxKind,
    next_kind: SyntaxKind,
) -> Option<usize> {
    use legend::{ENUM_MEMBER, FUNCTION, MACRO, METHOD, PARAMETER, PROPERTY, TYPE, VARIABLE};
    match parent {
        // Declarations: the name takes the declared kind. Definition
        // fields nest directly under `StructDef` (probe-verified tree
        // shape), and a field name is the ident followed by `:` — the
        // type name is followed by `{` or generics, never a colon.
        SyntaxKind::FnDef => Some(FUNCTION),
        SyntaxKind::StructDef | SyntaxKind::TypeAlias | SyntaxKind::ImplDef => {
            if next_kind == SyntaxKind::Colon {
                Some(PROPERTY)
            } else {
                Some(TYPE)
            }
        }
        SyntaxKind::TraitDef => Some(TYPE),
        SyntaxKind::EnumDef => Some(TYPE),
        SyntaxKind::EnumVariant => Some(ENUM_MEMBER),
        SyntaxKind::StructField => Some(PROPERTY),
        SyntaxKind::ConstDef | SyntaxKind::StaticDef => Some(VARIABLE),
        // Bindings and parameters.
        SyntaxKind::PatIdent => Some(VARIABLE),
        SyntaxKind::Param => Some(PARAMETER),
        // Member access: method name after `.`, field name after `.`.
        SyntaxKind::MethodCallExpr => Some(METHOD),
        SyntaxKind::FieldExpr => Some(PROPERTY),
        // GSX attribute names (`class="warn"`, `on:click={..}`).
        SyntaxKind::GsxAttr | SyntaxKind::GsxDirectiveAttr => Some(PROPERTY),
        // Path positions (the parser wraps path segments in `UsePath`).
        SyntaxKind::UsePath => match grandparent {
            SyntaxKind::PathType => Some(TYPE),
            SyntaxKind::PathExpr => match next_kind {
                // Call position: the callee is what `(` follows.
                SyntaxKind::LParen => Some(FUNCTION),
                // Struct-literal position: the constructor's shape name.
                SyntaxKind::LBrace => Some(TYPE),
                _ => Some(VARIABLE),
            },
            _ => None,
        },
        // Macro invocation name (`my_macro! { .. }`).
        SyntaxKind::MacroCall => Some(MACRO),
        _ => None,
    }
}

/// A classified token: byte extent plus its legend index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassifiedToken {
    /// Byte offset of the first byte of the token.
    pub lo: usize,
    /// Byte offset one past the last byte of the token.
    pub hi: usize,
    /// Index into [`TOKEN_TYPES`].
    pub token_type: usize,
}

/// Classification policy over a whole file: filter the source's tokens
/// through [`token_type_for`] and normalize the output.
///
/// Normalization (the part tests pin): empty spans are dropped (nothing
/// to highlight), and a token that rewinds before the previous kept
/// token's end is dropped, keeping the output strictly monotonic in
/// document order — the delta encoding below depends on that invariant,
/// and a defensive skip here turns any future source-order bug into a
/// missing token instead of a wire-level negative delta (which clients
/// would render as garbage).
#[must_use]
pub fn classify_tokens(source: &dyn TokenLexSource) -> Vec<ClassifiedToken> {
    let mut classified = Vec::new();
    let mut prev_hi = 0usize;
    for token in source.lexical_tokens() {
        if token.hi <= token.lo {
            continue;
        }
        if token.lo < prev_hi {
            continue;
        }
        let Some(token_type) =
            token_type_for(token.kind, token.parent, token.grandparent, token.next_kind)
        else {
            continue;
        };
        prev_hi = token.hi;
        classified.push(ClassifiedToken {
            lo: token.lo,
            hi: token.hi,
            token_type,
        });
    }
    classified
}

/// Delta-encode classified tokens into the spec's wire shape
/// (`SemanticToken` per entry; lsp-types serializes the vector as the
/// flat `u32` array clients expect).
///
/// All offsets are UTF-16 (INF-11): the start position comes from the
/// `SourceMap`'s UTF-16 column mapping and the length is the token
/// text's UTF-16 code-unit count, so a multi-byte character inside a
/// string literal shrinks the length rather than coloring past the end
/// of the token. Returns `None` when a position cannot be mapped (the
/// caller answers `null`, matching the other request engines).
#[must_use]
pub fn encode_delta(
    sm: &SourceMap,
    source: &str,
    tokens: &[ClassifiedToken],
) -> Option<Vec<SemanticToken>> {
    let mut prev_line = 0usize;
    let mut prev_col = 0usize;
    let mut wire = Vec::with_capacity(tokens.len());
    for token in tokens {
        // The start endpoint is all the wire needs; the length is
        // computed from the text below (a token never spans lines
        // except multi-line strings/comments, whose UTF-16 length is
        // still the whole token's).
        let (line, col) = sm.span_to_position(token.lo, token.hi)?.0;
        if token.hi > source.len() || !source.is_char_boundary(token.lo) {
            return None;
        }
        let length = source[token.lo..token.hi]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>();
        let delta_line = line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            col.saturating_sub(prev_col)
        } else {
            col
        };
        prev_line = line;
        prev_col = col;
        wire.push(SemanticToken {
            delta_line: u32::try_from(delta_line).ok()?,
            delta_start: u32::try_from(delta_start).ok()?,
            length: u32::try_from(length).ok()?,
            token_type: u32::try_from(token.token_type).ok()?,
            token_modifiers_bitset: 0,
        });
    }
    Some(wire)
}

/// `textDocument/semanticTokens/full` request entry point
/// (LSP-SEMANTIC-TOKENS).
///
/// Resolution order mirrors `selection_ranges`: URI → open-file lookup →
/// stored parse tree → classify → delta-encode. Returns `None` when the
/// document is not open, has no parse tree, or classifies no tokens
/// (an empty response would be legal, but `null` matches the other
/// engines' "nothing to answer" convention and saves the wire round
/// trip on files the policy skips entirely).
#[must_use]
pub fn semantic_tokens_full(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &SemanticTokensParams,
) -> Option<SemanticTokensResult> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let syntaxes = db.syntaxes.read();
    let root = syntaxes.get(&file_id)?;

    let classified = classify_tokens(&ParseTreeTokenSource::new(root));
    if classified.is_empty() {
        return None;
    }
    let data = encode_delta(sm, sm.source(), &classified)?;
    tracing::debug!(tokens = data.len(), "semantic tokens computed");
    Some(SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}

/// `textDocument/semanticTokens/range` request entry point
/// (LSP-SEMANTIC-TOKENS).
///
/// The range-scoped sibling of [`semantic_tokens_full`]: the same
/// classification over the same stored tree, restricted to the tokens
/// that *intersect* the requested screen range. Editors call this while
/// scrolling, and intersection (not containment) is what keeps a token
/// the view cuts through — a multi-line string straddling the viewport
/// edge — colored instead of dropped. Deltas restart from the first
/// returned token (the spec's encoding is relative to the response's
/// own first entry, which is why a subset re-encodes rather than slicing
/// the full response's array).
///
/// Returns `None` when the document is not open, has no parse tree, or
/// no token intersects the range; a malformed (reversed) range
/// degenerates to the empty intersection and answers `null` honestly.
#[must_use]
pub fn semantic_tokens_range(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &SemanticTokensRangeParams,
) -> Option<SemanticTokensRangeResult> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let syntaxes = db.syntaxes.read();
    let root = syntaxes.get(&file_id)?;

    // The wire range is (line, utf16 col) pairs; map both ends back to
    // byte offsets so the byte-extent tokens intersect-test directly.
    // A column past its line's end is unmappable (`line_col_to_offset`
    // answers `None` for it), and an unmappable endpoint leaves nothing
    // to intersect with.
    let range_lo = sm.line_col_to_offset(
        params.range.start.line as usize,
        params.range.start.character as usize,
    )?;
    let range_hi = sm.line_col_to_offset(
        params.range.end.line as usize,
        params.range.end.character as usize,
    )?;
    if range_hi <= range_lo {
        return None;
    }

    let classified = classify_tokens(&ParseTreeTokenSource::new(root));
    let visible: Vec<ClassifiedToken> = classified
        .into_iter()
        .filter(|token| token.hi > range_lo && token.lo < range_hi)
        .collect();
    if visible.is_empty() {
        return None;
    }
    let data = encode_delta(sm, sm.source(), &visible)?;
    tracing::debug!(tokens = data.len(), "semantic tokens range computed");
    Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}
