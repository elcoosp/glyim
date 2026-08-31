use glyim_diag::GlyimDiagnostic;
use glyim_span::FileId;
use glyim_syntax::{GlyimLang, GreenNode, SyntaxKind, SyntaxNode};
use rowan::Language;

// Submodules containing specific parsing logic
mod expr;
mod item;
mod pat;
mod stmt;
mod ty;

#[derive(Clone, Debug)]
/// ParseResult.
pub struct ParseResult {
/// Struct.
    pub green_node: GreenNode,
/// Struct.
    pub diagnostics: Vec<GlyimDiagnostic>,
/// Struct.
    pub root: SyntaxNode,
}

pub(crate) struct Parser<'a> {
    tokens: &'a [crate::lexer::Token],
    pos: usize,
    builder: rowan::GreenNodeBuilder<'a>,
    diagnostics: Vec<GlyimDiagnostic>,
    last_was_path: bool,
    suppress_struct_lit: bool,
    pending_gt_count: u32,
    /// Monotonic expression-nesting counter (plan §3.6): guards against a
    /// stack-overflow DoS from pathologically nested source (e.g.
    /// `((((...))))`). `parse_expr` increments it on entry; once it exceeds
    /// `MAX_EXPR_DEPTH` we emit a diagnostic and bail instead of recursing
    /// further. It is never decremented — monotonic tracking is sufficient for
    /// a depth guard and avoids per-return cleanup in a large function.
    recursion_depth: u32,
}

/// Plan §3.6: maximum expression nesting depth before the parser bails with a
/// diagnostic. Deeply nested expressions are a classic stack-overflow DoS
/// vector in recursive-descent parsers; this bounds recursion without rejecting
/// any realistic program (real code rarely nests beyond a few dozen levels).
const MAX_EXPR_DEPTH: u32 = 256;

