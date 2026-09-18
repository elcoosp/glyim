//! Typed High-Level IR — fully typed, still generic.

use glyim_core::def_id::{AdtId, ConstDefId, DefId, FnDefId, TraitDefId, VariantIdx};
use glyim_core::interner::Name;
use glyim_core::primitives::*;
use glyim_span::Span;
use glyim_type::*;

glyim_core::define_idx!(LocalVarId);

#[derive(Clone, Debug)]
/// Body.
pub struct Body {
/// Struct.
    pub owner: DefId,
/// Struct.
    pub params: Vec<Param>,
/// Struct.
    pub return_ty: Ty,
/// Struct.
    pub stmts: Vec<Stmt>,
/// Struct.
    pub span: Span,
}

#[derive(Clone, Debug)]
/// Param.
pub struct Param {
/// Struct.
    pub name: Name,
/// Struct.
    pub ty: Ty,
/// Struct.
    pub span: Span,
/// Struct.
    pub pat: Pattern,
    /// The `LocalVarId` this parameter is bound to in the enclosing type-check
    /// scope. Mirrored into the closure body so the lowering can map it to the
    /// parameter's MIR local.
    pub local: LocalVarId,
}

#[derive(Clone, Debug)]
/// Stmt.
pub enum Stmt {
/// Variant.
    Let {
/// Struct.
        name: Name,
/// Struct.
        ty: Ty,
/// Struct.
        pat: Pattern,
/// Struct.
        init: Option<Expr>,
/// Struct.
        span: Span,
    },
/// Variant.
    Assign {
/// Struct.
        lhs: Expr,
/// Struct.
        rhs: Expr,
/// Struct.
        span: Span,
    },
/// Variant.
    Return {
/// Struct.
        value: Option<Expr>,
/// Struct.
        span: Span,
    },
/// Variant.
    Expr {
/// Struct.
        expr: Expr,
    },
}

#[derive(Clone, Debug)]
/// Expr.
pub struct Expr {
/// Struct.
    pub kind: ExprKind,
/// Struct.
    pub ty: Ty,
/// Struct.
    pub span: Span,
}

impl Expr {
    #[inline]
/// err.
    pub fn err(span: Span) -> Self {
        Self {
            kind: ExprKind::Err,
            ty: Ty::ERROR,
            span,
        }
    }
}

#[derive(Clone, Debug)]
/// ForIteratorNext.
pub struct ForIteratorNext {
/// Struct.
    pub fn_def_id: FnDefId,
/// Struct.
    pub fn_substs: Substitution,
/// Struct.
    pub option_ty: Ty,
/// Struct.
    pub discr_ty: Ty,
/// Struct.
    pub ref_iter_ty: Ty,
/// Struct.
    pub fn_ty: Ty,
}

