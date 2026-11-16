//! Document-link request support (feature tag LSP-DOC-LINK).
//!
//! The server answers navigation questions about *names* (definition,
//! declaration, type-definition, implementation, references) — but a
//! Glyim source file can pull other files in *by path*, and nothing
//! answered that: the builtin file-taking macros (`include!`,
//! `include_str!`, `include_bytes!`, registered by the expander in
//! `crates/glyim-meta`) carry a string-literal argument naming a file
//! relative to the containing document, and no request turned that
//! literal into something an editor can ctrl-click. This module closes
//! that capability gap with `textDocument/documentLink`.
//!
//! The seam: *which token shapes are include-macro call sites, and what
//! each literal resolves to* is split from *where the tokens come from*
//! by [`LinkLexSource`], mirroring LSP-SEMANTIC-TOKENS'
//! `TokenLexSource`. The policy ([`link_targets`]) reads through the
//! trait and stays testable without a rowan tree or locks
//! (`tests/document_link.rs`); the production [`ParseTreeLinkSource`]
//! walks the driver's stored parse tree — the same immutable
//! `GreenNode` store selection-range and semantic-tokens read, so no
//! re-analysis happens on the request path and the capability works on
//! broken code (the tree predates lowering, exactly like LSP-SELECTION).
//!
//! What counts as a link site: the whole call shape
//! `include[_str|_bytes]!( <string-literal> )` — identifier, `!`, `(`,
//! `StringLit`, `)`. Partial shapes (missing `!`, a non-literal argument
//! such as `include!(concat!("a", "b"))`) and non-file macros
//! (`env!`, `option_env!`, `format!`, …) answer no link: the expansion
//! target is not knowable from source, so guessing would be dishonest.
//! The literal keeps its quotes in the token text (that is how the lexer
//! spells it), so the policy decodes the minimal escape set a path can
//! carry (`\\` and `\"`) and treats every other backslash pair as
//! literal source. Relative targets join the containing document's
//! directory; absolute targets are used as-is. A target that does not
//! exist yet still links — editors report the missing file when clicked,
//! which is the honest answer for a file that is about to be created.
//!
//! Wire mapping (LSP spec): each [`IncludeLink`] becomes one
//! `DocumentLink` whose range is the string-literal token's exact extent
//! (the clickable region, quotes included — the LSP-EXACT-EXTENTS
//! convention) and whose target is the `file://` URI of the resolved
//! path, with a tooltip naming the macro. Links come in source order
//! (the token-walk order), so the response is deterministic. The
//! response is `null` when the document is not open or carries no
//! include-macro call, per the other engines' convention.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use lsp_types::{DocumentLink, DocumentLinkParams};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;

/// The file-taking builtin macros a document link can point at
/// (LSP-DOC-LINK).
///
/// The set is exactly the expander's builtin file macros
/// (`crates/glyim-meta/src/expander/mod.rs`): `include!` splices a
/// source file into the tree, `include_str!` embeds its contents as a
/// string, `include_bytes!` embeds them as bytes. The stdlib-defined
/// declarative macros (`println!`, `panic!`, …) and the environment
/// builtins (`env!`, `option_env!`) are deliberately absent — their
/// arguments name values, not files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeMacroKind {
    /// `include!("path.g")` — splice a source file.
    Include,
    /// `include_str!("path.txt")` — embed file contents as a string.
    IncludeStr,
    /// `include_bytes!("path.bin")` — embed file contents as bytes.
    IncludeBytes,
}

impl IncludeMacroKind {
    /// The macro's source-level name (without the `!`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Include => "include",
            Self::IncludeStr => "include_str",
            Self::IncludeBytes => "include_bytes",
        }
    }

    /// The kind for an identifier token's text, if it names one of the
    /// file-taking builtins.
    fn from_ident(text: &str) -> Option<Self> {
        match text {
            "include" => Some(Self::Include),
            "include_str" => Some(Self::IncludeStr),
            "include_bytes" => Some(Self::IncludeBytes),
            _ => None,
        }
    }
}