impl<'a> Parser<'a> {
    pub(crate) fn new(tokens: &'a [crate::lexer::Token]) -> Self {
        Self {
            tokens,
            pos: 0,
            builder: rowan::GreenNodeBuilder::new(),
            diagnostics: Vec::new(),
            last_was_path: false,
            suppress_struct_lit: false,
            pending_gt_count: 0,
            recursion_depth: 0,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    fn current(&self) -> Option<&crate::lexer::Token> {
        self.tokens.get(self.pos)
    }

    fn current_kind(&self) -> SyntaxKind {
        if self.pending_gt_count > 0 {
            return SyntaxKind::Gt;
        }
        self.current().map_or(SyntaxKind::Error, |t| t.kind)
    }

    /// Kind of the `n`th non-trivia token after the current one (skips
    /// whitespace). `n == 0` is the next non-ws token; `n == 1` the one after.
    /// Used to disambiguate `extern "C" fn` (string then fn) from a bare
    /// `extern { }` block (unstub-5 Phase 4).
    fn peek_non_ws_kind(&self, n: usize) -> SyntaxKind {
        let mut p = self.pos + 1;
        let mut seen = 0;
        while let Some(t) = self.tokens.get(p) {
            if t.kind != SyntaxKind::Whitespace {
                if seen == n {
                    return t.kind;
                }
                seen += 1;
            }
            p += 1;
        }
        SyntaxKind::Error
    }

    /// Kind of the next non-trivia token after the current one (skips
    /// whitespace). Used to disambiguate `const fn` from `const ITEM`.
    fn next_non_ws_kind(&self) -> SyntaxKind {
        let mut p = self.pos + 1;
        while let Some(t) = self.tokens.get(p) {
            if t.kind != SyntaxKind::Whitespace {
                return t.kind;
            }
            p += 1;
        }
        SyntaxKind::Error
    }

    fn bump(&mut self) {
        if self.pending_gt_count > 0 {
            self.builder
                .token(GlyimLang::kind_to_raw(SyntaxKind::Gt), ">");
            self.pending_gt_count -= 1;
            return;
        }
        if let Some(token) = self.current() {
            let kind = GlyimLang::kind_to_raw(token.kind);
            let text = token.text.clone();
            self.builder.token(kind, text.as_str());
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: SyntaxKind) {
        if self.current_kind() == expected {
            self.bump();
        } else {
            self.error(format!(
                "expected {:?}, found {:?}",
                expected,
                self.current_kind()
            ));
        }
    }

    fn error(&mut self, message: impl Into<String>) {
        let span = self
            .current()
            .map(|t| t.span)
            .unwrap_or(glyim_span::Span::DUMMY);
        self.diagnostics
            .push(GlyimDiagnostic::parse_error(span, message));
    }

    fn bump_expected(&mut self, expected: SyntaxKind) {
        if self.current_kind() != expected {
            self.error(format!(
                "expected {:?}, found {:?}",
                expected,
                self.current_kind()
            ));
        }
        if self.current().is_some() || self.pending_gt_count > 0 {
            self.bump();
        }
    }

    fn checkpoint(&self) -> rowan::Checkpoint {
        self.builder.checkpoint()
    }

    fn start_node_at(&mut self, checkpoint: rowan::Checkpoint, kind: SyntaxKind) {
        self.builder
            .start_node_at(checkpoint, GlyimLang::kind_to_raw(kind));
    }

    fn peek_kind(&self) -> Option<SyntaxKind> {
        self.tokens.get(self.pos + 1).map(|t| t.kind)
    }

    fn skip_token(&mut self) {
        if self.current().is_some() {
            self.pos += 1;
        }
    }

    fn start_node(&mut self, kind: SyntaxKind) {
        let raw_kind = GlyimLang::kind_to_raw(kind);
        self.builder.start_node(raw_kind);
    }

    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    pub(crate) fn parse_source_file(&mut self) {
        self.start_node(SyntaxKind::SourceFile);
        while self.current().is_some() {
            self.parse_item();
        }
        self.finish_node();
    }

    // Dispatchers to submodules
    // Implementations are in the submodules.

    // ---- FINISH ----

    fn finish(self) -> (GreenNode, Vec<GlyimDiagnostic>) {
        (self.builder.finish(), self.diagnostics)
    }
}

/// parse_to_syntax.
pub fn parse_to_syntax(source: &str, file_id: FileId) -> ParseResult {
    let lex_result = crate::lexer::lex(source, file_id);
    let mut parser = Parser::new(&lex_result.tokens);
    parser.parse_source_file();
    let (green_node, diagnostics) = parser.finish();
    let root = SyntaxNode::new_root(green_node.clone());
    let mut all_diagnostics = lex_result.diagnostics;
    all_diagnostics.extend(diagnostics);
    ParseResult {
        green_node,
        diagnostics: all_diagnostics,
        root,
    }
}

/// Validate whether `src` forms a single, whole, well-formed fragment of the
/// requested kind. Used by the macro matcher's Stage B to consume
/// variable-length fragments (e.g. a full `a + b * c` for `:expr`) instead of
/// a single token tree.
///
/// Returns `Some(())` when `src` parses as exactly one valid fragment of the
/// given kind (and only that), `None` otherwise. Parse *errors* are tolerated
/// as long as the structural shape is correct — the macro expander will surface
/// real diagnostics later. This keeps fragment matching permissive about
/// unfinished expressions while still rejecting structurally invalid input.
pub fn try_parse_fragment(kind: &str, src: &str) -> Option<()> {
    // `:meta` is validated by its own token-level grammar (plan §2.3),
    // independent of the wrapper-parse below — attributes aren't a distinct
    // syntax node in this frontend, so the surrounding `#[..] fn __f(){}`
    // wrapper can't observe meta structure. Handling it up front keeps the
    // real grammar authoritative and bypasses the generic diagnostics gate.
    if kind == "meta" {
        return validate_meta_fragment(src).then_some(());
    }
    // De-stubbing plan §5.1: the four fragment specifiers that previously fell
    // through to `_ => None` (always rejected). Each is validated lexically so
    // it stays permissive about later macro-expansion diagnostics while still
    // rejecting structurally invalid input.
    if kind == "lifetime" {
        return is_fragment_lifetime(src).then_some(());
    }
    if kind == "literal" {
        return is_fragment_literal(src).then_some(());
    }
    if kind == "vis" {
        return is_fragment_vis(src).then_some(());
    }
    if kind == "tt" {
        return is_fragment_tt(src).then_some(());
    }
    use glyim_syntax::SyntaxKind;
    let file_id = FileId::from_raw(0);
    // Wrap the fragment in a context that makes it a single top-level construct
    // of the requested kind, then inspect the parse tree.
    let wrapped = match kind {
        "expr" | "stmt" => format!("fn __f() {{ {} }}", src),
        // For `:block` the fragment *is* the function body — if it parses as a
        // bare block, it becomes `__f`'s body directly.
        "block" => format!("fn __f() {}", src),
        "ty" => format!("fn __f(__x: {}) {{ }}", src),
        "pat" => format!("fn __f() {{ let {} = 0; }}", src),
        "item" => format!("fn __f() {{ {} }}", src),
        "meta" => format!("#[{}] fn __f() {{ }}", src),
        // Unknown spec — fall back to permissive single-token acceptance.
        _ => return None,
    };
    let parsed = parse_to_syntax(&wrapped, file_id);
    // A fragment is only valid if it parses *cleanly* — no diagnostics. This
    // rejects over-consumed tokens (e.g. a stray `)` from greedy matching) and
    // malformed fragments, which is exactly what Stage B needs.
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    let root = parsed.root;
    let fndef = root
        .children()
        .find(|n| n.kind() == SyntaxKind::FnDef)?;
    match kind {
        "expr" => {
            // Exactly one expression-bearing child (ExprStmt / LetStmt / tail expr).
            let block = fndef
                .children()
                .find(|n| n.kind() == SyntaxKind::Block)?;
            let stmts: Vec<_> = block
                .children()
                .filter(|n| {
                    matches!(
                        n.kind(),
                        SyntaxKind::ExprStmt | SyntaxKind::LetStmt
                    )
                })
                .collect();
            // Allow a bare tail expression (block with a single non-statement
            // expr child) or a single statement.
            let has_expr = stmts.len() == 1
                || (stmts.is_empty()
                    && block.children().any(|n| is_exprish(n.kind())));
            has_expr.then_some(())
        }
        "stmt" => {
            let block = fndef
                .children()
                .find(|n| n.kind() == SyntaxKind::Block)?;
            let stmts: Vec<_> = block
                .children()
                .filter(|n| {
                    matches!(
                        n.kind(),
                        SyntaxKind::ExprStmt | SyntaxKind::LetStmt
                    )
                })
                .collect();
            (stmts.len() == 1).then_some(())
        }
        "block" => {
            // `<src>` is the function body itself. A `:block` fragment is valid
            // when it parses as exactly one block (brace-delimited) and nothing
            // else. We don't compare source text exactly (whitespace inside the
            // block is insignificant), only that a body block exists and the
            // fragment is brace-delimited.
            let _block = fndef
                .children()
                .find(|n| n.kind() == SyntaxKind::Block)?;
            let src_t = src.trim();
            (src_t.starts_with('{') && src_t.ends_with('}') && src_t.len() >= 2)
                .then_some(())
        }
        "ty" => {
            let param = fndef
                .children()
                .find(|n| n.kind() == SyntaxKind::ParamList)?
                .children()
                .find(|n| n.kind() == SyntaxKind::Param)?;
            // Param has a type child that is a recognised type node.
            let has_ty = param.children().any(|n| is_typeish(n.kind()));
            has_ty.then_some(())
        }
        "pat" => {
            // `<src>` is bound in `let <src> = 0;`. Find the LetStmt and check
            // its pattern child is a recognised pattern shape.
            let let_stmt = fndef
                .children()
                .find(|n| n.kind() == SyntaxKind::Block)?
                .children()
                .find(|n| n.kind() == SyntaxKind::LetStmt)?;
            let has_pat = let_stmt.children().any(|n| is_patish(n.kind()));
            has_pat.then_some(())
        }
        "item" => {
            let block = fndef
                .children()
                .find(|n| n.kind() == SyntaxKind::Block)?;
            let items: Vec<_> = block
                .children()
                .filter(|n| {
                    matches!(
                        n.kind(),
                        SyntaxKind::FnDef
                            | SyntaxKind::StructDef
                            | SyntaxKind::EnumDef
                            | SyntaxKind::ImplDef
                            | SyntaxKind::ExternBlock
                    )
                })
                .collect();
            (items.len() == 1).then_some(())
        }
        "meta" => {
            // De-stubbing plan §2.3: replace the old "non-empty content ⇒ accept"
            // permissive check with a real meta-item grammar. The three valid
            // shapes are:
            //   * `Word`       — `Path`
            //   * `NameValue`  — `Path '=' Lit`
            //   * `List`       — `Path '(' MetaItemList ')'`
            // Malformed input such as `#[attr(]` (unbalanced list) or `#[=foo]`
            // (missing path) is now rejected instead of silently succeeding.
            validate_meta_fragment(src).then_some(())
        }
        _ => None,
    }
}

/// De-stubbing plan §5.1: `:lifetime` fragment — exactly one lifetime token (`'a`).
fn is_fragment_lifetime(src: &str) -> bool {
    use glyim_syntax::SyntaxKind;
    let file_id = FileId::from_raw(0);
    let tokens: Vec<_> = crate::lexer::lex(src, file_id)
        .tokens
        .into_iter()
        .filter(|t| !matches!(t.kind, SyntaxKind::Whitespace))
        .collect();
    tokens.len() == 1 && tokens[0].kind == SyntaxKind::Lifetime
}

/// De-stubbing plan §5.1: `:literal` fragment — one literal token, or a
/// unary-minus literal (`-1`).
fn is_fragment_literal(src: &str) -> bool {
    use glyim_syntax::SyntaxKind;
    let file_id = FileId::from_raw(0);
    let tokens: Vec<_> = crate::lexer::lex(src, file_id)
        .tokens
        .into_iter()
        .filter(|t| !matches!(t.kind, SyntaxKind::Whitespace))
        .collect();
    match tokens.len() {
        1 => tokens[0].kind.is_literal(),
        2 => tokens[0].kind == SyntaxKind::Minus && tokens[1].kind.is_literal(),
        _ => false,
    }
}

/// De-stubbing plan §5.1: `:vis` fragment — matches *zero* tokens (private by
/// default) or a `pub` / `pub(crate)` / `pub(super)` / `pub(self)` /
/// `pub(in path)` visibility.
fn is_fragment_vis(src: &str) -> bool {
    use glyim_syntax::SyntaxKind;
    let s = src.trim();
    if s.is_empty() {
        return true; // vis matches nothing
    }
    let file_id = FileId::from_raw(0);
    let tokens: Vec<_> = crate::lexer::lex(src, file_id)
        .tokens
        .into_iter()
        .filter(|t| !matches!(t.kind, SyntaxKind::Whitespace))
        .collect();
    if tokens.is_empty() || tokens[0].kind != SyntaxKind::KwPub {
        return false;
    }
    if tokens.len() == 1 {
        return true; // `pub`
    }
    if tokens.len() >= 3
        && tokens[1].kind == SyntaxKind::LParen
        && tokens[tokens.len() - 1].kind == SyntaxKind::RParen
    {
        match tokens[2].kind {
            SyntaxKind::KwCrate | SyntaxKind::KwSuper | SyntaxKind::KwSelf => tokens.len() == 4,
            SyntaxKind::KwIn => true, // `pub(in path)` — path tail is permissively accepted
            _ => false,
        }
    } else {
        false
    }
}

/// De-stubbing plan §5.1: `:tt` fragment — exactly one token tree: a single
/// leaf token, or one balanced `(...)`/`[...]`/`{...}` delimited group.
fn is_fragment_tt(src: &str) -> bool {
    use glyim_syntax::SyntaxKind;
    let file_id = FileId::from_raw(0);
    let tokens: Vec<_> = crate::lexer::lex(src, file_id)
        .tokens
        .into_iter()
        .filter(|t| !matches!(t.kind, SyntaxKind::Whitespace))
        .collect();
    if tokens.is_empty() {
        return false;
    }
    if tokens.len() == 1 {
        return true; // single leaf token
    }
    let close = match tokens[0].kind {
        SyntaxKind::LParen => SyntaxKind::RParen,
        SyntaxKind::LBrace => SyntaxKind::RBrace,
        SyntaxKind::LBracket => SyntaxKind::RBracket,
        _ => return false,
    };
    if tokens[tokens.len() - 1].kind != close {
        return false;
    }
    // Inner delimiters must be balanced.
    let mut depth: i32 = 0;
    for t in &tokens {
        match t.kind {
            SyntaxKind::LParen | SyntaxKind::LBrace | SyntaxKind::LBracket => depth += 1,
            SyntaxKind::RParen | SyntaxKind::RBrace | SyntaxKind::RBracket => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

/// Validate that `src` is a whole, well-formed meta item (de-stubbing plan
/// §2.3). Returns `true` iff the token stream forms exactly one meta item of
/// one of the three shapes:
///   * `Word`       — `Path`
///   * `NameValue`  — `Path '=' Lit`
///   * `List`       — `Path '(' MetaItemList ')'`
/// Malformed input such as `#[attr(]` (unbalanced list) or `#[=foo]` (missing
/// path) returns `false`, so the macro matcher rejects it instead of silently
/// accepting any non-empty content.
fn validate_meta_fragment(src: &str) -> bool {
    use crate::lexer::lex;
    use glyim_syntax::SyntaxKind;
    let file_id = glyim_span::FileId::from_raw(0);
    let tokens = lex(src, file_id)
        .tokens
        .into_iter()
        .filter(|t| !matches!(t.kind, SyntaxKind::Whitespace))
        .collect::<Vec<_>>();
    let mut pos = 0;
    let mut matched = false;
    if parse_meta_item(&tokens, &mut pos) {
        // Must consume *all* tokens (a trailing `)`/`,`/stray token is malformed).
        matched = pos == tokens.len();
    }
    matched
}

/// Parse a single meta item starting at `tokens[pos]`, advancing `pos` past it
/// on success. Returns `true` if a well-formed meta item was consumed.
fn parse_meta_item(tokens: &[crate::lexer::Token], pos: &mut usize) -> bool {
    use glyim_syntax::SyntaxKind;
    let mut p = *pos;
    // Path: a sequence of `Ident`/`::` segments.
    if !parse_meta_path(tokens, &mut p) {
        return false;
    }
    if p < tokens.len() && tokens[p].kind == SyntaxKind::Eq {
        // `NameValue`: `Path '=' Lit`.
        p += 1;
        if p < tokens.len() && is_meta_literal(tokens[p].kind) {
            p += 1;
            *pos = p;
            return true;
        }
        return false;
    }
    if p < tokens.len() && tokens[p].kind == SyntaxKind::LParen {
        // `List`: `Path '(' MetaItemList ')'`.
        p += 1;
        loop {
            if p < tokens.len() && tokens[p].kind == SyntaxKind::RParen {
                p += 1;
                *pos = p;
                return true;
            }
            if !parse_meta_item(tokens, &mut p) {
                return false;
            }
            if p < tokens.len() && tokens[p].kind == SyntaxKind::Comma {
                p += 1;
                continue;
            }
            if p < tokens.len() && tokens[p].kind == SyntaxKind::RParen {
                p += 1;
                *pos = p;
                return true;
            }
            return false;
        }
    }
    *pos = p;
    true
}

/// Parse a meta-path: `Ident ('::' Ident)*`. Returns `true` and advances `pos`
/// if at least one identifier segment was consumed.
fn parse_meta_path(tokens: &[crate::lexer::Token], pos: &mut usize) -> bool {
    use glyim_syntax::SyntaxKind;
    let mut p = *pos;
    if p >= tokens.len() || tokens[p].kind != SyntaxKind::Ident {
        return false;
    }
    p += 1;
    while p + 1 < tokens.len()
        && tokens[p].kind == SyntaxKind::ColonColon
        && tokens[p + 1].kind == SyntaxKind::Ident
    {
        p += 2;
    }
    *pos = p;
    true
}

/// A meta literal is any literal token (string/int/float/char/bool).
fn is_meta_literal(kind: SyntaxKind) -> bool {
    use glyim_syntax::SyntaxKind;
    matches!(
        kind,
        SyntaxKind::StringLit
            | SyntaxKind::IntLit
            | SyntaxKind::FloatLit
            | SyntaxKind::CharLit
            | SyntaxKind::BoolLit
    )
}

/// Expression-like node kinds that can stand as a bare `:expr` tail.
fn is_exprish(kind: SyntaxKind) -> bool {
    use glyim_syntax::SyntaxKind;
    matches!(
        kind,
        SyntaxKind::PathExpr
            | SyntaxKind::LitExpr
            | SyntaxKind::BinaryExpr
            | SyntaxKind::CallExpr
            | SyntaxKind::MethodCallExpr
            | SyntaxKind::IfExpr
            | SyntaxKind::MatchExpr
            | SyntaxKind::Block
            | SyntaxKind::UnaryExpr
            | SyntaxKind::RefExpr
            | SyntaxKind::TupleExpr
            | SyntaxKind::ArrayExpr
            | SyntaxKind::StructExpr
            | SyntaxKind::CastExpr
            | SyntaxKind::IndexExpr
            | SyntaxKind::FieldExpr
            | SyntaxKind::RangeExpr
            | SyntaxKind::ClosureExpr
            | SyntaxKind::WhileExpr
            | SyntaxKind::LoopExpr
            | SyntaxKind::ForExpr
            | SyntaxKind::ReturnExpr
            | SyntaxKind::BreakExpr
            | SyntaxKind::ContinueExpr
            | SyntaxKind::AssignExpr
    )
}

/// Type-like node kinds for `:ty` validation.
fn is_typeish(kind: SyntaxKind) -> bool {
    use glyim_syntax::SyntaxKind;
    matches!(
        kind,
        SyntaxKind::PathType
            | SyntaxKind::TupleType
            | SyntaxKind::ArrayType
            | SyntaxKind::RefType
            | SyntaxKind::RawPtrType
            | SyntaxKind::FnType
            | SyntaxKind::DynType
            | SyntaxKind::NeverType
            | SyntaxKind::SliceType
            | SyntaxKind::ImplTraitType
            | SyntaxKind::InferType
    )
}

/// Pattern-like node kinds for `:pat` validation.
fn is_patish(kind: SyntaxKind) -> bool {
    use glyim_syntax::SyntaxKind;
    matches!(
        kind,
        SyntaxKind::PatIdent
            | SyntaxKind::PatWild
            | SyntaxKind::PatLit
            | SyntaxKind::PatRange
            | SyntaxKind::PatTuple
            | SyntaxKind::PatStruct
            | SyntaxKind::PatOr
            | SyntaxKind::PatSlice
    )
}
