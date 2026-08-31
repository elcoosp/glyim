use super::Parser;
use glyim_syntax::{GlyimLang, SyntaxKind};
use rowan::Language;

impl<'a> Parser<'a> {
    pub(crate) fn parse_type(&mut self) {
        match self.current_kind() {
            SyntaxKind::AndAnd => {
                if let Some(_tok) = self.current() {
                    self.start_node(SyntaxKind::RefType);
                    self.builder
                        .token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
                    self.start_node(SyntaxKind::RefType);
                    self.builder
                        .token(GlyimLang::kind_to_raw(SyntaxKind::And), "&");
                    self.skip_token();
                    self.parse_type();
                    self.finish_node(); // inner
                    self.finish_node(); // outer
                }
            }
            SyntaxKind::And => {
                self.start_node(SyntaxKind::RefType);
                self.bump(); // &
                // Optional `mut` and/or lifetime, in either order:
                // `&mut`, `&'a`, `&'a mut`, `&mut 'a`.
                loop {
                    if self.current_kind() == SyntaxKind::KwMut {
                        self.bump();
                        continue;
                    }
                    if self.current_kind() == SyntaxKind::Lifetime {
                        self.bump();
                        continue;
                    }
                    break;
                }
                self.parse_type();
                self.finish_node();
            }
            SyntaxKind::Lifetime => {
                self.start_node(SyntaxKind::Lifetime);
                self.bump();
                self.finish_node();
            }
            SyntaxKind::Star => {
                self.start_node(SyntaxKind::RawPtrType);
                self.bump(); // *
                if self.current_kind() == SyntaxKind::KwConst
                    || self.current_kind() == SyntaxKind::KwMut
                {
                    self.bump();
                }
                self.parse_type();
                self.finish_node();
            }
            SyntaxKind::LBracket => {
                let cp = self.checkpoint();
                self.bump(); // [
                self.parse_type(); // inner type
                if self.current_kind() == SyntaxKind::Semicolon {
                    // Array type: [T; N]
                    self.start_node_at(cp, SyntaxKind::ArrayType);
                    self.bump(); // ;
                    self.parse_expr(); // length
                    self.expect(SyntaxKind::RBracket);
                    self.finish_node();
                } else {
                    // Slice type: [T]
                    self.start_node_at(cp, SyntaxKind::SliceType);
                    self.expect(SyntaxKind::RBracket);
                    self.finish_node();
                }
            }
            SyntaxKind::LParen => {
                self.start_node(SyntaxKind::TupleType);
                self.bump(); // (
                while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RParen);
                self.finish_node();
            }
            SyntaxKind::Bang => {
                self.start_node(SyntaxKind::NeverType);
                self.bump(); // !
                self.finish_node();
            }
            SyntaxKind::Underscore => {
                self.start_node(SyntaxKind::InferType);
                self.bump();
                self.finish_node();
            }
            SyntaxKind::KwDyn => {
                self.start_node(SyntaxKind::DynType);
                self.bump(); // dyn
                loop {
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Plus {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.finish_node();
            }
            SyntaxKind::KwImpl => {
                self.start_node(SyntaxKind::ImplTraitType);
                self.bump(); // impl
                loop {
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Plus {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.finish_node();
            }
            SyntaxKind::KwExtern => {
                // `extern "C" fn(...) -> Ret` function-pointer type (used in
                // FFI signatures, e.g. `f: extern "C" fn(*mut u8)`).
                self.start_node(SyntaxKind::FnType);
                self.bump(); // extern
                if self.current_kind() == SyntaxKind::StringLit {
                    self.bump(); // ABI string ("C")
                }
                self.expect(SyntaxKind::KwFn);
                self.expect(SyntaxKind::LParen);
                while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RParen);
                if self.current_kind() == SyntaxKind::Arrow {
                    self.bump();
                    self.parse_type();
                }
                self.finish_node();
            }
            SyntaxKind::KwFn => {
                self.start_node(SyntaxKind::FnType);
                self.bump(); // fn
                self.expect(SyntaxKind::LParen);
                while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RParen);
                if self.current_kind() == SyntaxKind::Arrow {
                    self.bump();
                    self.parse_type();
                }
                self.finish_node();
            }
            SyntaxKind::Ident | SyntaxKind::KwSelf | SyntaxKind::KwSuper | SyntaxKind::KwCrate => {
                self.start_node(SyntaxKind::PathType);
                self.parse_path();
                if self.current_kind() == SyntaxKind::Lt {
                    self.parse_type_arg_list();
                } else if self.current_kind() == SyntaxKind::LParen {
                    // Function-trait shorthand: `FnOnce(Args)` / `Fn(Args)`,
                    // optionally `-> Ret` (desugars to `Output = Ret`).
                    self.start_node(SyntaxKind::GenericArgList);
                    self.bump(); // (
                    while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                        self.parse_type();
                        if self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                        }
                    }
                    self.expect(SyntaxKind::RParen);
                    self.finish_node(); // GenericArgList
                }
                if self.current_kind() == SyntaxKind::Arrow {
                    self.bump();
                    self.parse_type(); // function-trait Output
                }
                self.finish_node();
            }
            _ => {
                self.error(format!("expected type, found {:?}", self.current_kind()));
                if self.current().is_some() {
                    self.bump();
                }
            }
        }
    }
}