#[derive(Clone, Debug)]
/// ExprKind.
pub enum ExprKind {
#[allow(missing_docs)]
    Literal(Literal),
#[allow(missing_docs)]
    VarRef(LocalVarId),
#[allow(missing_docs)]
    FnRef(FnDefId),
    /// Reference to a constant defined in the value namespace
    /// (`const X = ...;` / `mod::X`). The expression's type is the
    /// constant's value type (e.g. `i32`), obtained from the def map.
    ConstRef(ConstDefId),
    /// Reference to an enum variant in the value namespace
    /// (`Color::Red` / `Red`). The expression's type is the enclosing enum's
    /// type (`TyKind::Adt(adt_id, substs)`); `variant_idx` selects the
    /// constructor. Unit variants lower to an `Aggregate` of the enum.
    VariantRef(AdtId, VariantIdx),
    /// Reference to an enum variant *constructor* in the value namespace
    /// (`Some`, `Color::Green`) used as a call target: `Some(x)` /
    /// `Color::Green(x)`. The expression's type is a function type
    /// `fn(field_tys) -> Enum` (registered as a `FnDefId` fn-sig). MIR
    /// lowers the surrounding `Call` to an `Aggregate` of the enum ADT.
/// Struct.
    VariantCtor {
        /// adt_id field.
        adt_id: AdtId,
        /// variant_idx field.
        variant_idx: VariantIdx,
    },
    /// Reference to a trait method written as a path-qualified call
    /// `Trait::method(receiver, ..)`. The receiver type selects the
    /// concrete impl at type-check time (static dispatch); downstream this
    /// lowers to a normal `Call` of the resolved impl function. (Full
    /// dynamic dispatch via trait objects uses `DynamicCall`.)
    TraitMethodRef {
/// Struct.
        trait_def_id: TraitDefId,
/// Struct.
        method_name: Name,
    },
/// Variant.
    Binary {
/// Struct.
        op: BinOp,
/// Struct.
        lhs: Box<Expr>,
/// Struct.
        rhs: Box<Expr>,
    },
/// Variant.
    Unary {
/// Struct.
        op: UnOp,
/// Struct.
        operand: Box<Expr>,
    },
/// Variant.
    Call {
/// Struct.
        func: Box<Expr>,
/// Struct.
        args: Vec<Expr>,
    },
/// Variant.
    DynamicCall {
/// Struct.
        receiver: Box<Expr>,
/// Struct.
        trait_def_id: TraitDefId,
/// Struct.
        method_name: Name,
/// Struct.
        args: Vec<Expr>,
    },
/// Variant.
    If {
/// Struct.
        cond: Box<Expr>,
/// Struct.
        then_branch: Box<Expr>,
/// Struct.
        else_branch: Option<Box<Expr>>,
    },
/// Variant.
    Match {
/// Struct.
        scrutinee: Box<Expr>,
/// Struct.
        arms: Vec<MatchArm>,
    },
/// Variant.
    Block {
/// Struct.
        stmts: Vec<Stmt>,
/// Struct.
        tail: Option<Box<Expr>>,
    },
/// Variant.
    Ref {
/// Struct.
        mutability: Mutability,
/// Struct.
        operand: Box<Expr>,
    },
/// Variant.
    Field {
/// Struct.
        receiver: Box<Expr>,
/// Struct.
        field: Name,
/// Struct.
        ty: Ty,
    },
/// Variant.
    Index {
/// Struct.
        base: Box<Expr>,
/// Struct.
        index: Box<Expr>,
    },
/// Variant.
    Cast {
/// Struct.
        expr: Box<Expr>,
    },
/// Variant.
    While {
/// Struct.
        cond: Box<Expr>,
/// Struct.
        body: Box<Expr>,
    },
/// Variant.
    Loop {
/// Struct.
        body: Box<Expr>,
    },
/// Variant.
    For {
/// Struct.
        pat: Box<Pattern>,
/// Struct.
        iterable: Box<Expr>,
/// Struct.
        body: Box<Expr>,
/// Phase 1 (GLYIM_DESTUB_PLAN): the `Iterator::next` method resolved for this
/// loop's iterable type, threaded from typeck so the lowering pass can take
/// the real multi-iteration path without re-solving the trait. `None` means
/// typeck could not resolve an `Iterator` impl (or the test harness left it
/// unset); lowering then falls back to `LowerCtx::iterator_next_fn`.
        next: Option<ForIteratorNext>,
    },
#[allow(missing_docs)]
    Array(Vec<Expr>),
#[allow(missing_docs)]
    Tuple(Vec<Expr>),
/// Variant.
    Struct {
/// Struct.
        adt_id: AdtId,
/// Struct.
        variant_idx: u32,
#[doc = "field"]
        fields: Vec<(Name, Expr)>,
/// Struct.
        spread: Option<Box<Expr>>,
    },
/// Variant.
    Break {
/// Struct.
        value: Option<Box<Expr>>,
    },
    /// Early return (`return expr`). Distinguished from `Break` (loop break)
    /// so the MIR lowering can target the function return place (`_0`)
    /// instead of treating it as a loop break (plan unstub-5 P5: `return`
    /// inside `loop`/`match` bodies).
    Return {
/// Struct.
        value: Option<Box<Expr>>,
    },
/// Variant.
    Continue,
/// Variant.
    Closure {
/// Struct.
        body: Box<Body>,
/// Struct.
        captures: Vec<Capture>,
/// Struct.
        is_move: bool,
    },
/// Variant.
    Range {
/// Struct.
        start: Option<Box<Expr>>,
/// Struct.
        end: Option<Box<Expr>>,
/// Struct.
        inclusive: bool,
    },
    /// `expr?` — the try operator. Carries the operand expression; typeck has
    /// already extracted the success type (and constrained the enclosing
    /// function's error type). MIR lowering turns this into an early-return on
    /// the error/None branch (`docs/issues/INDEX.md` §0007-adjacent).
    Try {
/// Struct.
        expr: Box<Expr>,
    },
/// Variant.
    Err,
}

