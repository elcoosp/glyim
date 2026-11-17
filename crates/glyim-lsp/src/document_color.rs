//! Document-color request support (feature tag LSP-COLOR).
//!
//! The editor-facing color story is the one capability class the server
//! never answered: `textDocument/documentColor` (which ranges of a file
//! spell a renderable color, so an editor can draw the swatch inline) and
//! `textDocument/colorPresentation` (what text to splice in when the user
//! picks a color for one of those ranges). Glyim sources carry colors as
//! *whole-string hex literals* — a GSX styling attribute value, an
//! include-consumed theme string — spelled `#RGB`, `#RGBA`, `#RRGGBB`, or
//! `#RRGGBBAA` inside a double-quoted string literal. Every fact the
//! requests need was already stored: the driver keeps each file's parse
//! tree (the same immutable `GreenNode` store selection-range,
//! semantic-tokens, and document-link read), and the string-literal tokens
//! carry their verbatim text. This module closes that capability gap with
//! no re-analysis on the request path — and it works on broken code, the
//! same property LSP-SELECTION and LSP-DOC-LINK get from walking the tree
//! that predates lowering.
//!
//! The seam: *which tokens are string literals* is split from *which of
//! them spell colors* by [`ColorLexSource`], mirroring LSP-DOC-LINK's
//! `LinkLexSource`. The policy ([`compute_document_colors`]) reads through
//! the trait and stays testable without a rowan tree or locks
//! (`tests/document_color.rs`); the production [`ParseTreeColorSource`]
//! walks the driver's stored parse tree; a future source over macro-
//! expanded tokens would slot in behind the same seam without touching the
//! policy.
//!
//! What counts as a color site, over the recorded literals: the literal's
//! decoded body must *be* a hex payload — `#` followed by exactly 3, 4, 6,
//! or 8 hex digits and nothing else — the four widths CSS consumers spell.
//! A color embedded in prose (`"bg: #ff00aa"`) answers nothing: the server
//! would have to guess which slice of the string the parser owns, and a
//! guessed site is a picker edit that rewrites arbitrary user text. The
//! decode honesty mirrors document-link's: an escaped body can never
//! match, because the minimal escape set decodes to backslashes and
//! quotes, and neither survives the hex charset — so a matching payload is
//! escape-free by construction and its extent is exactly the literal
//! without its quotes.
//!
//! Wire mapping (LSP spec): each [`RecordedColor`] becomes one
//! `ColorInformation` whose range is the payload extent (quotes excluded —
//! the swatch sits on the color characters, the LSP-EXACT-EXTENTS
//! convention document-link pins for the clickable region) and whose
//! components are the parsed channels as the spec's `f32` fractions in
//! `[0-1]` (3/4-digit literals expand each nibble over 15, 6/8-digit
//! literals divide the byte over 255; a 4th digit supplies alpha, an
//! absent one is `1.0`). Colors answer in source order and are *not*
//! deduplicated — two identical literals are two distinct sites with two
//! swatches, unlike the inline-value engine's repeated-fact collapse. The
//! response is `null` when the document is not open, its tree is not
//! stored, or it carries no color site, per the other engines' convention.
//!
//! Presentations ([`compute_color_presentations`], served by
//! [`color_presentations`]): the picker hands back a color *value* for a
//! reported range, and the server answers the text to splice. Glyim
//! carries exactly one color notation — the hex string literal — so the
//! answer is exactly one presentation: the canonical lowercase `#rrggbb`
//! spelling when the requested alpha is `1.0`, the 8-digit `#rrggbbaa`
//! form otherwise. An `rgb()`-functional or named-color presentation would
//! be invented syntax the parser cannot read back, so none is offered;
//! components are normalized into `[0-1]` and quantized to 8-bit channels
//! (the honest precision the hex notation carries). The entry point only
//! answers when the requested range maps onto a recorded payload extent —
//! a presentation for a range no color site occupies would generate a
//! prose rewrite, so it answers `None` internally. The requested color may
//! differ from the recorded one: recoloring is the API's purpose.
//!
//! The `None` convention vs the wire: lsp-types 0.97 pins the
//! `documentColor`/`colorPresentation` results to `Vec` (no null face, an
//! empty listing is the zero answer), so the router flattens the engines'
//! `None` — the engines' own "no answer" convention — into an empty list
//! on the wire; the tests pin both halves.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use lsp_types::{
    Color, ColorInformation, ColorPresentation, ColorPresentationParams, DocumentColorParams,
    TextEdit,
};
use url::Url;

/// One string-literal token, wire-type-free: byte extent and verbatim
/// text (quotes included, the lexer's spelling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorLit {
    /// Byte offset of the token's first byte.
    pub lo: usize,
    /// Byte offset one past the token's last byte.
    pub hi: usize,
    /// Verbatim token text, quotes included.
    pub text: String,
}

