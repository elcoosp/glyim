//! The slot/handler expression model (`ExprModel`) and its extraction from
//! the CST (design doc §7.4).
//!
//! GSX splits string literals into static pieces + typed holes *at parse
//! time* so each hole's read-set is syntactically visible (§3.2). Likewise a
//! handler is a state transformer `fn(&mut S, E)` (§7.3) whose write-set must
//! be statically computable. Both operate on this small expression tree.
//!
//! The model covers the expression forms views and handlers actually use:
//! paths rooted at the state parameter (`s.count`, `s.user.name`), field and
//! index projections (including `s.items[i]` for §7.6), method and free
//! calls (`s.history.iter()`, `trusted(html)` for G6), closures, literals,
//! operators, `if`/`match`/`for` blocks, and assignments/compound
//! assignments. Anything the model cannot see into (calls into unknown
//! functions, `dyn` dispatch) is kept opaque and analyses treat it
//! conservatively (§7.4: "conservatively the whole subtree at the argument
//! place if the callee is opaque").

use glyim_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// A small expression tree, expressive enough for slot reads and handler
/// bodies.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A plain name: `s`, `item`, `self`, an event parameter, a loop
    /// variable, or a keyword literal (`true`/`false` is [`Expr::Bool`]).
    Var(String),
    /// A literal integer / float / string / char / bool / unit.
    Lit(Lit),
    /// Field access `base.name`.
    Field(Box<Expr>, String),
    /// Index `base[index]` — §7.6's `s.items[i]`.
    Index(Box<Expr>, Box<Expr>),
    /// Free function call `f(args…)`.
    Call(String, Vec<Expr>),
    /// Method call `recv.name(args…)`.
    Method(Box<Expr>, String, Vec<Expr>),
    /// Closure `|a, b| body` (handlers, §7.3).
    Closure {
        /// Parameter names.
        params: Vec<String>,
        /// Body expression.
        body: Box<Expr>,
    },
    /// Unary `-e`, `!e`, `*e`, `&e`, `&mut e`.
    Unary(UnOp, Box<Expr>),
    /// Binary `a op b`.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// Assignment `lhs = rhs` and compound `lhs op= rhs` (handler writes).
    Assign {
        /// Written place.
        lhs: Box<Expr>,
        /// Read once for `op = Some(op)`.
        op: Option<BinOp>,
        /// Value.
        rhs: Box<Expr>,
    },
    /// `if cond { then } else { els }` — regions (§6.3) and handlers.
    If {
        /// Condition.
        cond: Box<Expr>,
        /// Then block.
        then: Box<Expr>,
        /// Else block/expr.
        els: Option<Box<Expr>>,
    },
    /// `match scrutinee { arms }` — regions (§6.3).
    Match {
        /// Scrutinee.
        scrutinee: Box<Expr>,
        /// `(pattern-text, guard, body)` per arm.
        arms: Vec<(String, Option<Expr>, Expr)>,
    },
    /// `for pat in iter by key { body }` — keyed list region (§6.3, §7.6).
    For {
        /// Pattern text (`item`).
        pat: String,
        /// Iterable expression.
        iter: Box<Expr>,
        /// Optional `by` key expression.
        key: Option<Box<Expr>>,
        /// Body block.
        body: Box<Expr>,
    },
    /// A `{ … }` block: statements + optional tail.
    Block {
        /// Statement expressions (incl. `let`-shaped items flattened to
        /// [`Expr::Let`]).
        stmts: Vec<Expr>,
        /// Tail expression.
        tail: Option<Box<Expr>>,
    },
    /// `let pat = value;` — a new local binding inside a handler.
    Let {
        /// Pattern text (binding name).
        pat: String,
        /// Initializer.
        value: Box<Expr>,
    },
    /// `break`/`continue`/`return` markers (opaque for analysis).
    Flow,
    /// Something the model deliberately keeps opaque: `dyn` calls, FFI,
    /// macros. Analyses treat an opaque node as reading/writing everything
    /// under its argument places (§7.4).
    Opaque(String),
    /// A nested GSX element (§2 principle 1: structure is never erased —
    /// region arms keep their element trees; the VIR builder consumes them).
    Gsx(Box<crate::ast::GsxNode>),
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `-`.
    Neg,
    /// `!`.
    Not,
    /// `*`.
    Deref,
    /// `&`.
    Ref,
    /// `&mut`.
    RefMut,
}

