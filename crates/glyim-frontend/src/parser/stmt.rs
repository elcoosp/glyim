use super::Parser;
use glyim_syntax::SyntaxKind;

impl<'a> Parser<'a> {
    pub(crate) fn parse_stmt(&mut self) {
        match self.current_kind() {
            SyntaxKind::KwLet => {
                self.start_node(SyntaxKind::LetStmt);
                self.bump(); // let
                self.parse_pat();
                if self.current_kind() == SyntaxKind::Colon {
                    self.bump();
                    self.parse_type();
                }
                if self.current_kind() == SyntaxKind::Eq {
                    self.bump();
                    self.parse_expr();
                }
                if self.current_kind() == SyntaxKind::Semicolon {
                    self.bump();
                } else {
                    self.error("expected ';' after let statement");
                }
                self.finish_node();
            }
            SyntaxKind::KwIf
            | SyntaxKind::KwWhile
            | SyntaxKind::KwFor
            | SyntaxKind::KwLoop
            | SyntaxKind::KwMatch
            | SyntaxKind::KwReturn
            | SyntaxKind::KwBreak
            | SyntaxKind::KwContinue
            | SyntaxKind::KwMove => {
                self.start_node(SyntaxKind::ExprStmt);
                self.parse_expr();
                if self.current_kind() == SyntaxKind::Semicolon {
                    self.bump();
                }
                self.finish_node();
            }
            SyntaxKind::KwFn
            | SyntaxKind::KwStruct
            | SyntaxKind::KwEnum
            | SyntaxKind::KwTrait
            | SyntaxKind::KwImpl
            | SyntaxKind::KwMod
            | SyntaxKind::KwConst
            | SyntaxKind::KwStatic
            | SyntaxKind::KwType
            | SyntaxKind::KwExtern
            | SyntaxKind::KwPub => self.parse_item(),
            SyntaxKind::KwUnsafe => {
                // Look ahead: if next is LBrace, it's an unsafe block expression
                if self.peek_kind() == Some(SyntaxKind::LBrace) {
                    self.start_node(SyntaxKind::ExprStmt);
                    self.parse_expr();
                    if self.current_kind() == SyntaxKind::Semicolon {
                        self.bump();
                    }
                    self.finish_node();
                } else {
                    self.parse_item();
                }
            }
            SyntaxKind::LBrace => {
                self.start_node(SyntaxKind::ExprStmt);
                self.parse_expr();
                if self.current_kind() == SyntaxKind::Semicolon {
                    self.bump();
                }
                self.finish_node();
            }
            _ => {
                if !matches!(
                    self.current_kind(),
                    SyntaxKind::Ident
                        | SyntaxKind::IntLit
                        | SyntaxKind::FloatLit
                        | SyntaxKind::StringLit
                        | SyntaxKind::CharLit
                        | SyntaxKind::ByteLit
                        | SyntaxKind::KwTrue
                        | SyntaxKind::KwFalse
                        | SyntaxKind::KwSelf
                        | SyntaxKind::KwSuper
                        | SyntaxKind::KwCrate
                        | SyntaxKind::LParen
                        | SyntaxKind::LBrace
                        | SyntaxKind::LBracket
                        | SyntaxKind::Or
                        | SyntaxKind::OrOr
                        | SyntaxKind::KwIf
                        | SyntaxKind::KwWhile
                        | SyntaxKind::KwFor
                        | SyntaxKind::KwLoop
                        | SyntaxKind::KwMatch
                        | SyntaxKind::KwReturn
                        | SyntaxKind::KwBreak
                        | SyntaxKind::KwContinue
                        | SyntaxKind::Bang
                        | SyntaxKind::Minus
                        | SyntaxKind::Star
                        | SyntaxKind::And
                        | SyntaxKind::KwUnsafe
                ) {
                    let found = self.current_kind();
                    self.error(format!("unexpected token in statement: {:?}", found));
                    if self.current().is_some() {
                        self.bump();
                    }
                    return;
                }
                self.start_node(SyntaxKind::ExprStmt);
                self.parse_expr();
                if self.current_kind() == SyntaxKind::Semicolon {
                    self.bump();
                } else if self.current_kind() == SyntaxKind::RBrace {
                    // tail expression
                } else {
                    self.error("expected ';' after expression");
                }
                self.finish_node();
            }
        }
    }
}
