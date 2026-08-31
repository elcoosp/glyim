use super::Parser;
use glyim_syntax::{GlyimLang, SyntaxKind};
use rowan::Language; // Required for kind_to_raw

impl<'a> Parser<'a> {
    pub(crate) fn parse_item(&mut self) {
        let _vis = self.parse_visibility();

        if self.current_kind() == SyntaxKind::KwUnsafe {
            self.bump(); // unsafe
            if !matches!(
                self.current_kind(),
                SyntaxKind::KwFn
                    | SyntaxKind::KwStruct
                    | SyntaxKind::KwUse
                    | SyntaxKind::KwEnum
                    | SyntaxKind::KwTrait
                    | SyntaxKind::KwImpl
                    | SyntaxKind::KwMod
                    | SyntaxKind::KwConst
                    | SyntaxKind::KwStatic
                    | SyntaxKind::KwType
                    | SyntaxKind::KwExtern
                    | SyntaxKind::KwAsync
                    | SyntaxKind::KwMacro
                    | SyntaxKind::KwMacroRules
            ) {
                self.error("expected item after 'unsafe'");
                return;
            }
        }

        // Route `const fn` and `async fn` to parse_fn_def, which consumes the
        // leading modifier token inside the FnDef node (plan §6.1). Each is
        // disambiguated from `const ITEM` / an `async` expression by peeking the
        // next non-trivia token.
        if (self.current_kind() == SyntaxKind::KwConst
            || self.current_kind() == SyntaxKind::KwAsync)
            && self.next_non_ws_kind() == SyntaxKind::KwFn
        {
            self.parse_fn_def();
            return;
        }

        match self.current_kind() {
            SyntaxKind::KwFn => self.parse_fn_def(),
            SyntaxKind::KwStruct => self.parse_struct_def(),
            SyntaxKind::KwUse => self.parse_use_decl(),
            SyntaxKind::KwEnum => self.parse_enum_def(),
            SyntaxKind::KwTrait => self.parse_trait_def(),
            SyntaxKind::KwImpl => self.parse_impl_def(),
            SyntaxKind::KwMod => {
                self.start_node(SyntaxKind::Module);
                self.bump(); // mod
                self.bump_expected(SyntaxKind::Ident);
                if self.current_kind() == SyntaxKind::LBrace {
                    self.bump(); // {
                    while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                        self.parse_item();
                    }
                    self.expect(SyntaxKind::RBrace);
                } else {
                    self.expect(SyntaxKind::Semicolon);
                }
                self.finish_node(); // Module
            }
            SyntaxKind::KwConst => {
                self.start_node(SyntaxKind::ConstDef);
                self.bump(); // const
                self.bump_expected(SyntaxKind::Ident);
                self.expect(SyntaxKind::Colon);
                self.parse_type();
                if self.current_kind() == SyntaxKind::Eq {
                    self.bump();
                    self.parse_expr();
                }
                self.expect(SyntaxKind::Semicolon);
                self.finish_node();
            }
            SyntaxKind::KwStatic => {
                self.start_node(SyntaxKind::StaticDef);
                self.bump(); // static
                if self.current_kind() == SyntaxKind::KwRef {
                    self.bump();
                }
                if self.current_kind() == SyntaxKind::KwMut {
                    self.bump();
                }
                self.bump_expected(SyntaxKind::Ident);
                self.expect(SyntaxKind::Colon);
                self.parse_type();
                if self.current_kind() == SyntaxKind::Eq {
                    self.bump();
                    self.parse_expr();
                }
                self.expect(SyntaxKind::Semicolon);
                self.finish_node();
            }
            SyntaxKind::KwType => {
                self.start_node(SyntaxKind::TypeAlias);
                self.bump(); // type
                self.bump_expected(SyntaxKind::Ident);
                if self.current_kind() == SyntaxKind::Lt {
                    self.parse_type_param_list();
                }
                if self.current_kind() == SyntaxKind::Eq {
                    self.bump();
                    self.parse_type();
                }
                self.expect(SyntaxKind::Semicolon);
                self.finish_node();
            }
            SyntaxKind::KwMacroRules | SyntaxKind::KwMacro => {
                self.parse_macro_def();
            }
            SyntaxKind::KwExtern => {
                // `extern "C" fn foo() {}` routes to parse_fn_def, which
                // consumes the `extern "C"` prefix. Otherwise treat as a bare
                // `extern { }` block (unstub-5 Phase 4).
                if self.next_non_ws_kind() == SyntaxKind::KwFn
                    || (self.current_kind() == SyntaxKind::KwExtern
                        && self.next_non_ws_kind() == SyntaxKind::StringLit
                        && self.peek_non_ws_kind(1) == SyntaxKind::KwFn)
                {
                    self.parse_fn_def();
                    return;
                }
                self.start_node(SyntaxKind::ExternBlock);
                self.bump(); // extern
                if self.current_kind() == SyntaxKind::StringLit {
                    self.bump(); // ABI string
                }
                if self.current_kind() == SyntaxKind::KwCrate {
                    self.bump(); // crate
                    self.bump_expected(SyntaxKind::Ident); // crate name
                    if self.current_kind() == SyntaxKind::KwAs {
                        self.bump(); // as
                        self.bump_expected(SyntaxKind::Ident); // alias
                    }
                } else if self.current_kind() == SyntaxKind::LBrace {
                    self.bump(); // {
                    while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                        match self.current_kind() {
                            SyntaxKind::KwFn => self.parse_fn_def(),
                            SyntaxKind::KwStatic => {
                                self.start_node(SyntaxKind::StaticDef);
                                self.bump(); // static
                                if self.current_kind() == SyntaxKind::KwMut {
                                    self.bump();
                                }
                                self.bump_expected(SyntaxKind::Ident);
                                self.expect(SyntaxKind::Colon);
                                self.parse_type();
                                self.expect(SyntaxKind::Semicolon);
                                self.finish_node();
                            }
                            _ => {
                                self.error("expected fn or static in extern block");
                                self.bump();
                            }
                        }
                    }
                    self.expect(SyntaxKind::RBrace);
                } else {
                    self.expect(SyntaxKind::Semicolon);
                }
                self.finish_node();
            }
            _ => {
                self.error(format!("expected item, found {:?}", self.current_kind()));
                while self.current().is_some()
                    && !matches!(
                        self.current_kind(),
                        SyntaxKind::KwFn
                            | SyntaxKind::KwStruct
                            | SyntaxKind::KwUse
                            | SyntaxKind::KwEnum
                            | SyntaxKind::KwTrait
                            | SyntaxKind::KwImpl
                            | SyntaxKind::KwMod
                            | SyntaxKind::KwConst
                            | SyntaxKind::KwStatic
                            | SyntaxKind::KwType
                            | SyntaxKind::KwExtern
                            | SyntaxKind::KwPub
                            | SyntaxKind::KwUnsafe
                            | SyntaxKind::KwMacroRules
                    )
                {
                    self.bump();
                }
            }
        }
    }

    pub(crate) fn parse_visibility(&mut self) -> bool {
        if self.current_kind() != SyntaxKind::KwPub {
            return false;
        }
        self.start_node(SyntaxKind::Visibility);
        self.bump(); // pub
        if self.current_kind() == SyntaxKind::LParen {
            self.bump(); // (
            match self.current_kind() {
                SyntaxKind::KwCrate => {
                    self.start_node(SyntaxKind::VisCrate);
                    self.bump();
                    self.finish_node();
                }
                SyntaxKind::KwSuper => {
                    self.start_node(SyntaxKind::VisSuper);
                    self.bump();
                    self.finish_node();
                }
                SyntaxKind::KwSelf => {
                    self.start_node(SyntaxKind::VisSelf);
                    self.bump();
                    self.finish_node();
                }
                SyntaxKind::Ident => {
                    self.start_node(SyntaxKind::VisPath);
                    self.parse_path();
                    self.finish_node();
                }
                _ => {
                    self.error("expected crate, super, self, or path in visibility");
                }
            }
            self.expect(SyntaxKind::RParen);
        }
        self.finish_node(); // Visibility
        true
    }

    pub(crate) fn parse_macro_def(&mut self) {
        self.start_node(SyntaxKind::MacroDef);
        // Two accepted forms with different token orders:
        //   `macro_rules! name { ... }`  -> macro_rules ! name
        //   `macro name! { ... }`        -> macro name !
        if self.current_kind() == SyntaxKind::KwMacroRules {
            self.bump_expected(SyntaxKind::KwMacroRules);
            self.bump_expected(SyntaxKind::Bang); // !
            self.bump_expected(SyntaxKind::Ident); // macro name
        } else {
            self.bump_expected(SyntaxKind::KwMacro);
            self.bump_expected(SyntaxKind::Ident); // macro name
            self.bump_expected(SyntaxKind::Bang); // !
        }
        self.expect(SyntaxKind::LBrace);
        while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
            // Skip separators between arms
            if matches!(
                self.current_kind(),
                SyntaxKind::Comma | SyntaxKind::Semicolon
            ) {
                self.bump();
                continue;
            }
            self.start_node(SyntaxKind::MacroArm);
            // Pattern: token tree (must be parenthesized token tree)
            self.parse_token_tree();
            self.expect(SyntaxKind::FatArrow);
            // Expansion: token tree (must be braced token tree)
            self.parse_token_tree();
            self.finish_node(); // MacroArm
            // Consume optional separator
            if matches!(
                self.current_kind(),
                SyntaxKind::Comma | SyntaxKind::Semicolon
            ) {
                self.bump();
            }
        }
        self.expect(SyntaxKind::RBrace);
        self.finish_node(); // MacroDef
    }

    pub(crate) fn parse_token_tree(&mut self) {
        match self.current_kind() {
            SyntaxKind::LParen => {
                self.start_node(SyntaxKind::TokenTree);
                self.bump(); // (
                while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                    if self.current_kind() == SyntaxKind::Dollar {
                        self.start_node(SyntaxKind::TokenTree);
                        self.bump(); // $
                        if self.current_kind() == SyntaxKind::LParen {
                            self.parse_token_tree(); // $(...)
                            if matches!(
                                self.current_kind(),
                                SyntaxKind::Comma | SyntaxKind::Semicolon
                            ) {
                                self.bump(); // separator
                            }
                            if matches!(
                                self.current_kind(),
                                SyntaxKind::Plus | SyntaxKind::Star | SyntaxKind::Question
                            ) {
                                self.bump(); // operator
                            } else {
                                self.error("expected repetition operator after $(...)");
                            }
                        } else if self.current_kind() == SyntaxKind::Ident {
                            let name_cp = self.checkpoint();
                            self.bump(); // variable name
                            if self.current_kind() == SyntaxKind::Colon {
                                self.bump(); // :
                                if self.current_kind() == SyntaxKind::Ident {
                                    self.start_node_at(name_cp, SyntaxKind::MetaVar);
                                    self.bump(); // fragment specifier
                                    self.finish_node();
                                } else {
                                    self.error("expected fragment specifier after ':'");
                                }
                            } else {
                                self.start_node_at(name_cp, SyntaxKind::MetaVar);
                                self.finish_node();
                            }
                        } else if self.current_kind() == SyntaxKind::KwCrate {
                            self.start_node(SyntaxKind::MetaVarCrate);
                            self.bump();
                            self.finish_node();
                        } else {
                            self.error("expected '(' or identifier after '$'");
                            self.bump();
                        }
                        self.finish_node(); // TokenTree for dollar
                    } else {
                        if matches!(
                            self.current_kind(),
                            SyntaxKind::LParen | SyntaxKind::LBrace | SyntaxKind::LBracket
                        ) {
                            self.parse_token_tree();
                        } else {
                            self.start_node(SyntaxKind::TokenTree);
                            self.bump();
                            self.finish_node();
                        }
                    }
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RParen);
                self.finish_node(); // TokenTree group
            }
            SyntaxKind::LBrace => {
                self.start_node(SyntaxKind::TokenTree);
                self.bump();
                while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                    if matches!(
                        self.current_kind(),
                        SyntaxKind::LParen | SyntaxKind::LBrace | SyntaxKind::LBracket
                    ) {
                        self.parse_token_tree();
                    } else {
                        self.start_node(SyntaxKind::TokenTree);
                        self.bump();
                        self.finish_node();
                    }
                }
                self.expect(SyntaxKind::RBrace);
                self.finish_node();
            }
            SyntaxKind::LBracket => {
                self.start_node(SyntaxKind::TokenTree);
                self.bump();
                while self.current_kind() != SyntaxKind::RBracket && self.current().is_some() {
                    if matches!(
                        self.current_kind(),
                        SyntaxKind::LParen | SyntaxKind::LBrace | SyntaxKind::LBracket
                    ) {
                        self.parse_token_tree();
                    } else {
                        self.start_node(SyntaxKind::TokenTree);
                        self.bump();
                        self.finish_node();
                    }
                }
                self.expect(SyntaxKind::RBracket);
                self.finish_node();
            }
            _ => {
                self.start_node(SyntaxKind::TokenTree);
                self.bump();
                self.finish_node();
            }
        }
    }

    pub(crate) fn parse_fn_def(&mut self) {
        self.start_node(SyntaxKind::FnDef);
        // Consume a leading `extern "C"` FFI prefix so lowering can read the
        // ABI (unstub-5 Phase 4). `extern` is only the fn prefix when `fn`
        // follows (possibly after an ABI string literal).
        if self.current_kind() == SyntaxKind::KwExtern {
            self.bump(); // extern
            if self.current_kind() == SyntaxKind::StringLit {
                self.bump(); // "C" / ABI string
            }
        }
        // Consume a leading `const` or `async` fn modifier inside the FnDef
        // node so lowering can detect it (plan §6.1). The modifier precedes
        // `fn`, so we only treat it as the fn modifier when `fn` follows; an
        // `async` expression or `const { ... }` block is handled elsewhere.
        if (self.current_kind() == SyntaxKind::KwConst
            || self.current_kind() == SyntaxKind::KwAsync)
            && self.next_non_ws_kind() == SyntaxKind::KwFn
        {
            self.bump(); // const | async
        }
        self.bump_expected(SyntaxKind::KwFn);
        self.bump_expected(SyntaxKind::Ident);
        if self.current_kind() == SyntaxKind::Lt {
            self.parse_type_param_list();
        }
        self.expect(SyntaxKind::LParen);
        if self.current_kind() != SyntaxKind::RParen {
            self.start_node(SyntaxKind::ParamList);
            while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                self.parse_param();
                if self.current_kind() == SyntaxKind::Comma {
                    self.bump();
                }
            }
            self.finish_node(); // ParamList
        }
        self.expect(SyntaxKind::RParen);
        if self.current_kind() == SyntaxKind::Arrow {
            self.bump();
            self.parse_type();
        }
        if self.current_kind() == SyntaxKind::KwWhere {
            self.parse_where_clause();
        }
        if self.current_kind() == SyntaxKind::LBrace {
            self.parse_block();
        } else {
            self.expect(SyntaxKind::Semicolon);
        }
        self.finish_node(); // FnDef
    }

    pub(crate) fn parse_param(&mut self) {
        self.start_node(SyntaxKind::Param);
        if self.current_kind() == SyntaxKind::And {
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
            if self.current_kind() == SyntaxKind::KwSelf {
                // `&mut self` / `&self`: build a `RefType` node whose inner is
                // produced by `parse_type` (which yields the correct
                // `PathType(self)` child). This makes the receiver reference
                // survive to typeck as `&mut Self` / `&Self`. Without it,
                // `lower_param` sees no recognised type node and the receiver
                // is typed as the pointee, breaking every `&self` / `&mut self`
                // method.
                self.start_node(SyntaxKind::RefType);
                self.parse_type();
                self.finish_node(); // RefType
                self.finish_node(); // Param
                return;
            }
            // `&'a T` / `&mut T` named parameter: wrap the pointee in a
            // RefType so the parameter type survives to typeck.
            self.start_node(SyntaxKind::RefType);
            self.parse_type();
            self.finish_node(); // RefType
            self.finish_node(); // Param
            self.bump_expected(SyntaxKind::Ident);
            self.expect(SyntaxKind::Colon);
            self.parse_type();
        } else if self.current_kind() == SyntaxKind::KwMut {
            self.bump(); // mut
            if self.current_kind() == SyntaxKind::KwSelf {
                self.bump(); // self
                // `mut self` (by-value mutable self): no reference — leave the
                // receiver as the bare pointee (matches Rust `fn f(mut self)`).
                self.finish_node(); // Param
                return;
            }
            self.bump_expected(SyntaxKind::Ident);
            self.expect(SyntaxKind::Colon);
            self.parse_type();
        } else if self.current_kind() == SyntaxKind::KwSelf {
            self.bump(); // self
            // `self` (by-value): bare pointee, no reference.
            self.finish_node(); // Param
            return;
        } else {
            self.bump_expected(SyntaxKind::Ident);
            self.expect(SyntaxKind::Colon);
            self.parse_type();
        }
        self.finish_node(); // Param
    }

    pub(crate) fn parse_struct_def(&mut self) {
        self.start_node(SyntaxKind::StructDef);
        self.bump_expected(SyntaxKind::KwStruct);
        self.bump_expected(SyntaxKind::Ident);
        if self.current_kind() == SyntaxKind::Lt {
            self.parse_type_param_list();
        }
        if self.current_kind() == SyntaxKind::KwWhere {
            self.parse_where_clause();
        }
        match self.current_kind() {
            SyntaxKind::LParen => {
                self.bump(); // (
                while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                    // Tuple-struct fields may carry a visibility modifier.
                    self.parse_visibility();
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RParen);
                self.expect(SyntaxKind::Semicolon);
            }
            SyntaxKind::LBrace => {
                self.bump(); // {
                while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                    self.parse_visibility();
                    if self.current_kind() == SyntaxKind::Ident {
                        self.bump();
                        if self.current_kind() == SyntaxKind::Colon {
                            self.bump();
                            self.parse_type();
                        }
                    } else {
                        self.error("expected field name");
                        self.bump();
                    }
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RBrace);
            }
            _ => {
                self.expect(SyntaxKind::Semicolon);
            }
        }
        self.finish_node(); // StructDef
    }

    pub(crate) fn parse_enum_def(&mut self) {
        self.start_node(SyntaxKind::EnumDef);
        self.bump_expected(SyntaxKind::KwEnum);
        self.bump_expected(SyntaxKind::Ident);
        if self.current_kind() == SyntaxKind::Lt {
            self.parse_type_param_list();
        }
        if self.current_kind() == SyntaxKind::KwWhere {
            self.parse_where_clause();
        }
        self.expect(SyntaxKind::LBrace);
        self.start_node(SyntaxKind::VariantList);
        while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
            self.start_node(SyntaxKind::EnumVariant);
            self.bump_expected(SyntaxKind::Ident);
            if self.current_kind() == SyntaxKind::LParen {
                self.bump(); // (
                while self.current_kind() != SyntaxKind::RParen && self.current().is_some() {
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.expect(SyntaxKind::RParen);
            }
            if self.current_kind() == SyntaxKind::LBrace {
                self.start_node(SyntaxKind::FieldList);
                self.bump(); // {
                while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                    self.start_node(SyntaxKind::StructField);
                    if self.current_kind() == SyntaxKind::Ident {
                        self.bump(); // field name
                        if self.current_kind() == SyntaxKind::Colon {
                            self.bump(); // :
                            self.parse_type();
                        }
                    } else {
                        self.error("expected field name");
                        if self.current().is_some() {
                            self.bump();
                        }
                    }
                    self.finish_node(); // StructField
                    if self.current_kind() == SyntaxKind::Comma {
                        self.bump();
                    }
                }
                self.finish_node(); // FieldList
                self.expect(SyntaxKind::RBrace);
            }
            if self.current_kind() == SyntaxKind::Eq {
                self.bump();
                self.parse_expr();
            }
            self.finish_node(); // EnumVariant
            if self.current_kind() == SyntaxKind::Comma {
                self.bump();
            }
        }
        self.finish_node(); // VariantList
        self.expect(SyntaxKind::RBrace);
        self.finish_node(); // EnumDef
    }

    pub(crate) fn parse_trait_def(&mut self) {
        self.start_node(SyntaxKind::TraitDef);
        self.bump_expected(SyntaxKind::KwTrait);
        self.bump_expected(SyntaxKind::Ident);
        if self.current_kind() == SyntaxKind::Lt {
            self.parse_type_param_list();
        }
        if self.current_kind() == SyntaxKind::Colon {
            self.bump(); // :
            loop {
                self.parse_type();
                if self.current_kind() == SyntaxKind::Plus {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        if self.current_kind() == SyntaxKind::KwWhere {
            self.parse_where_clause();
        }
        self.expect(SyntaxKind::LBrace);
        while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
            match self.current_kind() {
                SyntaxKind::KwFn => self.parse_fn_def(),
                SyntaxKind::KwType => {
                    self.start_node(SyntaxKind::TypeAlias);
                    self.bump(); // type
                    self.bump_expected(SyntaxKind::Ident);
                    if self.current_kind() == SyntaxKind::Colon {
                        self.bump();
                        loop {
                            self.parse_type();
                            if self.current_kind() == SyntaxKind::Plus {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(SyntaxKind::Semicolon);
                    self.finish_node();
                }
                SyntaxKind::KwConst => {
                    self.start_node(SyntaxKind::ConstDef);
                    self.bump(); // const
                    self.bump_expected(SyntaxKind::Ident);
                    self.expect(SyntaxKind::Colon);
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Eq {
                        self.bump();
                        self.parse_expr();
                    }
                    self.expect(SyntaxKind::Semicolon);
                    self.finish_node();
                }
                SyntaxKind::KwExtern => self.parse_extern_block(),
                SyntaxKind::KwUnsafe => self.parse_item(),
                _ => {
                    self.error(format!(
                        "expected trait item, found {:?}",
                        self.current_kind()
                    ));
                    self.bump();
                }
            }
        }
        self.expect(SyntaxKind::RBrace);
        self.finish_node(); // TraitDef
    }

    pub(crate) fn parse_extern_block(&mut self) {
        // `extern "C" { ... }` foreign block (used as an impl/trait item or
        // statement). The `extern "C" fn foo()` form is handled by parse_fn_def.
        self.start_node(SyntaxKind::ExternBlock);
        self.bump(); // extern
        if self.current_kind() == SyntaxKind::StringLit {
            self.bump(); // ABI string
        }
        if self.current_kind() == SyntaxKind::LBrace {
            self.bump(); // {
            while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                match self.current_kind() {
                    SyntaxKind::KwFn => self.parse_fn_def(),
                    SyntaxKind::KwStatic => {
                        self.start_node(SyntaxKind::StaticDef);
                        self.bump(); // static
                        if self.current_kind() == SyntaxKind::KwMut {
                            self.bump();
                        }
                        self.bump_expected(SyntaxKind::Ident);
                        self.expect(SyntaxKind::Colon);
                        self.parse_type();
                        self.expect(SyntaxKind::Semicolon);
                        self.finish_node();
                    }
                    _ => {
                        self.error("expected fn or static in extern block");
                        self.bump();
                    }
                }
            }
            self.expect(SyntaxKind::RBrace);
        } else {
            self.expect(SyntaxKind::Semicolon);
        }
        self.finish_node();
    }

    pub(crate) fn parse_impl_def(&mut self) {
        self.start_node(SyntaxKind::ImplDef);
        self.bump_expected(SyntaxKind::KwImpl);
        if self.current_kind() == SyntaxKind::Lt {
            self.parse_type_param_list();
        }
        self.parse_type();
        if self.current_kind() == SyntaxKind::KwFor {
            self.bump(); // for
            self.parse_type();
        }
        if self.current_kind() == SyntaxKind::KwWhere {
            self.parse_where_clause();
        }
        self.expect(SyntaxKind::LBrace);
        while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
            match self.current_kind() {
                SyntaxKind::KwFn => self.parse_fn_def(),
                SyntaxKind::KwType => {
                    self.start_node(SyntaxKind::TypeAlias);
                    self.bump(); // type
                    self.bump_expected(SyntaxKind::Ident);
                    if self.current_kind() == SyntaxKind::Eq {
                        self.bump();
                        self.parse_type();
                    }
                    self.expect(SyntaxKind::Semicolon);
                    self.finish_node();
                }
                SyntaxKind::KwConst => {
                    self.start_node(SyntaxKind::ConstDef);
                    self.bump(); // const
                    self.bump_expected(SyntaxKind::Ident);
                    self.expect(SyntaxKind::Colon);
                    self.parse_type();
                    self.expect(SyntaxKind::Eq);
                    self.parse_expr();
                    self.expect(SyntaxKind::Semicolon);
                    self.finish_node();
                }
                SyntaxKind::KwExtern => self.parse_extern_block(),
                SyntaxKind::KwUnsafe => self.parse_item(),
                _ => {
                    self.error(format!(
                        "expected impl item, found {:?}",
                        self.current_kind()
                    ));
                    self.bump();
                }
            }
        }
        self.expect(SyntaxKind::RBrace);
        self.finish_node(); // ImplDef
    }

    pub(crate) fn parse_where_clause(&mut self) {
        self.start_node(SyntaxKind::WhereClause);
        self.expect(SyntaxKind::KwWhere);
        loop {
            self.start_node(SyntaxKind::WherePredicate);
            self.parse_type(); // type or associated type binding
            if self.current_kind() == SyntaxKind::Colon {
                self.bump(); // :
                loop {
                    self.start_node(SyntaxKind::Bound);
                    self.parse_type(); // bound (trait with args)
                    self.finish_node(); // Bound
                    if self.current_kind() == SyntaxKind::Plus {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            self.finish_node(); // WherePredicate
            if self.current_kind() == SyntaxKind::Comma {
                self.bump();
                // A trailing comma before the function body/terminator is valid
                // (e.g. `where T: Foo, {`). Only continue if another predicate
                // actually follows.
                match self.current_kind() {
                    SyntaxKind::Ident
                    | SyntaxKind::Lifetime
                    | SyntaxKind::KwSelf
                    | SyntaxKind::KwSuper
                    | SyntaxKind::KwCrate => continue,
                    _ => break,
                }
            }
            break;
        }
        self.finish_node(); // WhereClause
    }

    pub(crate) fn parse_type_param_list(&mut self) {
        self.start_node(SyntaxKind::TypeParamList);
        self.expect(SyntaxKind::Lt);
        while self.current_kind() != SyntaxKind::Gt
            && self.current_kind() != SyntaxKind::Shr
            && self.current().is_some()
        {
            self.start_node(SyntaxKind::TypeParam);
            // A lifetime parameter (`'a`) is also valid here: `impl<'a, T>`.
            // Bump it as a bare Lifetime leaf; the TypeParam node still wraps it.
            if self.current_kind() == SyntaxKind::Lifetime {
                self.bump();
            } else {
                self.bump_expected(SyntaxKind::Ident);
            }
            if self.current_kind() == SyntaxKind::Colon {
                self.bump();
                loop {
                    self.parse_type();
                    if self.current_kind() == SyntaxKind::Plus {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            if self.current_kind() == SyntaxKind::Eq {
                self.bump();
                self.parse_type();
            }
            self.finish_node(); // TypeParam
            if self.current_kind() == SyntaxKind::Comma {
                self.bump();
            }
        }
        // Handle >> as two Gt tokens: emit both, one for this list, one queued
        if self.current_kind() == SyntaxKind::Shr {
            self.builder
                .token(GlyimLang::kind_to_raw(SyntaxKind::Gt), ">");
            self.builder
                .token(GlyimLang::kind_to_raw(SyntaxKind::Gt), ">");
            self.pos += 1; // skip the Shr token
            self.pending_gt_count += 1;
            // Do NOT consume from pending here.
            self.finish_node();
            return;
        }
        // Not Shr - consume a real Gt token
        if self.current_kind() == SyntaxKind::Gt {
            self.bump();
        }
        self.finish_node();
    }

    pub(crate) fn parse_type_arg_list(&mut self) {
        self.bump(); // <
        while self.current_kind() != SyntaxKind::Gt
            && self.current_kind() != SyntaxKind::Shr
            && self.current().is_some()
        {
            self.parse_type();
            // Associated-type binding: `Trait<Assoc = T>` (e.g.
            // `IntoIterator<Item = T>`). The binder `Ident = Type` is a single
            // generic argument.
            if self.current_kind() == SyntaxKind::Eq {
                self.bump(); // =
                self.parse_type();
            }
            if self.current_kind() == SyntaxKind::Comma {
                self.bump();
            }
        }
        // Handle >> as two Gt tokens:
        // Emit both now. The first closes this list, the second is queued for the outer list.
        if self.current_kind() == SyntaxKind::Shr {
            // Emit Gt for THIS list (closes this type arg list)
            self.builder
                .token(GlyimLang::kind_to_raw(SyntaxKind::Gt), ">");
            // Emit Gt for the OUTER list (queued via pending_gt_count)
            self.builder
                .token(GlyimLang::kind_to_raw(SyntaxKind::Gt), ">");
            self.pos += 1; // skip the Shr token
            self.pending_gt_count += 1; // outer list will see Gt via current_kind
            // Do NOT consume from pending here - the Gt for this list was already emitted.
            return;
        }
        // Not Shr - consume a real Gt token
        if self.current_kind() == SyntaxKind::Gt {
            self.bump();
        }
    }

    pub(crate) fn parse_use_decl(&mut self) {
        self.start_node(SyntaxKind::UseDecl);
        self.bump_expected(SyntaxKind::KwUse);
        self.parse_use_tree();
        self.expect(SyntaxKind::Semicolon);
        self.finish_node();
    }

    pub(crate) fn parse_use_tree(&mut self) {
        self.start_node(SyntaxKind::UseTree);

        // Parse the path (could be simple or qualified)
        self.parse_use_path();

        // Handle glob import `*`
        if self.current_kind() == SyntaxKind::Star {
            self.bump(); // *
        }

        // Handle alias `as`
        if self.current_kind() == SyntaxKind::KwAs {
            self.bump(); // as
            if self.current_kind() == SyntaxKind::Ident
                || self.current_kind() == SyntaxKind::Underscore
            {
                self.bump();
            } else {
                self.error("expected identifier or `_` after `as`");
            }
        }

        // Handle nested imports `{a, b, c}`
        if self.current_kind() == SyntaxKind::LBrace {
            self.bump(); // {
            while self.current_kind() != SyntaxKind::RBrace && self.current().is_some() {
                self.parse_use_tree();
                if self.current_kind() == SyntaxKind::Comma {
                    self.bump();
                }
            }
            self.expect(SyntaxKind::RBrace);
        }

        self.finish_node();
    }

    pub(crate) fn parse_use_path(&mut self) {
        // For use declarations, we need to parse the full path including :: segments
        self.start_node(SyntaxKind::UsePath);

        // First segment
        match self.current_kind() {
            SyntaxKind::Ident | SyntaxKind::KwSelf | SyntaxKind::KwSuper | SyntaxKind::KwCrate => {
                self.bump();
            }
            _ => {
                self.error("expected identifier in use path");
                return;
            }
        }

        // Continue with :: segments
        while self.current_kind() == SyntaxKind::ColonColon {
            self.bump(); // ::
            // After :: we can have an identifier, * (glob), or { for nested imports
            if self.current_kind() == SyntaxKind::Star || self.current_kind() == SyntaxKind::LBrace
            {
                break;
            }
            match self.current_kind() {
                SyntaxKind::Ident
                | SyntaxKind::KwSelf
                | SyntaxKind::KwSuper
                | SyntaxKind::KwCrate => {
                    self.bump();
                }
                _ => {
                    self.error("expected identifier, '*', or '{{' after '::'");
                    break;
                }
            }
        }

        self.finish_node();
    }
}
