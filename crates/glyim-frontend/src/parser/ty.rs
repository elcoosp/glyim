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
                    // T019-PATCHED [FE-104]: run the same mut/lifetime
                    // loop the single-`&` arm uses, so `&&mut T` and
                    // `&&'a T` parse. The previous code called
                    // `parse_type` directly, which rejected `KwMut` and
                    // `Lifetime` and left the pointee unconsumed.
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
                // T077-PATCHED [FE-110]: distinguish `(T)` (parenthesized
                // type — Rust-equivalent to `T`) from `()` / `(T,)` /
                // `(T, U)` (tuple types). The previous code always emitted
                // a TupleType node, so `fn f(x: (i32))` silently had a
                // 1-tuple parameter — a layout/ABI/generics mismatch.
                //
                // T077-REV2: handle the empty tuple `()` explicitly — the
                // first version called `parse_type()` and then checked
                // for a `,`, but for `()` the current token after `(` is
                // `)`, and the comma that follows belongs to the *outer*
                // generic list. The check therefore misclassified `()`
                // as a tuple and swallowed the outer comma.
                let cp = self.checkpoint();
                self.bump(); // (
                if self.current_kind() == SyntaxKind::RParen {
                    // Unit tuple `()` — TupleType with no element children.
                    self.start_node_at(cp, SyntaxKind::TupleType);
                    self.expect(SyntaxKind::RParen);
                    self.finish_node();
                    return;
                }
                self.parse_type();
                if self.current_kind() == SyntaxKind::Comma {
                    // True tuple: consume comma-separated remaining types
                    // and the trailing comma if present.
                    self.start_node_at(cp, SyntaxKind::TupleType);
                    while self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                        if self.current_kind() == SyntaxKind::RParen {
                            break;
                        }
                        self.parse_type();
                    }
                    self.expect(SyntaxKind::RParen);
                    self.finish_node();
                } else {
                    // Parenthesized type: `(` and `)` stay as loose tokens
                    // inside the parent; the inner type node is the type.
                    self.expect(SyntaxKind::RParen);
                }
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
                let found = self.current_kind();
                self.error(format!("expected type, found {:?}", found));
                if self.current().is_some() {
                    self.bump();
                }
            }
        }
    }
}
