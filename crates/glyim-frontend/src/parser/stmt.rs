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
            // BLOCK-LIKE-STMT: `if`/`while`/`for`/`loop`/`match` (and a
            // labeled `'a: while`) are *block-like*: in statement position
            // they terminate the statement. Parse ONLY the construct via
            // `parse_primary_expr` — running the full `parse_expr` chain let
            // a trailing binary operator bind to the block, so
            // `while c { } & s[0..n]` parsed as `(while c {}) & (s[0..n])`
            // (bitwise-AND) and the tail's type came out `()`.
            SyntaxKind::KwIf
            | SyntaxKind::KwWhile
            | SyntaxKind::KwFor
            | SyntaxKind::KwLoop
            | SyntaxKind::KwMatch
            // FE-10: a labeled loop/block statement (`'a: while …`).
            | SyntaxKind::Lifetime => {
                self.start_node(SyntaxKind::ExprStmt);
                self.parse_primary_expr();
                if self.current_kind() == SyntaxKind::Semicolon {
                    self.bump();
                }
                self.finish_node();
            }
            // `return`/`break`/`continue`/`move` are NOT block-like — their
            // operand IS the expression, so the full chain applies.
            SyntaxKind::KwReturn
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
                // A bare block is block-like in statement position.
                self.start_node(SyntaxKind::ExprStmt);
                self.parse_primary_expr();
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