#[derive(Clone, Debug)]
/// MatchArm.
pub struct MatchArm {
/// Struct.
    pub pat: Pattern,
/// Struct.
    pub guard: Option<Box<Expr>>,
/// Struct.
    pub body: Expr,
}

#[derive(Clone, Debug)]
/// Pattern.
pub struct Pattern {
/// Struct.
    pub kind: PatternKind,
/// Struct.
    pub ty: Ty,
/// Struct.
    pub span: Span,
}

impl Pattern {
    #[inline]
/// wild.
    pub fn wild(ty: Ty, span: Span) -> Self {
        Self {
            kind: PatternKind::Wild,
            ty,
            span,
        }
    }

    #[inline]
    /// binding.
    pub fn binding(
        var_id: LocalVarId,
        name: Name,
        mutability: Mutability,
        ty: Ty,
        span: Span,
    ) -> Self {
        Self {
            kind: PatternKind::Binding {
                var_id,
                name,
                mutability,
                subpattern: None,
            },
            ty,
            span,
        }
    }

    #[inline]
/// err.
    pub fn err(span: Span) -> Self {
        Self {
            kind: PatternKind::Error,
            ty: Ty::ERROR,
            span,
        }
    }
}

// Manual implementation of Clone and Debug to avoid derive issues
#[derive(Clone, Debug)]
/// FieldPat.
pub struct FieldPat {
/// Struct.
    pub field: Name,
/// Struct.
    pub pattern: Pattern,
/// Struct.
    pub span: Span,
}

