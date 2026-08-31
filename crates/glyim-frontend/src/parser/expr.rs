use super::Parser;
use glyim_syntax::{GlyimLang, SyntaxKind};
use rowan::Language;

impl<'a> Parser<'a> {
    pub(crate) fn parse_block(&mut self) {
        self.start_node(SyntaxKind::Block);
        self.expect(SyntaxKind::LBrace);
        self.parse_block_inner();
        self.expect(SyntaxKind::RBrace);
        self.finish_node();
    }

    pub(crate) fn parse_block_inner(&mut self) {
        while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
            self.parse_stmt();
        }
    }

    pub(crate) fn parse_expr(&mut self) {
        self.parse_assignment_expr();
    }

    pub(crate) fn parse_assignment_expr(&mut self) {
        let cp = self.checkpoint();
        self.parse_range_expr();
        if matches!(
            self.current_kind(),
            SyntaxKind::Eq
                | SyntaxKind::PlusEq
                | SyntaxKind::MinusEq
                | SyntaxKind::StarEq
                | SyntaxKind::SlashEq
                | SyntaxKind::OrEq
                | SyntaxKind::AndEq
                | SyntaxKind::CaretEq
                | SyntaxKind::ShlEq
                | SyntaxKind::ShrEq
        ) {
            self.start_node_at(cp, SyntaxKind::AssignExpr);
            self.bump();
            self.parse_assignment_expr();
            self.finish_node();
        }
    }

    pub(crate) fn parse_range_expr(&mut self) {
        let cp = self.checkpoint();
        // Range-from / inclusive-range-from: `..end` or `..=end` (no start).
        // Must be checked before parse_or_expr, otherwise the leading `..`
        // is seen as an unexpected primary.
        if matches!(
            self.current_kind(),
            SyntaxKind::DotDot | SyntaxKind::DotDotEq
        ) {
            self.start_node_at(cp, SyntaxKind::RangeExpr);
            self.bump();
            if !matches!(
                self.current_kind(),
                SyntaxKind::Eq
                    | SyntaxKind::Semicolon
                    | SyntaxKind::Comma
                    | SyntaxKind::RParen
                    | SyntaxKind::RBrace
                    | SyntaxKind::RBracket
            ) {
                self.parse_or_expr();
            }
            self.finish_node();
            return;
        }
        self.parse_or_expr();
        if matches!(
            self.current_kind(),
            SyntaxKind::DotDot | SyntaxKind::DotDotEq
        ) {
            self.start_node_at(cp, SyntaxKind::RangeExpr);
            self.bump();
            if !matches!(
                self.current_kind(),
                SyntaxKind::Eq
                    | SyntaxKind::Semicolon
                    | SyntaxKind::Comma
                    | SyntaxKind::RParen
                    | SyntaxKind::RBrace
                    | SyntaxKind::RBracket
            ) {
                self.parse_or_expr();
            }
            self.finish_node();
        }
    }

    pub(crate) fn parse_or_expr(&mut self) {
        self.parse_and_expr();
        while self.current_kind() == SyntaxKind::OrOr {
            self.start_node(SyntaxKind::BinaryExpr);
            self.bump();
            self.parse_and_expr();
            self.finish_node();
        }
    }

    pub(crate) fn parse_and_expr(&mut self) {
        self.parse_comparison_expr();
        while self.current_kind() == SyntaxKind::AndAnd {
            self.start_node(SyntaxKind::BinaryExpr);
            self.bump();
            self.parse_comparison_expr();
            self.finish_node();
        }
    }

    pub(crate) fn parse_comparison_expr(&mut self) {
        let cp = self.checkpoint();
        self.parse_bitwise_expr();
        if matches!(
            self.current_kind(),
            SyntaxKind::EqEq
                | SyntaxKind::BangEq
                | SyntaxKind::Lt
                | SyntaxKind::Gt
                | SyntaxKind::LtEq
                | SyntaxKind::GtEq
        ) {
            self.start_node_at(cp, SyntaxKind::BinaryExpr);
            self.bump();
            self.parse_bitwise_expr();
            self.finish_node();
        }
    }

    pub(crate) fn parse_bitwise_expr(&mut self) {
        let mut cp = self.checkpoint();
        self.parse_additive_expr();
        while matches!(
            self.current_kind(),
            SyntaxKind::And
                | SyntaxKind::Or
                | SyntaxKind::Caret
                | SyntaxKind::Shl
                | SyntaxKind::Shr
        ) {
            self.start_node_at(cp, SyntaxKind::BinaryExpr);
            self.bump();
            self.parse_additive_expr();
            self.finish_node();
            cp = self.checkpoint();
        }
    }

    pub(crate) fn parse_additive_expr(&mut self) {
        let mut cp = self.checkpoint();
        self.parse_multiplicative_expr();
        while matches!(self.current_kind(), SyntaxKind::Plus | SyntaxKind::Minus) {
            self.start_node_at(cp, SyntaxKind::BinaryExpr);
            self.bump();
            self.parse_multiplicative_expr();
            self.finish_node();
            cp = self.checkpoint();
        }
    }

    pub(crate) fn parse_multiplicative_expr(&mut self) {
        let mut cp = self.checkpoint();
        self.parse_unary_expr();
        while matches!(
            self.current_kind(),
            SyntaxKind::Star | SyntaxKind::Slash | SyntaxKind::Percent
        ) {
            self.start_node_at(cp, SyntaxKind::BinaryExpr);
            self.bump();
            self.parse_unary_expr();
            self.finish_node();
            cp = self.checkpoint();
        }
    }

    pub(crate) fn parse_unary_expr(&mut self) {
        if matches!(
            self.current_kind(),
            SyntaxKind::Bang | SyntaxKind::Minus | SyntaxKind::Star | SyntaxKind::And
        ) {
            self.start_node(SyntaxKind::UnaryExpr);
            self.bump();
            // For mutable borrows (`&mut x`), consume the `mut` keyword here so
            // it becomes a direct child token of the `UnaryExpr` node. Without
            // this, `mut` falls through to `parse_postfix_expr`, is wrapped in
            // an error node, and HIR lowering cannot distinguish `&mut` from
            // `&` (it always lowers to an immutable `Ref`).
            if self.current_kind() == SyntaxKind::KwMut {
                self.bump();
            }
            self.parse_unary_expr();
            self.finish_node();
        } else {
            self.parse_postfix_expr();
        }
    }

    pub(crate) fn parse_postfix_expr(&mut self) {
        let cp = self.checkpoint();
        self.parse_primary_expr();
        // Macro invocation: path followed by '!'
        if self.current_kind() == SyntaxKind::Bang && self.last_was_path {
            self.start_node_at(cp, SyntaxKind::MacroCall);
            self.bump(); // !
            // Parse token tree as arguments
            self.parse_token_tree();
            self.finish_node();
            return;
        }
        loop {
            match self.current_kind() {
                SyntaxKind::Dot => {
                    self.bump();
                    if self.current_kind() == SyntaxKind::KwAwait {
                        // `.await` postfix: wrap the already-parsed operand
                        // (everything before the `.`) into an `AwaitExpr`.
                        self.bump(); // await
                        self.start_node_at(cp, SyntaxKind::AwaitExpr);
                        self.finish_node();
                        continue;
                    }
                    if matches!(
                        self.current_kind(),
                        SyntaxKind::Ident | SyntaxKind::IntLit | SyntaxKind::FloatLit
                    ) {
                        self.bump();
                    } else {
                        self.error("expected field name or index after '.'");
                    }
                    if self.current_kind() == SyntaxKind::ColonColon {
                        self.bump();
                        if self.current_kind() == SyntaxKind::Lt {
                            self.parse_type_arg_list();
                        }
                    }
                    if self.current_kind() == SyntaxKind::LParen {
                        self.start_node_at(cp, SyntaxKind::MethodCallExpr);
                        self.bump();
                        if self.current_kind() != SyntaxKind::RParen {
                            self.parse_expr();
                            while self.current_kind() == SyntaxKind::Comma {
                                self.bump();
                                if self.current_kind() == SyntaxKind::RParen {
                                    break;
                                }
                                self.parse_expr();
                            }
                        }
                        self.expect(SyntaxKind::RParen);
                        self.finish_node();
                    } else {
                        self.start_node_at(cp, SyntaxKind::FieldExpr);
                        self.finish_node();
                    }
                }
                SyntaxKind::LParen => {
                    self.start_node_at(cp, SyntaxKind::CallExpr);
                    self.bump();
                    if self.current_kind() != SyntaxKind::RParen {
                        self.parse_expr();
                        while self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                            if self.current_kind() == SyntaxKind::RParen {
                                break;
                            }
                            self.parse_expr();
                        }
                    }
                    self.expect(SyntaxKind::RParen);
                    self.finish_node();
                }
                SyntaxKind::LBracket => {
                    self.start_node_at(cp, SyntaxKind::IndexExpr);
                    self.bump();
                    self.parse_expr();
                    self.expect(SyntaxKind::RBracket);
                    self.finish_node();
                }
                SyntaxKind::Question => {
                    self.start_node_at(cp, SyntaxKind::TryExpr);
                    self.bump();
                    self.finish_node();
                }
                SyntaxKind::KwAs => {
                    self.start_node_at(cp, SyntaxKind::CastExpr);
                    self.bump();
                    self.parse_type();
                    self.finish_node();
                }
                SyntaxKind::LBrace if self.last_was_path && !self.suppress_struct_lit => {
                    self.start_node_at(cp, SyntaxKind::StructExpr);
                    self.bump(); // {
                    while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                        if self.current_kind() == SyntaxKind::Ident {
                            let field_cp = self.checkpoint();
                            // `current()` is `Some` here because the outer loop
                            // is guarded by `self.current().is_some()`; break on
                            // the (unreachable under that guard) `None` instead
                            // of `unwrap()` so EOF never panics the parser.
                            let field_name_token = match self.current() {
                                Some(t) => t.clone(),
                                None => break,
                            };
                            self.bump(); // field name
                            if self.current_kind() == SyntaxKind::Colon {
                                // Explicit field: name: expr
                                self.bump(); // colon
                                // Start the StructField node at the field name
                                // and nest the value expression INSIDE it, so the
                                // HIR lower (which reads the field value as a child
                                // of StructField) can find it. The shorthand branch
                                // below does the same; keeping the two shapes
                                // identical is what makes field collection work.
                                self.start_node_at(field_cp, SyntaxKind::StructField);
                                self.parse_expr(); // expression value (nested)
                                self.finish_node(); // StructField
                            } else {
                                // Shorthand field: name (same as expression)
                                self.start_node_at(field_cp, SyntaxKind::StructField);
                                // Emit the field name as a PathExpr for the value
                                self.start_node(SyntaxKind::PathExpr);
                                self.start_node(SyntaxKind::UsePath);
                                self.builder.token(
                                    GlyimLang::kind_to_raw(field_name_token.kind),
                                    field_name_token.text.as_str(),
                                );
                                self.finish_node(); // UsePath
                                self.finish_node(); // PathExpr
                                self.finish_node(); // StructField
                            }
                        } else if self.current_kind() == SyntaxKind::DotDot {
                            self.bump(); // ..
                            if self.current_kind() != SyntaxKind::RBrace {
                                self.parse_expr();
                            }
                        } else {
                            self.error("expected field name or '..'");
                            if self.current().is_some() {
                                self.bump();
                            }
                        }
                        if self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                        }
                    }
                    self.expect(SyntaxKind::RBrace);
                    self.finish_node(); // StructExpr
                }
                _ => break,
            }
        }
    }

    pub(crate) fn parse_path_expr(&mut self) {
        self.start_node(SyntaxKind::PathExpr);
        self.parse_path();
        self.finish_node();
        self.last_was_path = true;
    }

    pub(crate) fn parse_path(&mut self) {
        self.start_node(SyntaxKind::UsePath);
        self.parse_path_inner();
        self.finish_node();
    }

    pub(crate) fn parse_if_expr(&mut self) {
        self.start_node(SyntaxKind::IfExpr);
        self.bump_expected(SyntaxKind::KwIf);
        self.suppress_struct_lit = true;
        if self.current_kind() == SyntaxKind::KwLet {
            self.bump(); // let
            self.parse_pat();
            self.expect(SyntaxKind::Eq);
            self.parse_expr();
        } else {
            self.parse_expr();
        }
        self.suppress_struct_lit = false;
        self.parse_block();
        if self.current_kind() == SyntaxKind::KwElse {
            self.bump();
            if self.current_kind() == SyntaxKind::KwIf {
                self.parse_if_expr();
            } else {
                self.parse_block();
            }
        }
        self.finish_node();
    }

    pub(crate) fn parse_label(&mut self) -> bool {
        if (self.current_kind() == SyntaxKind::Ident || self.current_kind() == SyntaxKind::Lifetime)
            && self.peek_kind() == Some(SyntaxKind::Colon)
        {
            self.bump(); // label name
            self.bump(); // colon
            true
        } else {
            false
        }
    }

    pub(crate) fn parse_while_expr(&mut self) {
        self.start_node(SyntaxKind::WhileExpr);
        self.parse_label();
        self.bump(); // while
        self.suppress_struct_lit = true;
        if self.current_kind() == SyntaxKind::KwLet {
            self.bump(); // let
            self.parse_pat();
            self.expect(SyntaxKind::Eq);
            self.parse_expr();
        } else {
            self.parse_expr();
        }
        self.suppress_struct_lit = false;
        self.parse_block();
        self.finish_node();
    }

    pub(crate) fn parse_for_expr(&mut self) {
        self.start_node(SyntaxKind::ForExpr);
        self.parse_label();
        self.bump(); // for
        self.suppress_struct_lit = true;
        self.parse_pat();
        self.expect(SyntaxKind::KwIn);
        self.parse_expr();
        self.suppress_struct_lit = false;
        self.parse_block();
        self.finish_node();
    }

    pub(crate) fn parse_closure_expr(&mut self) {
        self.start_node(SyntaxKind::ClosureExpr);
        if self.current_kind() == SyntaxKind::KwMove {
            self.bump(); // move
        }
        // Opening pipe: a single `|` (Or) OR an empty-param `||` which the
        // lexer coalesces into one `OrOr` token. The latter carries both the
        // open and close pipe, so no separate closing pipe follows.
        let open_double = match self.current_kind() {
            SyntaxKind::Or => {
                self.bump();
                false
            }
            SyntaxKind::OrOr => {
                self.bump();
                true
            }
            _ => {
                self.error("expected '|' or '||' to start a closure");
                false
            }
        };
        self.start_node(SyntaxKind::ParamList);
        if !open_double {
            while self.current_kind() != SyntaxKind::Or && self.current().is_some() {
                self.start_node(SyntaxKind::Param);
                self.parse_pat_single();
                if self.current_kind() == SyntaxKind::Colon {
                    self.bump();
                    self.parse_type();
                }
                self.finish_node(); // Param
                if self.current_kind() == SyntaxKind::Comma {
                    self.bump();
                }
            }
            if self.current_kind() == SyntaxKind::Or {
                self.bump(); // closing pipe
            } else {
                self.error("expected '|' to close closure parameters");
            }
        }
        self.finish_node(); // ParamList
        if self.current_kind() == SyntaxKind::Arrow {
            self.bump();
            self.parse_type();
        }
        if self.current_kind() == SyntaxKind::LBrace {
            self.parse_block();
        } else {
            self.parse_expr();
        }
        self.finish_node(); // ClosureExpr
    }

    pub(crate) fn parse_match_arm(&mut self) {
        self.start_node(SyntaxKind::MatchArm);
        self.parse_pat();
        if self.current_kind() == SyntaxKind::KwIf {
            self.bump(); // if
            self.parse_expr(); // guard expression
        }
        self.expect(SyntaxKind::FatArrow);
        if self.current_kind() == SyntaxKind::LBrace {
            self.parse_block();
            if self.current_kind() == SyntaxKind::Comma {
                self.bump();
            }
        } else {
            self.parse_expr();
            if self.current_kind() == SyntaxKind::Comma {
                self.bump();
            }
        }
        self.finish_node();
    }

    pub(crate) fn parse_primary_expr(&mut self) {
        self.last_was_path = false;
        // Check for label before block or loop
        if (self.current_kind() == SyntaxKind::Ident || self.current_kind() == SyntaxKind::Lifetime)
            && self.peek_kind() == Some(SyntaxKind::Colon)
        {
            // This is a label - consume it, then parse the labeled item (loop, while, for, block)
            self.bump(); // label name
            self.bump(); // colon
            // Now parse the labeled expression
            match self.current_kind() {
                SyntaxKind::KwLoop => self.parse_loop_expr(),
                SyntaxKind::KwWhile => self.parse_while_expr(),
                SyntaxKind::KwFor => self.parse_for_expr(),
                SyntaxKind::LBrace => self.parse_block(),
                _ => {
                    self.error("expected loop, while, for, or block after label");
                }
            }
            return;
        }
        match self.current_kind() {
            SyntaxKind::Ident | SyntaxKind::KwSelf | SyntaxKind::KwSuper | SyntaxKind::KwCrate => {
                self.parse_path_expr();
            }
            SyntaxKind::IntLit
            | SyntaxKind::FloatLit
            | SyntaxKind::StringLit
            | SyntaxKind::CharLit
            | SyntaxKind::ByteLit
            | SyntaxKind::KwTrue
            | SyntaxKind::KwFalse => {
                self.start_node(SyntaxKind::LitExpr);
                self.bump();
                self.finish_node();
            }
            SyntaxKind::LParen => {
                let cp = self.checkpoint();
                self.bump(); // (
                if self.current_kind() == SyntaxKind::RParen {
                    self.bump(); // )
                    // empty tuple
                    self.start_node_at(cp, SyntaxKind::TupleExpr);
                    self.finish_node();
                } else {
                    self.parse_expr();
                    if self.current_kind() == SyntaxKind::Comma {
                        // Multiple elements: wrap in TupleExpr
                        self.start_node_at(cp, SyntaxKind::TupleExpr);
                        while self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                            self.parse_expr();
                        }
                        self.expect(SyntaxKind::RParen);
                        self.finish_node(); // TupleExpr
                    } else {
                        // Single expression: just parenthesized, not a tuple
                        self.expect(SyntaxKind::RParen);
                    }
                }
            }
            SyntaxKind::KwMove => {
                self.parse_closure_expr();
            }
            SyntaxKind::Or | SyntaxKind::OrOr => self.parse_closure_expr(),
            SyntaxKind::KwUnsafe => {
                self.bump(); // unsafe
                if self.current_kind() == SyntaxKind::LBrace {
                    self.parse_block();
                } else {
                    self.error("expected '{{' after unsafe");
                }
            }
            SyntaxKind::LBrace => self.parse_block(),
            SyntaxKind::KwIf => self.parse_if_expr(),
            SyntaxKind::KwWhile => self.parse_while_expr(),
            SyntaxKind::KwLoop => self.parse_loop_expr(),
            SyntaxKind::KwFor => self.parse_for_expr(),
            SyntaxKind::KwReturn => {
                self.start_node(SyntaxKind::ReturnExpr);
                self.bump(); // return
                if !matches!(
                    self.current_kind(),
                    SyntaxKind::Semicolon
                        | SyntaxKind::RBrace
                        | SyntaxKind::Comma
                        | SyntaxKind::FatArrow
                        | SyntaxKind::RParen
                        | SyntaxKind::RBracket
                        | SyntaxKind::Eq
                ) {
                    self.parse_expr();
                }
                self.finish_node(); // ReturnExpr
            }
            SyntaxKind::KwBreak => {
                self.start_node(SyntaxKind::BreakExpr);
                self.bump(); // break
                if !matches!(
                    self.current_kind(),
                    SyntaxKind::Semicolon
                        | SyntaxKind::RBrace
                        | SyntaxKind::Comma
                        | SyntaxKind::FatArrow
                        | SyntaxKind::RParen
                        | SyntaxKind::RBracket
                        | SyntaxKind::Eq
                ) {
                    self.parse_expr();
                }
                self.finish_node(); // BreakExpr
            }
            SyntaxKind::KwContinue => {
                self.start_node(SyntaxKind::ContinueExpr);
                self.bump(); // continue
                self.finish_node(); // ContinueExpr
            }
            SyntaxKind::LBracket => {
                self.start_node(SyntaxKind::ArrayExpr);
                self.bump(); // [
                if self.current_kind() == SyntaxKind::RBracket {
                    // empty array
                } else {
                    self.parse_expr();
                    if self.current_kind() == SyntaxKind::Semicolon {
                        self.bump(); // ;
                        self.parse_expr(); // count
                    } else {
                        while self.current_kind() == SyntaxKind::Comma {
                            self.bump();
                            if self.current_kind() == SyntaxKind::RBracket {
                                break;
                            }
                            self.parse_expr();
                        }
                    }
                }
                self.expect(SyntaxKind::RBracket);
                self.finish_node();
            }
            SyntaxKind::KwMatch => {
                self.start_node(SyntaxKind::MatchExpr);
                self.bump(); // match
                self.suppress_struct_lit = true;
                self.parse_expr();
                self.suppress_struct_lit = false;
                self.expect(SyntaxKind::LBrace);
                self.start_node(SyntaxKind::MatchArmList);
                while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                    self.parse_match_arm();
                }
                self.finish_node(); // MatchArmList
                self.expect(SyntaxKind::RBrace);
                self.finish_node(); // MatchExpr
            }
            _ => {
                self.error(format!(
                    "expected expression, found {:?}",
                    self.current_kind()
                ));
                if self.current().is_some() {
                    self.bump();
                }
            }
        }
    }

    pub(crate) fn parse_path_inner(&mut self) {
        // First segment
        match self.current_kind() {
            SyntaxKind::Ident | SyntaxKind::KwSelf | SyntaxKind::KwSuper | SyntaxKind::KwCrate => {
                self.bump();
            }
            _ => {
                self.error("expected identifier in path");
                return;
            }
        }
        while self.current_kind() == SyntaxKind::ColonColon {
            self.bump(); // ::
            // After :: we can have an identifier, generic args, or (for use statements) * or {
            match self.current_kind() {
                SyntaxKind::Ident
                | SyntaxKind::KwSelf
                | SyntaxKind::KwSuper
                | SyntaxKind::KwCrate => {
                    self.bump();
                }
                SyntaxKind::Lt => {
                    self.parse_type_arg_list();
                }
                // For use statements, we stop before * or { (handled by caller)
                SyntaxKind::Star | SyntaxKind::LBrace => {
                    break;
                }
                _ => {
                    self.error("expected identifier, '<', '*', or '{{' after '::'");
                    break;
                }
            }
        }
    }
    pub(crate) fn parse_loop_expr(&mut self) {
        self.start_node(SyntaxKind::LoopExpr);
        self.parse_label();
        self.bump(); // loop
        if self.current_kind() == SyntaxKind::LBrace {
            self.parse_block();
        } else {
            self.error("expected '{{' after loop");
        }
        self.finish_node();
    }
}
