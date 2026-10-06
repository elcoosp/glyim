use super::Parser;
use glyim_syntax::{GlyimLang, SyntaxKind};
use rowan::Language;

impl<'a> Parser<'a> {
    pub(crate) fn parse_pat(&mut self) {
        let cp = self.checkpoint();
        self.parse_pat_single();
        if self.current_kind() == SyntaxKind::Or {
            self.start_node_at(cp, SyntaxKind::PatOr);
            while self.current_kind() == SyntaxKind::Or {
                self.bump(); // |
                self.parse_pat_single();
            }
            self.finish_node(); // PatOr
        }
    }

    pub(crate) fn parse_pat_single(&mut self) {
        match self.current_kind() {
            SyntaxKind::KwRef => {
                self.bump(); // ref
                if self.current_kind() == SyntaxKind::KwMut {
                    self.bump(); // mut
                }
                self.parse_pat_inner();
            }
            SyntaxKind::KwMut => {
                self.bump(); // mut
                self.parse_pat_inner();
            }
            SyntaxKind::AndAnd => {
                // T017-PATCHED [FE-101]: the lexer fuses `&&` into a single
                // `AndAnd` token, so the previous double `bump()` consumed
                // the fused token *and* the first token of the inner
                // pattern (e.g. `x` in `&&x`) and cascaded into an
                // "expected pattern, found ..." error. Emit two synthetic
                // `&` tokens (one per nested `PatRef`) and consume the
                // fused token once via `skip_token`.
                self.start_node(SyntaxKind::PatRef);
                self.builder
                    .token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
                self.start_node(SyntaxKind::PatRef);
                self.builder
                    .token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
                self.skip_token(); // consume the fused `&&`
                self.parse_pat_inner();
                self.finish_node();
                self.finish_node();
            }
            SyntaxKind::And => {
                self.bump(); // &
                if self.current_kind() == SyntaxKind::KwMut {
                    self.bump(); // mut
                }
                self.parse_pat_inner();
            }
            SyntaxKind::LParen => {
                self.start_node(SyntaxKind::PatTuple);
                self.bump(); // (
                while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                    // T076-PATCHED [FE-108]: `..` inside a tuple pattern
                    // ignores the remaining elements — a common idiom
                    // (`let (a, ..) = t;`). `parse_pat` has no DotDot
                    // arm, so previously this cascaded into "expected
                    // pattern, found DotDot". The slice-pattern code
                    // already treats it specially.
                    if self.current_kind() == SyntaxKind::DotDot {
                        self.bump(); // ..
                        if self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                        }
                        continue;
                    }
                    self.parse_pat();
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RParen);
                self.finish_node();
            }
            _ => {
                self.parse_pat_inner();
            }
        }
    }

    pub(crate) fn parse_pat_inner(&mut self) {
        // FE-6: nested patterns are a stack-overflow vector; bound them.
        if self.recursion_depth > super::MAX_EXPR_DEPTH {
            self.error("pattern nested too deeply");
            if self.current().is_some() {
                self.bump();
            }
            return;
        }
        self.recursion_depth += 1;
        self.parse_pat_inner_impl();
        self.recursion_depth -= 1;
    }

    fn parse_pat_inner_impl(&mut self) {
        match self.current_kind() {
            SyntaxKind::LBracket => {
                self.start_node(SyntaxKind::PatSlice);
                self.bump(); // [
                while self.current_kind() != SyntaxKind::RBracket && self.current().is_some() {
                    if self.current_kind() == SyntaxKind::DotDot {
                        self.bump(); // ..
                        if self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                        }
                    } else {
                        self.parse_pat();
                        if self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                        }
                    }
                }
                self.expect(SyntaxKind::RBracket);
                self.finish_node();
            }
            SyntaxKind::Bang => {
                self.start_node(SyntaxKind::NeverType);
                self.bump(); // !
                self.finish_node();
            }
            SyntaxKind::Underscore => {
                self.start_node(SyntaxKind::PatWild);
                self.bump();
                self.finish_node();
            }
            SyntaxKind::Ident | SyntaxKind::KwSelf | SyntaxKind::KwSuper | SyntaxKind::KwCrate => {
                let next = self.peek_kind().unwrap_or(SyntaxKind::Error);
                if next == SyntaxKind::ColonColon
                    || next == SyntaxKind::LParen
                    || next == SyntaxKind::LBrace
                {
                    let outer_cp = self.checkpoint();
                    self.start_node(SyntaxKind::UsePath);
                    self.parse_path_inner();
                    self.finish_node();

                    if self.current_kind() == SyntaxKind::LParen {
                        // Wrap UsePath + PatTuple in a single PatStruct node
                        self.start_node_at(outer_cp, SyntaxKind::PatStruct);
                        self.start_node(SyntaxKind::PatTuple);
                        self.bump(); // (
                        while self.current_kind() != SyntaxKind::RParen && self.current().is_some()
                        {
                            self.parse_pat();
                            if self.current_kind() == SyntaxKind::Comma {
                                self.bump();
                            }
                        }
                        self.expect(SyntaxKind::RParen);
                        self.finish_node(); // PatTuple
                        self.finish_node(); // PatStruct
                    } else if self.current_kind() == SyntaxKind::LBrace {
                        // Wrap UsePath + fields in a single PatStruct node
                        self.start_node_at(outer_cp, SyntaxKind::PatStruct);
                        self.bump(); // {
                        while self.current_kind() != SyntaxKind::RBrace && self.current().is_some()
                        {
                            if self.current_kind() == SyntaxKind::DotDot {
                                self.bump(); // ..
                                if self.current_kind() == SyntaxKind::Comma {
                                    self.bump();
                                }
                            } else if self.current_kind() == SyntaxKind::Ident {
                                let cp = self.checkpoint();
                                self.bump(); // field name
                                if self.current_kind() == SyntaxKind::Colon {
                                    self.start_node_at(cp, SyntaxKind::PatIdent);
                                    self.finish_node();
                                    self.bump(); // :
                                    self.parse_pat();
                                } else {
                                    self.start_node_at(cp, SyntaxKind::PatIdent);
                                    self.finish_node();
                                }
                            } else {
                                self.error("expected field pattern");
                                if self.current().is_some() {
                                    self.bump();
                                }
                            }
                            if self.current_kind() == SyntaxKind::Comma {
                                self.bump();
                            }
                        }
                        self.expect(SyntaxKind::RBrace);
                        self.finish_node(); // PatStruct
                    }
                } else {
                    // T002-PATCHED [FE-102]: `current_kind()` flushes pending
                    // trivia into the currently open node. Probing for `@`
                    // *inside* the PatIdent therefore absorbed the trailing
                    // whitespace after `x` into the node (FE-9 regression,
                    // CST contract broken for six snapshot tests). Peek with
                    // a manual scan over `self.tokens` (no flushing) and
                    // only open the node when we know the shape.
                    self.start_node(SyntaxKind::PatIdent);
                    self.bump();
                    // FE-9: `x @ subpat` — binding with a sub-pattern.
                    let at_follows = {
                        let mut p = self.pos;
                        while let Some(t) = self.tokens.get(p) {
                            if !t.kind.is_trivia() {
                                break;
                            }
                            p += 1;
                        }
                        self.tokens.get(p).map(|t| t.kind) == Some(SyntaxKind::At)
                    };
                    if at_follows {
                        self.flush_trivia(); // whitespace legitimately precedes `@`
                        self.bump(); // @
                        // T164-PATCHED [FE-109]: `x @ (a, b)` needs the
                        // tuple/struct/tuple-struct arm that lives in
                        // `parse_pat_single`, not the narrower
                        // `parse_pat_inner` (which handles `&`, `ref`,
                        // `mut`, and the primitives but not `(`).
                        // `x @ Some(y)` worked only because the Ident
                        // arm already chains an optional `(`; the parenthesized
                        // subpattern was unparseable.
                        self.parse_pat_single();
                    }
                    self.finish_node();
                }
            }
            // T022-PATCHED [FE-107]: leading `-` for negative literal
            // patterns. `match x { -1 => ... }` previously fell through to
            // the "expected pattern" arm and cascaded.
            SyntaxKind::Minus => {
                self.start_node(SyntaxKind::PatLit);
                self.bump(); // -
                if matches!(
                    self.current_kind(),
                    SyntaxKind::IntLit | SyntaxKind::FloatLit
                ) {
                    self.bump(); // the literal
                } else {
                    self.error("expected numeric literal after `-` in pattern");
                }
                self.finish_node();
            }
            SyntaxKind::ByteLit
            | SyntaxKind::IntLit
            | SyntaxKind::FloatLit
            | SyntaxKind::StringLit
            | SyntaxKind::CharLit
            | SyntaxKind::KwTrue
            | SyntaxKind::KwFalse => {
                let start_cp = self.checkpoint();
                self.bump(); // consume start literal
                if matches!(
                    self.current_kind(),
                    SyntaxKind::DotDot | SyntaxKind::DotDotEq
                ) {
                    // Range pattern: use PatRange as the outer node
                    self.start_node_at(start_cp, SyntaxKind::PatRange);
                    let _range_op = self.current_kind();
                    self.bump(); // .. or ..=
                    // Validate that the endpoint is a literal, not a pattern.
                    // We'll check if the current token is a literal; if not, emit error.
                    let is_literal = matches!(
                        self.current_kind(),
                        SyntaxKind::ByteLit
                            | SyntaxKind::IntLit
                            | SyntaxKind::FloatLit
                            | SyntaxKind::StringLit
                            | SyntaxKind::CharLit
                            | SyntaxKind::KwTrue
                            | SyntaxKind::KwFalse
                    );
                    if !matches!(
                        self.current_kind(),
                        SyntaxKind::FatArrow
                            | SyntaxKind::Comma
                            | SyntaxKind::RBrace
                            | SyntaxKind::RParen
                            | SyntaxKind::RBracket
                    ) {
                        if !is_literal {
                            self.error("range endpoint must be a literal");
                        }
                        self.parse_pat(); // Parses the end literal into a nested PatLit
                    }
                    self.finish_node(); // PatRange
                } else {
                    // Simple literal — wrap in PatLit
                    self.start_node_at(start_cp, SyntaxKind::PatLit);
                    self.finish_node();
                }
            }
            _ => {
                let found = self.current_kind();
                self.error(format!("expected pattern, found {:?}", found));
                if self.current().is_some() {
                    self.bump();
                }
            }
        }
    }
}