#[derive(Clone, Debug)]
/// Capture.
pub struct Capture {
/// Struct.
    pub local: LocalVarId,
/// Struct.
    pub kind: CaptureKind,
/// Struct.
    pub ty: Ty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// CaptureKind.
pub enum CaptureKind {
/// Variant.
    ByValue,
#[allow(missing_docs)]
    ByRef(Mutability),
}

#[derive(Clone, Debug)]
/// Literal.
pub enum Literal {
#[allow(missing_docs)]
    Int(i128, Option<IntTy>),
#[allow(missing_docs)]
    Uint(u128, Option<UintTy>),
#[allow(missing_docs)]
    FloatBits(u64, FloatTy),
#[allow(missing_docs)]
    Bool(bool),
#[allow(missing_docs)]
    Char(char),
#[allow(missing_docs)]
    String(Name),
/// Variant.
    Unit,
}

/// PatternKind.
pub enum PatternKind {
/// Variant.
    Wild,
/// Variant.
Binding {
    /// The `LocalVarId` assigned by the type-checker. Threaded through so
    /// the MIR lowering can map `VarRef(local_var_id)` back to the
    /// `LocalIdx` it allocated for this binding (the lowering allocates
    /// locals in a different order than the type-checker's `LocalVarId`
    /// space, so a direct `LocalIdx::from_raw(var_id.to_raw())` is wrong).
    var_id: LocalVarId,
    /// Struct.
    name: Name,
    /// Struct.
    mutability: Mutability,
    /// Struct.
    subpattern: Option<Box<Pattern>>,
},
/// Variant.
    Struct {
/// Struct.
        adt_id: AdtId,
/// Struct.
        variant_idx: u32,
/// Struct.
        fields: Vec<FieldPat>,
/// Struct.
        rest: bool,
    },
#[allow(missing_docs)]
    Tuple(Vec<Pattern>),
#[allow(missing_docs)]
    Or(Vec<Pattern>),
#[allow(missing_docs)]
    Literal(Literal),
/// Variant.
    Range {
/// Struct.
        start: Option<Literal>,
/// Struct.
        end: Option<Literal>,
/// Struct.
        inclusive: bool,
    },
#[allow(missing_docs)]
    ConstBlock(Box<Body>),
/// Variant.
    Slice {
/// Struct.
        prefix: Vec<Pattern>,
/// Struct.
        slice: Option<Box<Pattern>>,
/// Struct.
        suffix: Vec<Pattern>,
    },
/// Variant.
    Error,
}

// Manual Debug for PatternKind to avoid derive non-exhaustive error
impl std::fmt::Debug for PatternKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternKind::Wild => write!(f, "Wild"),
            PatternKind::Binding {
                var_id,
                name,
                mutability,
                subpattern,
            } => f
                .debug_struct("Binding")
                .field("var_id", var_id)
                .field("name", name)
                .field("mutability", mutability)
                .field("subpattern", subpattern)
                .finish(),
            PatternKind::Struct {
                adt_id,
                variant_idx,
                fields,
                rest,
            } => f
                .debug_struct("Struct")
                .field("adt_id", adt_id)
                .field("variant_idx", variant_idx)
                .field("fields", fields)
                .field("rest", rest)
                .finish(),
            PatternKind::Tuple(pats) => f.debug_tuple("Tuple").field(pats).finish(),
            PatternKind::Or(pats) => f.debug_tuple("Or").field(pats).finish(),
            PatternKind::Literal(lit) => f.debug_tuple("Literal").field(lit).finish(),
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => f
                .debug_struct("Range")
                .field("start", start)
                .field("end", end)
                .field("inclusive", inclusive)
                .finish(),
            PatternKind::ConstBlock(body) => f.debug_tuple("ConstBlock").field(body).finish(),
            PatternKind::Slice {
                prefix,
                slice,
                suffix,
            } => f
                .debug_struct("Slice")
                .field("prefix", prefix)
                .field("slice", slice)
                .field("suffix", suffix)
                .finish(),
            PatternKind::Error => write!(f, "Error"),
        }
    }
}

// Manual Clone for PatternKind
impl Clone for PatternKind {
    fn clone(&self) -> Self {
        match self {
            PatternKind::Wild => PatternKind::Wild,
            PatternKind::Binding {
                var_id,
                name,
                mutability,
                subpattern,
            } => PatternKind::Binding {
                var_id: *var_id,
                name: *name,
                mutability: *mutability,
                subpattern: subpattern.clone(),
            },
            PatternKind::Struct {
                adt_id,
                variant_idx,
                fields,
                rest,
            } => PatternKind::Struct {
                adt_id: *adt_id,
                variant_idx: *variant_idx,
                fields: fields.clone(),
                rest: *rest,
            },
            PatternKind::Tuple(pats) => PatternKind::Tuple(pats.clone()),
            PatternKind::Or(pats) => PatternKind::Or(pats.clone()),
            PatternKind::Literal(lit) => PatternKind::Literal(lit.clone()),
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => PatternKind::Range {
                start: start.clone(),
                end: end.clone(),
                inclusive: *inclusive,
            },
            PatternKind::ConstBlock(body) => PatternKind::ConstBlock(body.clone()),
            PatternKind::Slice {
                prefix,
                slice,
                suffix,
            } => PatternKind::Slice {
                prefix: prefix.clone(),
                slice: slice.clone(),
                suffix: suffix.clone(),
            },
            PatternKind::Error => PatternKind::Error,
        }
    }
}