/// The string-literal seam for color requests (LSP-COLOR).
///
/// The color policy in [`compute_document_colors`] must stay testable
/// without a live `AnalysisDatabase` (parse trees, locks, source maps), so
/// it reads recorded literals through this trait. The production
/// [`ParseTreeColorSource`] walks the driver's stored parse tree; a future
/// macro-expanded token source would slot in behind the same seam without
/// touching the policy.
pub trait ColorLexSource {
    /// Every string-literal token of one file in source order, verbatim.
    fn string_literals(&self) -> Vec<ColorLit>;
}

/// Production [`ColorLexSource`]: borrows the driver's stored parse tree
/// for one file.
///
/// The store keeps the immutable [`GreenNode`] (rowan cursor nodes carry
/// raw parent pointers and are not `Send`, so they cannot live in the
/// shared database); each request materializes its own cursor root with
/// one `Arc` clone — the same trick LSP-SELECTION, LSP-SEMANTIC-TOKENS and
/// LSP-DOC-LINK use — instead of re-parsing the file.
pub struct ParseTreeColorSource<'a> {
    green: &'a GreenNode,
}

impl<'a> ParseTreeColorSource<'a> {
    /// Bind the source to one file's stored parse tree.
    #[must_use]
    pub fn new(green: &'a GreenNode) -> Self {
        Self { green }
    }
}

impl ColorLexSource for ParseTreeColorSource<'_> {
    fn string_literals(&self) -> Vec<ColorLit> {
        let root = SyntaxNode::new_root(self.green.clone());
        root.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::StringLit)
            .map(|token| {
                let range = token.text_range();
                ColorLit {
                    lo: u32::from(range.start()) as usize,
                    hi: u32::from(range.end()) as usize,
                    text: token.text().to_string(),
                }
            })
            .collect()
    }
}

/// One color site the request can answer with, wire-type-free: the hex
/// payload's byte-exact extent inside the literal (quotes excluded) and
/// the parsed channels as fractions in `[0-1]`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedColor {
    /// Byte offset of the `#` — the payload's first byte.
    pub payload_lo: usize,
    /// Byte offset one past the payload's last hex digit.
    pub payload_hi: usize,
    /// Red channel in `[0-1]`.
    pub red: f32,
    /// Green channel in `[0-1]`.
    pub green: f32,
    /// Blue channel in `[0-1]`.
    pub blue: f32,
    /// Alpha channel in `[0-1]`; `1.0` when the literal spells no digit.
    pub alpha: f32,
}

/// Parse a string-literal body into its color channels, wire-free.
///
/// The whole body must *be* the payload: `#` then exactly 3, 4, 6, or 8
/// ASCII hex digits. `#RGB`/`#RGBA` expand each nibble over 15;
/// `#RRGGBB`/`#RRGGBBAA` divide each byte over 255; a missing digit
/// spells alpha `1.0`. Anything else (prose around a color, a wrong
/// digit count, a non-hex character) answers `None` — see the module
/// docs for why a guessed slice of a string would be dishonest.
fn hex_payload_channels(body: &str) -> Option<(f32, f32, f32, f32)> {
    let digits = body.strip_prefix('#')?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |index: usize| -> f32 {
        char::from(digits.as_bytes()[index])
            .to_digit(16)
            .unwrap_or(0) as f32
    };
    match digits.len() {
        3 | 4 => {
            // Nibble shorthand: one digit per channel, the channel
            // fraction the digit names (v*17/255 == v/15).
            let expand = |index: usize| digit(index) / 15.0;
            let alpha = if digits.len() == 4 { expand(3) } else { 1.0 };
            Some((expand(0), expand(1), expand(2), alpha))
        }
        6 | 8 => {
            // Byte-per-channel: two digits spell one channel, the byte
            // value (d_hi*16 + d_lo) divided over 255.
            let byte = |hi_index: usize, lo_index: usize| -> f32 {
                (digit(hi_index) * 16.0 + digit(lo_index)) / 255.0
            };
            let alpha = if digits.len() == 8 { byte(6, 7) } else { 1.0 };
            Some((byte(0, 1), byte(2, 3), byte(4, 5), alpha))
        }
        _ => None,
    }
}

/// One presentation fact the picker answers with, wire-type-free: the
/// label (and splice text) for the requested color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorPresentationFact {
    /// The canonical hex payload spelling, e.g. `#ff00aa`.
    pub label: String,
}