/// One lexed token, wire-type-free: kind, byte extent, and verbatim text.
///
/// Text is carried (unlike LSP-SEMANTIC-TOKENS' `LexedToken`, which
/// classifies by kinds alone) because the link policy must read the
/// string literal's contents; the production source clones it from the
/// green node per token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkToken {
    /// Byte offset of the token's first byte.
    pub lo: usize,
    /// Byte offset one past the token's last byte.
    pub hi: usize,
    /// Lexical kind.
    pub kind: SyntaxKind,
    /// Verbatim token text (a string literal keeps its quotes).
    pub text: String,
}

/// The token-lexing seam for document-link requests (LSP-DOC-LINK).
///
/// The include-call policy in [`link_targets`] must stay testable
/// without a live `AnalysisDatabase` (parse trees, locks, source maps),
/// so it reads raw lexical facts through this trait. The production
/// [`ParseTreeLinkSource`] walks the driver's stored parse tree; a
/// future macro-expanded or partially-expanded token source would slot
/// in behind the same seam without touching the policy.
pub trait LinkLexSource {
    /// Every candidate token of one file in source order, with
    /// whitespace skipped. Comments are kept — they cannot complete a
    /// call shape, so the policy simply never matches across them.
    fn lexical_tokens(&self) -> Vec<LinkToken>;
}

/// Production [`LinkLexSource`]: borrows the driver's stored parse
/// tree for one file.
///
/// The store keeps the immutable [`GreenNode`] (rowan cursor nodes
/// carry raw parent pointers and are not `Send`, so they cannot live in
/// the shared database); each request materializes its own cursor root
/// with one `Arc` clone — the same trick LSP-SELECTION and
/// LSP-SEMANTIC-TOKENS use — instead of re-parsing the file. Whitespace
/// is skipped so the call-shape match is whitespace-insensitive
/// (`include!( "x.g" )` links like `include!("x.g")`).
pub struct ParseTreeLinkSource<'a> {
    green: &'a GreenNode,
}

impl<'a> ParseTreeLinkSource<'a> {
    /// Borrows `green` as the token source for one file's requests.
    #[must_use]
    pub fn new(green: &'a GreenNode) -> Self {
        Self { green }
    }
}

impl LinkLexSource for ParseTreeLinkSource<'_> {
    fn lexical_tokens(&self) -> Vec<LinkToken> {
        let root = SyntaxNode::new_root(self.green.clone());
        root.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() != SyntaxKind::Whitespace)
            .map(|token| {
                let range = token.text_range();
                LinkToken {
                    lo: u32::from(range.start()) as usize,
                    hi: u32::from(range.end()) as usize,
                    kind: token.kind(),
                    text: token.text().to_string(),
                }
            })
            .collect()
    }
}

/// One include-macro call site with its resolved target, wire-type-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeLink {
    /// Which builtin the site calls (drives the tooltip).
    pub macro_kind: IncludeMacroKind,
    /// Byte extent of the string-literal token (quotes included) — the
    /// clickable region on the wire.
    pub literal_lo: usize,
    /// One past the literal's last byte.
    pub literal_hi: usize,
    /// The resolved target path: the literal's contents joined onto the
    /// document's directory, or the untouched absolute path.
    pub target: PathBuf,
}

/// Decode a string-literal token's text into the path it spells.
///
/// Strips the surrounding quotes, decodes the two escapes a path can
/// realistically carry (`\\` → `\`, `\"` → `"`), and leaves any other
/// backslash pair as literal source (paths do not carry `\n`-style
/// escapes; silently inventing characters would be dishonest).
/// Returns `None` for an empty literal — there is nothing to link to.
fn literal_to_path(text: &str) -> Option<String> {
    let body = text.strip_prefix('"')?.strip_suffix('"')?;
    if body.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                // Unknown escape: keep the backslash and the escaped
                // character verbatim instead of guessing.
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

/// Resolve one include literal against the containing document's
/// directory. Absolute literals win; relative ones join `doc_dir`.
fn resolve_include_target(literal: &str, doc_dir: &Path) -> Option<PathBuf> {
    let spelled = literal_to_path(literal)?;
    let path = Path::new(&spelled);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(doc_dir.join(path))
    }
}

