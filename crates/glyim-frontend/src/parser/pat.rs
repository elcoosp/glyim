use super::Parser;
use glyim_syntax::SyntaxKind;

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
                self.start_node(SyntaxKind::PatRef);
                self.bump(); // &
                self.start_node(SyntaxKind::PatRef);
                self.bump(); // &
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
                    self.start_node(SyntaxKind::PatIdent);
                    self.bump();
                    self.finish_node();
                }
            }
            SyntaxKind::IntLit
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
                        SyntaxKind::IntLit
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