/// Binary operators (analysis-relevant subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `<`.
    Lt,
    /// `>`.
    Gt,
    /// `<=`.
    Le,
    /// `>=`.
    Ge,
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%`.
    Rem,
    /// `&&`.
    And,
    /// `||`.
    Or,
}

/// Literals.
#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    /// Signed / unsuffixed integer.
    Int(i128),
    /// Unsigned integer.
    Uint(u128),
    /// Float (bit-preserving through f64).
    Float(f64),
    /// String literal with escapes already resolved.
    Str(String),
    /// Character.
    Char(char),
    /// `true`/`false`.
    Bool(bool),
    /// `()`.
    Unit,
}

impl Expr {
    /// `s.count` shorthand.
    pub fn field(base: &str, name: &str) -> Expr {
        Expr::Field(Box::new(Expr::Var(base.to_string())), name.to_string())
    }

    /// Walk sub-expressions (order is structural).
    pub fn children(&self) -> Vec<&Expr> {
        fn e(x: &Expr) -> &Expr {
            x
        }
        match self {
            Expr::Var(_) | Expr::Lit(_) | Expr::Flow | Expr::Opaque(_) | Expr::Gsx(_) => vec![],
            Expr::Field(b, _) | Expr::Unary(_, b) => vec![e(b)],
            Expr::Index(b, i) | Expr::Binary(_, b, i) => vec![e(b), e(i)],
            Expr::Call(_, a) => a.iter().collect(),
            Expr::Method(_, _, a) => a.iter().collect(),
            Expr::Closure { body, .. } => vec![e(body)],
            Expr::Assign { lhs, rhs, .. } => vec![e(lhs), e(rhs)],
            Expr::If { cond, then, els } => {
                let mut v = vec![e(cond), e(then)];
                if let Some(x) = els {
                    v.push(e(x));
                }
                v
            }
            Expr::Match { scrutinee, arms } => {
                let mut v = vec![e(scrutinee)];
                for (_, g, b) in arms {
                    if let Some(g) = g {
                        v.push(g);
                    }
                    v.push(b);
                }
                v
            }
            Expr::For { iter, key, body, .. } => {
                let mut v = vec![e(iter)];
                if let Some(k) = key {
                    v.push(e(k));
                }
                v.push(e(body));
                v
            }
            Expr::Block { stmts, tail } => {
                let mut v: Vec<&Expr> = stmts.iter().collect();
                if let Some(t) = tail {
                    v.push(e(t));
                }
                v
            }
            Expr::Let { value, .. } => vec![e(value)],
        }
    }

    /// Walk all sub-expressions including `self`.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Expr)) {
        f(self);
        for c in self.children() {
            c.walk(f);
        }
    }
}

/// Extract the `ExprModel` for one CST expression node.
///
/// `node` must be an expression-ish node (`PathExpr`, `LitExpr`, `FieldExpr`,
/// `CallExpr`, …). Unknown/unmodeled shapes degrade to [`Expr::Opaque`] so
/// analyses stay conservative instead of losing reads.
pub fn expr_from_cst(node: &SyntaxNode) -> Expr {
    // GSX elements keep their structure through the expression model
    // (§2 principle 1: structure is never erased early).
    if matches!(
        node.kind(),
        SyntaxKind::GsxElement | SyntaxKind::GsxSelfClosing | SyntaxKind::GsxFragment
    ) {
        let (node, _ds) = crate::ast::node_from_cst_pub(node);
        if let Some(node) = node {
            return Expr::Gsx(Box::new(node));
        }
    }
    match node.kind() {
        SyntaxKind::PathExpr => path_expr(node),
        SyntaxKind::LitExpr => lit_expr(node),
        SyntaxKind::FieldExpr => {
            let mut kids = expr_children(node);
            let field = node
                .children_with_tokens()
                .filter_map(|e| e.into_token()).find(|t| t.kind() == SyntaxKind::Ident);
            match (kids.pop(), field) {
                (Some(base), Some(t)) => Expr::Field(Box::new(base), t.text().to_string()),
                (Some(base), None) => base, // tuple index `.0` — keep the base
                (None, _) => Expr::Opaque(node.text().to_string()),
            }
        }
        SyntaxKind::MethodCallExpr => {
            // Node wraps [receiver…, Dot, Ident, LParen args RParen]; the
            // receiver is the first expression child, args the rest.
            let mut kids = expr_children(node);
            let name = node
                .children_with_tokens()
                .filter_map(|e| e.into_token()).find(|t| t.kind() == SyntaxKind::Ident)
                .map(|t| t.text().to_string())
                .unwrap_or_default();
            let args = if kids.len() >= 2 {
                kids.split_off(1)
            } else {
                vec![]
            };
            match kids.pop() {
                Some(recv) => Expr::Method(Box::new(recv), name, args),
                None => Expr::Opaque(node.text().to_string()),
            }
        }
        SyntaxKind::CallExpr => {
            // [callee…, LParen, args…, RParen]
            let mut kids = expr_children(node);
            if kids.len() >= 2 {
                let args = kids.split_off(1);
                match kids.pop() {
                    Some(callee) => match callee {
                        Expr::Var(name) => Expr::Call(name, args),
                        other => Expr::Method(Box::new(other), String::new(), args),
                    },
                    None => Expr::Opaque(node.text().to_string()),
                }
            } else {
                match kids.pop() {
                    Some(callee) => match callee {
                        Expr::Var(name) => Expr::Call(name, vec![]),
                        other => other,
                    },
                    None => Expr::Opaque(node.text().to_string()),
                }
            }
        }
        SyntaxKind::IndexExpr => {
            let mut kids = expr_children(node);
            if kids.len() == 2 {
                let idx = kids.pop().unwrap();
                let base = kids.pop().unwrap();
                Expr::Index(Box::new(base), Box::new(idx))
            } else {
                Expr::Opaque(node.text().to_string())
            }
        }
        SyntaxKind::UnaryExpr => {
            let op_tok = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| {
                    matches!(
                        t.kind(),
                        SyntaxKind::Minus | SyntaxKind::Bang | SyntaxKind::Star | SyntaxKind::And
                    )
                });
            let mut kids = expr_children(node);
            let op = match op_tok.map(|t| t.kind()) {
                Some(SyntaxKind::Minus) => UnOp::Neg,
                Some(SyntaxKind::Bang) => UnOp::Not,
                Some(SyntaxKind::Star) => UnOp::Deref,
                Some(SyntaxKind::And) => {
                    // `&mut x` — the KwMut token follows `&`.
                    if node
                        .children_with_tokens()
                        .filter_map(|e| e.into_token())
                        .any(|t| t.kind() == SyntaxKind::KwMut)
                    {
                        UnOp::RefMut
                    } else {
                        UnOp::Ref
                    }
                }
                _ => UnOp::Ref,
            };
            match kids.pop() {
                Some(e) => Expr::Unary(op, Box::new(e)),
                None => Expr::Opaque(node.text().to_string()),
            }
        }
        SyntaxKind::BinaryExpr => {
            let kids = expr_children(node);
            let op_tok = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| bin_op_kind(t.kind()).is_some())
                .and_then(|t| bin_op_kind(t.kind()));
            if let [lhs, rhs] = kids.as_slice() {
                Expr::Binary(
                    op_tok.unwrap_or(BinOp::Add),
                    Box::new(lhs.clone()),
                    Box::new(rhs.clone()),
                )
            } else {
                Expr::Opaque(node.text().to_string())
            }
        }
        SyntaxKind::AssignExpr => {
            let kids = expr_children(node);
            let op_tok = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| compound_op_kind(t.kind()).is_some())
                .and_then(|t| compound_op_kind(t.kind()));
            if let [lhs, rhs] = kids.as_slice() {
                Expr::Assign {
                    lhs: Box::new(lhs.clone()),
                    op: op_tok,
                    rhs: Box::new(rhs.clone()),
                }
            } else {
                Expr::Opaque(node.text().to_string())
            }
        }
        SyntaxKind::ClosureExpr => {
            let params: Vec<String> = node
                .children()
                .find(|n| n.kind() == SyntaxKind::ParamList)
                .map(|pl| {
                    pl.descendants_with_tokens()
                        .filter_map(|e| e.into_token())
                        .filter_map(|t| match t.kind() {
                            SyntaxKind::Ident => Some(t.text().to_string()),
                            SyntaxKind::Underscore => Some("_".to_string()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Body: first expression-ish child outside ParamList.
            let body = node
                .children()
                .find(|n| is_expr_node(n.kind()) && n.kind() != SyntaxKind::ParamList)
                .map(|n| expr_from_cst(&n))
                .unwrap_or(Expr::Opaque(String::new()));
            Expr::Closure {
                params,
                body: Box::new(body),
            }
        }
        SyntaxKind::IfExpr => {
            let mut kids = expr_children(node);
            // [cond, then, else?]
            if kids.len() >= 2 {
                let els = if kids.len() >= 3 {
                    Some(Box::new(kids.pop().unwrap()))
                } else {
                    None
                };
                let then = Box::new(kids.pop().unwrap());
                let cond = Box::new(kids.pop().unwrap());
                Expr::If { cond, then, els }
            } else {
                Expr::Opaque(node.text().to_string())
            }
        }
        SyntaxKind::MatchExpr => {
            let scrutinee = node
                .children()
                .find(|n| is_expr_node(n.kind()) && n.kind() != SyntaxKind::MatchArmList)
                .map(|n| expr_from_cst(&n))
                .unwrap_or_else(|| Expr::Opaque(String::new()));
            let arms = node
                .children()
                .find(|n| n.kind() == SyntaxKind::MatchArmList)
                .map(|al| {
                    al.children()
                        .filter(|a| a.kind() == SyntaxKind::MatchArm)
                        .map(|a| {
                            let pat = a
                                .children()
                                .find(|n| n.kind() != SyntaxKind::MatchArmList && !is_expr_node(n.kind()))
                                .map(|n| n.text().to_string())
                                .unwrap_or_default();
                            let guard = a
                                .children()
                                .find(|n| n.kind() == SyntaxKind::KwIf)
                                .and(None::<Expr>);
                            let body = a
                                .children()
                                .find(|n| is_expr_node(n.kind()))
                                .map(|n| expr_from_cst(&n))
                                .unwrap_or(Expr::Opaque(String::new()));
                            (pat, guard, body)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Expr::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            }
        }
        SyntaxKind::ForExpr => {
            // [pat…, KwIn, iterable, GsxForBy?, Block]
            let pat = node
                .children()
                .find(|n| n.kind() == SyntaxKind::PatIdent || n.kind() == SyntaxKind::PatWild)
                .map(|n| n.text().to_string())
                .unwrap_or_else(|| "_".to_string());
            let iter = node
                .children()
                .find(|n| is_expr_node(n.kind()) && n.kind() != SyntaxKind::GsxForBy)
                .map(|n| expr_from_cst(&n))
                .unwrap_or_else(|| Expr::Opaque(String::new()));
            let key = node
                .children()
                .find(|n| n.kind() == SyntaxKind::GsxForBy)
                .and_then(|b| b.children().find(|n| is_expr_node(n.kind())))
                .map(|n| Box::new(expr_from_cst(&n)));
            let body = node
                .children()
                .find(|n| n.kind() == SyntaxKind::Block)
                .map(|n| expr_from_cst(&n))
                .unwrap_or_else(|| Expr::Block { stmts: vec![], tail: None });
            Expr::For {
                pat,
                iter: Box::new(iter),
                key,
                body: Box::new(body),
            }
        }
        SyntaxKind::Block => {
            let mut stmts = vec![];
            let mut tail = None;
            for child in node.children() {
                match child.kind() {
                    SyntaxKind::LetStmt => {
                        let pat = child
                            .children()
                            .find(|n| n.kind() == SyntaxKind::PatIdent)
                            .map(|n| n.text().to_string())
                            .unwrap_or_default();
                        let value = child
                            .children()
                            .find(|n| is_expr_node(n.kind()))
                            .map(|n| expr_from_cst(&n))
                            .unwrap_or(Expr::Opaque(String::new()));
                        stmts.push(Expr::Let {
                            pat,
                            value: Box::new(value),
                        });
                    }
                    SyntaxKind::GsxElement | SyntaxKind::GsxSelfClosing | SyntaxKind::GsxFragment => {
                        let (node, _ds) = crate::ast::node_from_cst_pub(&child);
                        if let Some(node) = node {
                            if child.kind() == SyntaxKind::ExprStmt {
                                stmts.push(Expr::Gsx(Box::new(node)));
                            } else {
                                tail = Some(Box::new(Expr::Gsx(Box::new(node))));
                            }
                        }
                    }
                    k if is_expr_node(k) => {
                        let e = expr_from_cst(&child);
                        if child.kind() == SyntaxKind::ExprStmt && ends_with_semi(&child) {
                            stmts.push(e);
                        } else {
                            // Block-like construct or semicolon-less tail.
                            tail = Some(Box::new(e));
                        }
                    }
                    _ => {}
                }
            }
            Expr::Block { stmts, tail }
        }
        SyntaxKind::ExprStmt => {
            // Statements may wrap GSX elements (block-like stmt path).
            if let Some(inner) = node.children().find(|c| {
                matches!(
                    c.kind(),
                    SyntaxKind::GsxElement | SyntaxKind::GsxSelfClosing | SyntaxKind::GsxFragment
                )
            }) {
                let (node, _ds) = crate::ast::node_from_cst_pub(&inner);
                if let Some(node) = node {
                    return Expr::Gsx(Box::new(node));
                }
            }
            expr_children(node)
                .into_iter()
                .next()
                .unwrap_or_else(|| Expr::Opaque(node.text().to_string()))
        }
        SyntaxKind::TryExpr | SyntaxKind::AwaitExpr | SyntaxKind::ReturnExpr => Expr::Flow,
        SyntaxKind::ArrayExpr => {
            let kids = expr_children(node);
            // Model arrays as a method-shaped call so reads stay visible.
            Expr::Call("[]".to_string(), kids)
        }
        SyntaxKind::TupleExpr => {
            let kids = expr_children(node);
            Expr::Call("()".to_string(), kids)
        }
        SyntaxKind::RangeExpr => {
            let kids = expr_children(node);
            Expr::Call("..".to_string(), kids)
        }
        SyntaxKind::CastExpr => expr_children(node)
            .into_iter()
            .next()
            .unwrap_or_else(|| Expr::Opaque(node.text().to_string())),
        SyntaxKind::RefExpr => {
            let mut kids = expr_children(node);
            let has_mut = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .any(|t| t.kind() == SyntaxKind::KwMut);
            match kids.pop() {
                Some(e) => Expr::Unary(
                    if has_mut { UnOp::RefMut } else { UnOp::Ref },
                    Box::new(e),
                ),
                None => Expr::Opaque(node.text().to_string()),
            }
        }
        _ => Expr::Opaque(node.text().to_string()),
    }
}

/// True when an ExprStmt carries a trailing `;` (statement) versus being the
/// block's tail expression (§: the parser only bumps `;` inside the stmt).
fn ends_with_semi(stmt: &SyntaxNode) -> bool {
    stmt.children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| !t.kind().is_trivia())
        .last()
        .is_some_and(|t| t.kind() == SyntaxKind::Semicolon)
}

fn path_expr(node: &SyntaxNode) -> Expr {
    // PathExpr holds a UsePath of Ident/ColonColon tokens. Keywords that can
    // start paths (self/super/crate) are also path segments.
    let name: String = node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| {
            matches!(
                t.kind(),
                SyntaxKind::Ident | SyntaxKind::KwSelf | SyntaxKind::KwSuper | SyntaxKind::KwCrate
            )
        })
        .map(|t| t.text().to_string())
        .collect::<Vec<_>>()
        .join("::");
    if name.is_empty() {
        Expr::Opaque(node.text().to_string())
    } else {
        Expr::Var(name)
    }
}

fn lit_expr(node: &SyntaxNode) -> Expr {
    let tok = node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia());
    match tok {
        Some(t) => literal_from_token(&t),
        None => Expr::Lit(Lit::Unit),
    }
}

fn literal_from_token(t: &SyntaxToken) -> Expr {
    match t.kind() {
        SyntaxKind::IntLit => {
            let text = t.text().trim_end_matches(|c: char| c.is_ascii_alphabetic());
            let cleaned = text.replace('_', "");
            if let Ok(v) = cleaned.parse::<i128>() {
                Expr::Lit(Lit::Int(v))
            } else if let Ok(v) = cleaned.parse::<u128>() {
                Expr::Lit(Lit::Uint(v))
            } else {
                Expr::Opaque(t.text().to_string())
            }
        }
        SyntaxKind::FloatLit => {
            let cleaned = t.text().replace('_', "");
            match cleaned.parse::<f64>() {
                Ok(v) => Expr::Lit(Lit::Float(v)),
                Err(_) => Expr::Opaque(t.text().to_string()),
            }
        }
        SyntaxKind::StringLit => {
            let raw = t.text();
            let inner = raw
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .unwrap_or(raw);
            Expr::Lit(Lit::Str(unescape(inner)))
        }
        SyntaxKind::CharLit => {
            let raw = t.text();
            let inner = raw
                .strip_prefix('\'')
                .and_then(|x| x.strip_suffix('\''))
                .unwrap_or(raw);
            let unescaped = unescape(inner);
            let mut it = unescaped.chars();
            Expr::Lit(Lit::Char(it.next().unwrap_or('\0')))
        }
        SyntaxKind::KwTrue => Expr::Lit(Lit::Bool(true)),
        SyntaxKind::KwFalse => Expr::Lit(Lit::Bool(false)),
        SyntaxKind::ByteLit => Expr::Lit(Lit::Int(0)),
        _ => Expr::Opaque(t.text().to_string()),
    }
}

/// Minimal unescape for string/char literal bodies.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('0') => out.push('\0'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn bin_op_kind(k: SyntaxKind) -> Option<BinOp> {
    Some(match k {
        SyntaxKind::LtEq => BinOp::Le,
        SyntaxKind::GtEq => BinOp::Ge,
        SyntaxKind::EqEq => BinOp::Eq,
        SyntaxKind::BangEq => BinOp::Ne,
        SyntaxKind::Lt => BinOp::Lt,
        SyntaxKind::Gt => BinOp::Gt,
        SyntaxKind::Plus => BinOp::Add,
        SyntaxKind::Minus => BinOp::Sub,
        SyntaxKind::Star => BinOp::Mul,
        SyntaxKind::Slash => BinOp::Div,
        SyntaxKind::Percent => BinOp::Rem,
        SyntaxKind::AndAnd => BinOp::And,
        SyntaxKind::OrOr => BinOp::Or,
        _ => return None,
    })
}

fn compound_op_kind(k: SyntaxKind) -> Option<BinOp> {
    Some(match k {
        SyntaxKind::PlusEq => BinOp::Add,
        SyntaxKind::MinusEq => BinOp::Sub,
        SyntaxKind::StarEq => BinOp::Mul,
        SyntaxKind::SlashEq => BinOp::Div,
        SyntaxKind::PercentEq => BinOp::Rem,
        _ => return None,
    })
}

/// Expression-ish node kinds of the CST (mirrors `is_exprish`).
pub fn is_expr_node(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::PathExpr
            | SyntaxKind::LitExpr
            | SyntaxKind::FieldExpr
            | SyntaxKind::MethodCallExpr
            | SyntaxKind::CallExpr
            | SyntaxKind::IndexExpr
            | SyntaxKind::UnaryExpr
            | SyntaxKind::BinaryExpr
            | SyntaxKind::CastExpr
            | SyntaxKind::RefExpr
            | SyntaxKind::ClosureExpr
            | SyntaxKind::IfExpr
            | SyntaxKind::WhileExpr
            | SyntaxKind::LoopExpr
            | SyntaxKind::ForExpr
            | SyntaxKind::MatchExpr
            | SyntaxKind::Block
            | SyntaxKind::ArrayExpr
            | SyntaxKind::TupleExpr
            | SyntaxKind::StructExpr
            | SyntaxKind::RangeExpr
            | SyntaxKind::AssignExpr
            | SyntaxKind::TryExpr
            | SyntaxKind::AwaitExpr
            | SyntaxKind::ReturnExpr
            | SyntaxKind::BreakExpr
            | SyntaxKind::ContinueExpr
            | SyntaxKind::ExprStmt
            | SyntaxKind::LetStmt
            | SyntaxKind::GsxElement
            | SyntaxKind::GsxSelfClosing
            | SyntaxKind::GsxFragment
    )
}

/// Direct expression children of a node (skips tokens and non-expr nodes).
fn expr_children(node: &SyntaxNode) -> Vec<Expr> {
    node.children()
        .filter(|c| is_expr_node(c.kind()))
        .map(|c| expr_from_cst(&c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_syntax::SyntaxKind;

    /// Parse a source expression and extract its model.
    fn model(src: &str) -> Expr {
        let wrapped = format!("fn __g() {{ let _x = {}; }}", src);
        let parsed = glyim_frontend::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
        let block = parsed
            .root
            .children()
            .find(|n| n.kind() == SyntaxKind::FnDef)
            .and_then(|f| f.children().find(|n| n.kind() == SyntaxKind::Block))
            .expect("block");
        let let_stmt = block.children().find(|n| n.kind() == SyntaxKind::LetStmt).expect("let");
        let value = let_stmt
            .children()
            .find(|n| is_expr_node(n.kind()))
            .expect("initializer");
        expr_from_cst(&value)
    }

    /// §7.4: field paths rooted at the state param are visible
    /// (`s.count`, `s.user.name`).
    #[test]
    fn state_place_reads() {
        assert_eq!(
            model("s.count"),
            Expr::field("s", "count")
        );
        assert_eq!(
            model("s.user.name"),
            Expr::Field(
                Box::new(Expr::field("s", "user")),
                "name".to_string()
            )
        );
    }

    /// §7.6: indexed places `s.items[i]` keep both the container and the
    /// index visible.
    #[test]
    fn indexed_place_read() {
        let e = model("s.items[i]");
        assert!(matches!(
            e,
            Expr::Index(ref b, ref ix)
                if **b == Expr::field("s", "items")
                    && **ix == Expr::Var("i".to_string())
        ));
    }

    /// Method calls stay structured: `s.history.iter()`.
    #[test]
    fn method_call_shape() {
        let e = model("s.history.iter()");
        match e {
            Expr::Method(recv, name, args) => {
                assert_eq!(name, "iter");
                assert!(args.is_empty());
                assert_eq!(*recv, Expr::field("s", "history"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Handlers (§7.3): `|s, _| s.count += s.step` — closure with a compound
    /// assignment whose write-set is one place.
    #[test]
    fn handler_closure_with_write() {
        let e = model("|s, _| s.count += s.step");
        match e {
            Expr::Closure { params, body } => {
                assert_eq!(params, vec!["s".to_string(), "_".to_string()]);
                assert_eq!(
                    *body,
                    Expr::Assign {
                        lhs: Box::new(Expr::field("s", "count")),
                        op: Some(BinOp::Add),
                        rhs: Box::new(Expr::field("s", "step")),
                    }
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Text-hole splitting is downstream of literals: `"Count: {s.count}"`
    /// keeps its static text; the hole becomes its own expression.
    #[test]
    fn literals_and_operators() {
        assert_eq!(model("s.count < 0"), Expr::Binary(
            BinOp::Lt,
            Box::new(Expr::field("s", "count")),
            Box::new(Expr::Lit(Lit::Int(0))),
        ));
        assert_eq!(model("\"hi\""), Expr::Lit(Lit::Str("hi".to_string())));
        assert_eq!(model("true"), Expr::Lit(Lit::Bool(true)));
        assert_eq!(model("12.5"), Expr::Lit(Lit::Float(12.5)));
    }

    /// Blocks with lets and tails survive as structured regions.
    #[test]
    fn block_with_let() {
        let e = model("{ let a = s.count; a + 1 }");
        match e {
            Expr::Block { stmts, tail } => {
                assert!(matches!(&stmts[0], Expr::Let { pat, .. } if pat == "a"));
                assert!(matches!(*tail.expect("tail"), Expr::Binary(..)));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// `for` with a GSX `by` key keeps both the iterable and the key. `by`
    /// is a GSX-scoped contextual keyword, so the source must be a GSX block
    /// child (§3.2).
    #[test]
    fn for_with_by_key() {
        let wrapped = "fn __g() { <p>{for item in s.history.iter() by item.id { 1 }}</p> }";
        let parsed = glyim_frontend::parse_to_syntax(wrapped, glyim_span::FileId::from_raw(0));
        let block_child = parsed
            .root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::GsxBlockChild)
            .expect("gsx block child");
        let for_expr = block_child
            .children()
            .find(|n| n.kind() == SyntaxKind::ForExpr)
            .expect("for expr");
        let e = expr_from_cst(&for_expr);
        match e {
            Expr::For { pat, iter, key, .. } => {
                assert_eq!(pat, "item");
                assert!(matches!(*iter, Expr::Method(..)));
                assert!(key.is_some());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Unmodeled constructs degrade to Opaque (conservative), never drop.
    #[test]
    fn opaque_fallback_is_conservative() {
        let e = model("do_thing(x)");
        assert!(matches!(e, Expr::Call(ref n, _) if n == "do_thing"));
    }
}