/// Spell the canonical hex payload for the requested channels: lowercase,
/// `#rrggbb` at full opacity, `#rrggbbaa` otherwise.
///
/// Components are clamped into `[0-1]` first (a picker may hand back an
/// out-of-range float; the hex notation has no spelling for it) and each
/// channel is quantized to the nearest 8-bit value — the honest precision
/// the notation carries.
#[must_use]
pub fn spell_hex_payload(red: f32, green: f32, blue: f32, alpha: f32) -> String {
    let channel = |v: f32| -> u8 { (v.clamp(0.0, 1.0) * 255.0).round() as u8 };
    let (r, g, b, a) = (channel(red), channel(green), channel(blue), channel(alpha));
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// The color-presentation policy: the single presentation the requested
/// channels answer with, spelled by [`spell_hex_payload`].
///
/// Glyim carries exactly one color notation, so the picker answers one
/// presentation — see the module docs for why no `rgb()`-functional form
/// is invented.
#[must_use]
pub fn compute_color_presentations(
    red: f32,
    green: f32,
    blue: f32,
    alpha: f32,
) -> Vec<ColorPresentationFact> {
    vec![ColorPresentationFact {
        label: spell_hex_payload(red, green, blue, alpha),
    }]
}

/// The document-color policy: every color site in the recorded literals,
/// in source order.
///
/// A site is a literal whose decoded body *is* a hex payload (see
/// [`hex_payload_channels`]); the payload extent is the literal without
/// its quotes — sound because a matching body is escape-free by
/// construction. Sites are not deduplicated: two identical literals are
/// two distinct swatches.
#[must_use]
pub fn compute_document_colors(source: &dyn ColorLexSource) -> Vec<RecordedColor> {
    let mut sites = Vec::new();
    for lit in source.string_literals() {
        let Some(body) = lit.text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
            continue;
        };
        if let Some((red, green, blue, alpha)) = hex_payload_channels(body) {
            sites.push(RecordedColor {
                payload_lo: lit.lo + 1,
                payload_hi: lit.hi - 1,
                red,
                green,
                blue,
                alpha,
            });
        }
    }
    sites
}

/// Map one fact to the wire color information, with the span→position
/// mapping injected (`None` when the site's file is gone from the session
/// — one unmappable site must not erase the good ones, and the caller
/// filters `None`s).
#[must_use]
pub fn wire_color_information(
    fact: &RecordedColor,
    map: &dyn Fn(usize, usize) -> Option<((usize, usize), (usize, usize))>,
) -> Option<ColorInformation> {
    let ((start_line, start_col), (end_line, end_col)) = map(fact.payload_lo, fact.payload_hi)?;
    Some(ColorInformation {
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
        color: Color {
            red: fact.red,
            green: fact.green,
            blue: fact.blue,
            alpha: fact.alpha,
        },
    })
}

/// `textDocument/documentColor` request entry point (LSP-COLOR).
///
/// Resolution order mirrors `document_links`: URI → open-file lookup →
/// stored parse tree (no re-analysis) → color-site policy → wire colors.
/// Returns `None` when the document is not open, its parse tree is not
/// stored, or it carries no color site — the router flattens that to the
/// empty listing the lsp-types `Vec` result pins (see the module docs).
#[must_use]
pub fn document_colors(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentColorParams,
) -> Option<Vec<ColorInformation>> {
    let path = Url::parse(params.text_document.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let green = {
        let syntaxes = db.syntaxes.read();
        syntaxes.get(&file_id)?.clone()
    };
    let sites = compute_document_colors(&ParseTreeColorSource::new(&green));
    if sites.is_empty() {
        return None;
    }

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let items: Vec<ColorInformation> = sites
        .iter()
        .filter_map(|site| wire_color_information(site, &|lo, hi| sm.span_to_position(lo, hi)))
        .collect();
    drop(source_maps);
    if items.is_empty() {
        return None;
    }
    tracing::debug!(file = %file_id.to_raw(), colors = items.len(), "document colors resolved");
    Some(items)
}

/// `textDocument/colorPresentation` request entry point (LSP-COLOR).
///
/// The picker hands back a color *value* for a range documentColor
/// reported; the entry point re-derives the same recorded sites and only
/// answers when the requested range maps byte-exactly onto one of them —
/// a presentation for a range no site occupies would splice a hex payload
/// over arbitrary user text, so it answers `None` internally (the router
/// flattens that to the empty result the lsp-types `Vec` pins). The
/// requested color may differ from the recorded one (recoloring is the
/// API's purpose); the answered edit replaces the payload extent with the
/// canonical spelling.
#[must_use]
pub fn color_presentations(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &ColorPresentationParams,
) -> Option<Vec<ColorPresentation>> {
    let path = Url::parse(params.text_document.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let green = {
        let syntaxes = db.syntaxes.read();
        syntaxes.get(&file_id)?.clone()
    };
    let sites = compute_document_colors(&ParseTreeColorSource::new(&green));
    if sites.is_empty() {
        return None;
    }

    // The requested range back to byte offsets (INF-11: the same UTF-16
    // mapping the reported ranges went out through), then the site whose
    // payload extent is exactly that range.
    let lo_hi = {
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
        drop(source_maps);
        (lo, hi)
    };
    let color = params.color;
    let site = sites
        .iter()
        .find(|site| (site.payload_lo, site.payload_hi) == lo_hi)?;

    let facts = compute_color_presentations(color.red, color.green, color.blue, color.alpha);
    let wire: Vec<ColorPresentation> = facts
        .iter()
        .map(|fact| ColorPresentation {
            label: fact.label.clone(),
            text_edit: Some(TextEdit {
                range: params.range,
                new_text: fact.label.clone(),
            }),
            additional_text_edits: None,
        })
        .collect();
    tracing::debug!(file = %file_id.to_raw(), site = site.payload_lo, "color presentations resolved");
    Some(wire)
}
