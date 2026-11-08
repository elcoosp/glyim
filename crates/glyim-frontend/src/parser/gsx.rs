//! GSX view syntax parsing (docs/design/GSX-in-Glyim.md §3).
//!
//! GSX is first-class syntax with its own CST nodes (§3.4, finding F6): the
//! compiler must see the element structure, not a runtime function-call tree.
//! Prefix-position `<` + ident/`>` is unambiguously GSX today (finding F5:
//! `parse_primary_expr` had no `Lt` arm and qualified paths do not exist).
//!
//! Text is always a string literal (§3.2, finding F4): the lexer stays
//! context-free and eager — no lexer modes, no JSX whitespace rules. Spaces
//! between siblings are insignificant; write `" "` explicitly.

use glyim_syntax::SyntaxKind;
use rowan::Language;

use super::Parser;

/// §3.4: bound GSX element nesting independently of expression nesting so a
/// pathological `<a><a><a>…` source cannot overflow the stack.
const MAX_GSX_DEPTH: u32 = 128;

impl Parser<'_> {
    /// True when the next non-trivia token after the current one can start a
    /// GSX element/fragment name (`<div`, `<Counter`, `<web:canvas`, `<>`).
    pub(crate) fn at_gsx_start(&mut self) -> bool {
        if self.current_kind() != SyntaxKind::Lt {
            return false;
        }
        matches!(self.peek_kind(), Some(SyntaxKind::Ident | SyntaxKind::Gt))
    }

    /// Entry point from `parse_primary_expr`: `current_kind() == Lt`.
    pub(crate) fn parse_gsx_element(&mut self) {
        if self.gsx_depth > MAX_GSX_DEPTH {
            self.error("GSX element nested too deeply");
            // Consume one token so the enclosing children loop makes forward
            // progress instead of re-entering this arm forever.
            if self.current().is_some() {
                self.bump();
            }
            return;
        }
        self.gsx_depth += 1;
        self.parse_gsx_element_inner();
        self.gsx_depth -= 1;
    }

    fn parse_gsx_element_inner(&mut self) {
        let cp = self.checkpoint();
        self.bump(); // `<`
        if self.current_kind() == SyntaxKind::Gt {
            // Fragment: `<> … </>`
            self.start_node_at(cp, SyntaxKind::GsxFragment);
            self.bump(); // `>`
            self.parse_gsx_children();
            self.parse_gsx_close_tag(None);
            self.finish_node(); // GsxFragment
            return;
        }
        let name = self.parse_gsx_tag_name();
        let _ = &name;
        self.parse_gsx_attrs();
        match self.current_kind() {
            SyntaxKind::Slash => {
                // Self-closing: `<tag attrs />` — GsxSelfClosing wraps the
                // whole construct; GsxOpenTag wraps everything before the `/`.
                self.start_node_at(cp, SyntaxKind::GsxSelfClosing);
                self.start_node_at(cp, SyntaxKind::GsxOpenTag);
                self.finish_node(); // GsxOpenTag (ends before the `/`)
                self.bump(); // `/`
                self.expect_tag_gt();
                self.finish_node(); // GsxSelfClosing
            }
            SyntaxKind::Gt => {
                // Paired: `<tag attrs> children </tag>`
                self.bump(); // `>`
                self.start_node_at(cp, SyntaxKind::GsxElement);
                self.start_node_at(cp, SyntaxKind::GsxOpenTag);
                self.finish_node(); // GsxOpenTag (wraps `<tag attrs >`)
                self.parse_gsx_children();
                self.parse_gsx_close_tag(Some(&name));
                self.finish_node(); // GsxElement
            }
            found => {
                self.error(format!(
                    "expected `>` or `/>` to close the tag, found {found:?}"
                ));
                self.start_node_at(cp, SyntaxKind::GsxSelfClosing);
                self.finish_node();
            }
        }
    }

    /// Tag name: `Ident`, dialect namespace `Ident : Ident` (single colon,
    /// §3.2 `<web:canvas/>`), or a Rust path `Ident :: Ident …` (components,
    /// §3.2 `<ui::Button/>`).
    fn parse_gsx_tag_name(&mut self) -> String {
        let mut name = String::new();
        if self.current_kind() == SyntaxKind::Ident {
            name.push_str(self.current_text());
            self.bump();
        } else {
            self.error("expected a tag name");
            return name;
        }
        loop {
            match self.current_kind() {
                SyntaxKind::Colon if matches!(self.peek_kind(), Some(SyntaxKind::Ident)) => {
                    name.push(':');
                    self.bump(); // `:`
                    name.push_str(self.current_text());
                    self.bump(); // ident
                }
                SyntaxKind::ColonColon if matches!(self.peek_kind(), Some(SyntaxKind::Ident)) => {
                    name.push_str("::");
                    self.bump(); // `::`
                    name.push_str(self.current_text());
                    self.bump(); // ident
                }
                _ => break,
            }
        }
        name
    }

    /// Attribute list of an open tag. Ends at `>`, `/>`, or an unexpected token.
    fn parse_gsx_attrs(&mut self) {
        loop {
            match self.current_kind() {
                SyntaxKind::Ident | SyntaxKind::KwType | SyntaxKind::KwSelf => {
                    self.parse_gsx_attr();
                }
                SyntaxKind::LBrace => {
                    // `{..props}` spread or `{name}` shorthand.
                    if self.peek_non_ws_kind(0) == SyntaxKind::DotDot {
                        self.start_node(SyntaxKind::GsxSpread);
                        self.bump(); // `{`
                        self.bump(); // `..`
                        self.parse_expr();
                        self.expect(SyntaxKind::RBrace);
                        self.finish_node(); // GsxSpread
                    } else {
                        self.start_node(SyntaxKind::GsxAttr);
                        self.bump(); // `{`
                        self.parse_expr();
                        self.expect(SyntaxKind::RBrace);
                        self.finish_node(); // GsxAttr (shorthand)
                    }
                }
                _ => break,
            }
        }
    }

    /// One attribute. Shape decided by lookahead:
    /// `ns:name={…}` → [`SyntaxKind::GsxDirectiveAttr`],
    /// `name="lit" | name={expr} | bare` → [`SyntaxKind::GsxAttr`].
    fn parse_gsx_attr(&mut self) {
        let is_directive = self.peek_kind() == Some(SyntaxKind::Colon)
            && self.peek_non_ws_kind(1) == SyntaxKind::Ident;
        let kind = if is_directive {
            SyntaxKind::GsxDirectiveAttr
        } else {
            SyntaxKind::GsxAttr
        };
        self.start_node(kind);
        self.bump(); // name ident (keywords are accepted too via the caller gate)
        if is_directive {
            self.bump(); // `:`
            self.bump(); // directive ident
        }
        // Hyphenated names (`aria-label`, `data-foo`): `Ident Minus Ident`.
        while self.current_kind() == SyntaxKind::Minus
            && self.next_non_ws_kind() == SyntaxKind::Ident
        {
            self.bump(); // `-`
            self.bump(); // ident
        }
        if self.current_kind() == SyntaxKind::Eq {
            self.bump(); // `=`
            match self.current_kind() {
                SyntaxKind::StringLit => self.bump(),
                SyntaxKind::LBrace => {
                    self.bump(); // `{`
                    self.parse_expr();
                    self.expect(SyntaxKind::RBrace);
                }
                found => {
                    self.error(format!(
                        "expected string literal or `{{expr}}`, found {found:?}"
                    ));
                }
            }
        } else if is_directive && self.current_kind() == SyntaxKind::LBrace {
            // Directive shorthand (§3.2): `on:click={h}`, `bind:value={place}` —
            // the brace expression follows the colon directly, without `=`.
            self.bump(); // `{`
            self.parse_expr();
            self.expect(SyntaxKind::RBrace);
        }
        // Bare attribute: value defaults to `true` (§3.2).
        self.finish_node();
    }

    /// Children of a paired element or fragment. Trivia is insignificant
    /// (§3.2): text is only a string literal, so newlines between siblings
    /// produce nothing.
    fn parse_gsx_children(&mut self) {
        loop {
            match self.current_kind() {
                SyntaxKind::StringLit => {
                    self.start_node(SyntaxKind::GsxTextLit);
                    self.bump();
                    self.finish_node();
                }
                SyntaxKind::LBrace => {
                    self.start_node(SyntaxKind::GsxBlockChild);
                    self.bump(); // `{`
                    self.gsx_child_context = true;
                    self.parse_expr();
                    self.gsx_child_context = false;
                    self.expect(SyntaxKind::RBrace);
                    self.finish_node();
                }
                SyntaxKind::Lt if matches!(self.peek_kind(), Some(SyntaxKind::Gt)) => {
                    // Nested fragment `<>…</>`; hand off to the general entry.
                    self.parse_gsx_element();
                }
                SyntaxKind::Lt if matches!(self.peek_kind(), Some(SyntaxKind::Slash)) => {
                    // Close tag — belongs to this element.
                    break;
                }
                SyntaxKind::Lt if self.at_gsx_start() => {
                    self.parse_gsx_element();
                }
                SyntaxKind::RBrace | SyntaxKind::Semicolon => {
                    break;
                }
                tok => {
                    // Recovery: absorb one unexpected token per diagnostic.
                    self.error(format!("unexpected token in element children: {tok:?}"));
                    if self.current().is_some() {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// `</tag>` / `</>`. Optionally verifies the name matches the open tag.
    fn parse_gsx_close_tag(&mut self, open_name: Option<&str>) {
        if self.current_kind() != SyntaxKind::Lt || self.peek_kind() != Some(SyntaxKind::Slash) {
            self.error(format!(
                "expected closing tag `{}</{}>`",
                open_name.unwrap_or(""),
                open_name.unwrap_or("")
            ));
            return;
        }
        let cp = self.checkpoint();
        self.bump(); // `<`
        self.bump(); // `/`
        let mut name = String::new();
        if self.current_kind() == SyntaxKind::Gt {
            self.bump();
        } else {
            name = self.parse_gsx_tag_name();
            self.expect_tag_gt();
        }
        self.start_node_at(cp, SyntaxKind::GsxCloseTag);
        self.finish_node();
        if let (Some(open), false) = (open_name, name.is_empty()) {
            if open != name {
                self.error(format!(
                    "mismatched closing tag: expected `</{open}>`, found `</{name}>`"
                ));
            }
        }
    }

    /// `>` for a tag close, splitting a greedy `>>` ([`SyntaxKind::Shr`])
    /// the way the generics path does (§3.4).
    fn expect_tag_gt(&mut self) {
        match self.current_kind() {
            SyntaxKind::Gt => self.bump(),
            SyntaxKind::Shr => {
                // `>>` lexed greedily: emit one `>` and absorb the token.
                self.builder
                    .token(glyim_syntax::GlyimLang::kind_to_raw(SyntaxKind::Gt), ">");
                self.pos += 1;
            }
            found => {
                self.error(format!("expected `>`, found {found:?}"));
            }
        }
    }

    /// Text of the current token (for contextual keywords like `by`).
    pub(crate) fn current_text(&mut self) -> &str {
        self.flush_trivia();
        self.tokens.get(self.pos).map_or("", |t| t.text.as_str())
    }
}

// (Both element and fragment children stop at a leading `</`.)