/// Document-link policy: every include-macro call site in the token
/// stream, in source order.
///
/// A site is the whole call shape `include[_str|_bytes]!(
/// <string-literal> )`; anything else (partial shapes, non-literal
/// arguments, non-file macros) produces no link. The walk is a sliding
/// window over the token stream, so a nested call
/// (`include!(include!("x"))`-style shapes the parser can recover from)
/// still finds the inner site.
#[must_use]
pub fn link_targets(source: &dyn LinkLexSource, doc_dir: &Path) -> Vec<IncludeLink> {
    let tokens = source.lexical_tokens();
    let mut links = Vec::new();
    let mut i = 0;
    // A complete call shape is five tokens: Ident Bang LParen StringLit RParen.
    while i + 5 <= tokens.len() {
        let shape = (
            &tokens[i],
            &tokens[i + 1],
            &tokens[i + 2],
            &tokens[i + 3],
            &tokens[i + 4],
        );
        let macro_kind = if shape.0.kind == SyntaxKind::Ident
            && shape.1.kind == SyntaxKind::Bang
            && shape.2.kind == SyntaxKind::LParen
            && shape.3.kind == SyntaxKind::StringLit
            && shape.4.kind == SyntaxKind::RParen
        {
            IncludeMacroKind::from_ident(&shape.0.text)
        } else {
            None
        };
        if let Some(macro_kind) = macro_kind {
            if let Some(target) = resolve_include_target(&shape.3.text, doc_dir) {
                links.push(IncludeLink {
                    macro_kind,
                    literal_lo: shape.3.lo,
                    literal_hi: shape.3.hi,
                    target,
                });
            }
            // The shape is consumed; the next candidate starts after the
            // closing paren.
            i += 5;
        } else {
            i += 1;
        }
    }
    links
}

/// `textDocument/documentLink` request entry point (LSP-DOC-LINK).
///
/// Resolution order mirrors `semantic_tokens_full`: URI → open-file
/// lookup → stored parse tree (no re-analysis) → include-call policy
/// over the document's directory → wire links. Returns `None` when the
/// document is not open, its parse tree is not stored, or it carries no
/// include-macro call; callers answer `null` for that.
#[must_use]
pub fn document_links(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentLinkParams,
) -> Option<Vec<DocumentLink>> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let green = {
        let syntaxes = db.syntaxes.read();
        syntaxes.get(&file_id)?.clone()
    };
    let doc_dir = path.parent()?;

    let links = link_targets(&ParseTreeLinkSource::new(&green), doc_dir);
    if links.is_empty() {
        return None;
    }

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let mut wire = Vec::with_capacity(links.len());
    for link in links {
        // The literal was lexed from this same document, so its extent
        // maps cleanly; the guard keeps a mid-request invalidation race
        // from aborting the whole response (one unmappable literal is
        // dropped, the good ones survive).
        let Some(((start_line, start_col), (end_line, end_col))) =
            sm.span_to_position(link.literal_lo, link.literal_hi)
        else {
            continue;
        };
        let Some(target_uri) = Url::from_file_path(&link.target).ok() else {
            continue;
        };
        let Ok(target) = lsp_types::Uri::from_str(target_uri.as_str()) else {
            continue;
        };
        wire.push(DocumentLink {
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
            target: Some(target),
            tooltip: Some(format!("Follow {}! target", link.macro_kind.as_str())),
            data: None,
        });
    }
    if wire.is_empty() {
        return None;
    }
    tracing::debug!(links = wire.len(), "document links resolved");
    Some(wire)
}
