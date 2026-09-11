//! High-Level IR — name-resolved, untyped.
//!
//! This crate depends only on `glyim-core` and `glyim-span`,
//! NOT on `glyim-type`. This ensures the frontend does not pull
//! in the heavy type system machinery.
//!
//! [F12] Uses `glyim_core::path::PathKind` (shared). Defines its
//! own `PathSegment` with `generic_args: Option<Vec<TypeRef>>`,
//! which differs from `glyim_core::path::PathSegment` (name-only).
// Stylistic clippy lints suppressed crate-wide (test-noise lints).
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::vec_init_then_push,
    clippy::assertions_on_constants,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_c_str_literals,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    clippy::manual_strip,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::clone_on_copy,
    clippy::mutable_key_type,
    clippy::only_used_in_recursion,
    clippy::let_unit_value,
    clippy::unnecessary_literal_unwrap,
    clippy::format_in_format_args,
    clippy::permissions_set_readonly_false,
    clippy::needless_lifetimes,
    clippy::collapsible_if
)]
// Plan §3.1 (ratchet, not a hard ban): HIR lowering consumes untrusted user
// input, so `unwrap`/`expect`/`panic!` on source-derived values must become
// `Diagnostic`/Result. `#[warn]` (not `#[deny]`) surfaces the count under
// `cargo clippy` so the sweep is tracked and ratcheted per-PR without
// breaking the build.
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::{Interner, Name};
use glyim_core::path::PathKind;
use glyim_core::primitives::*;
use glyim_span::Span;

glyim_core::define_idx!(ExprId);
glyim_core::define_idx!(PatId);
glyim_core::define_idx!(BodyId);
glyim_core::define_idx!(ItemId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// HirId.
pub struct HirId {
/// Struct.
    pub owner: LocalDefId,
/// Struct.
    pub local: u32,
}

#[derive(Clone, Debug)]
/// CrateHir.
pub struct CrateHir {
/// Struct.
    pub items: IndexVec<ItemId, Item>,
/// Struct.
    pub bodies: IndexVec<BodyId, Body>,
/// Struct.
    pub body_owners: IndexVec<BodyId, LocalDefId>,
    /// The `Interner` used while lowering this crate. Names embedded in the
    /// HIR (`Body::pats`, `PathSegment`s, …) are valid `Name` ids in THIS
    /// interner. The type-checker is typically built from the same shared
    /// database interner, but in some call paths (e.g. a freshly lowered HIR
    /// paired with a separately-constructed `TyCtx`) the two can diverge, so
    /// pattern/reference names must be re-mapped through this interner before
    /// being added to the lexical environment.
    pub interner: Interner,
}

#[derive(Clone, Debug)]
/// Item.
pub struct Item {
/// Struct.
    pub id: ItemId,
/// Struct.
    pub name: Name,
/// Struct.
    pub kind: ItemKind,
/// Struct.
    pub visibility: Visibility,
/// Struct.
    pub span: Span,
}

#[derive(Clone, Debug)]
/// ItemKind.
pub enum ItemKind {
#[allow(missing_docs)]
    Fn(FnItem),
#[allow(missing_docs)]
    Struct(StructItem),
#[allow(missing_docs)]
    Enum(EnumItem),
#[allow(missing_docs)]
    Trait(TraitItem),
#[allow(missing_docs)]
    Impl(ImplItem),
#[allow(missing_docs)]
    TypeAlias(TypeAliasItem),
#[allow(missing_docs)]
    Const(ConstItem),
#[allow(missing_docs)]
    Static(StaticItem),
#[allow(missing_docs)]
    Mod(ModItem),
#[allow(missing_docs)]
    Use(UseItem),
#[allow(missing_docs)]
    Extern(ExternBlockItem),
}

#[derive(Clone, Debug)]
/// FnItem.
pub struct FnItem {
/// Struct.
    pub params: Vec<Param>,
/// Struct.
    pub return_ty: Option<TypeRef>,
/// Struct.
    pub body: Option<BodyId>,
/// Struct.
    pub is_unsafe: bool,
/// Struct.
    pub is_async: bool,
/// Struct.
    pub is_const: bool,
/// Struct.
    pub generic_params: Vec<GenericParam>,
/// Struct.
    pub where_clauses: Vec<crate::where_clause::WhereClause>,
    /// Calling convention for FFI (`extern "C" fn`), if declared. `None`
    /// means the default Glyim ABI (unstub-5 Phase 4).
    pub abi: Option<Name>,
}

#[derive(Clone, Debug)]
/// StructItem.
pub struct StructItem {
/// Struct.
    pub fields: Vec<Field>,
/// Struct.
    pub kind: StructKind,
/// Struct.
    pub generic_params: Vec<GenericParam>,
/// Struct.
    pub where_clauses: Vec<crate::where_clause::WhereClause>,
}

#[derive(Clone, Debug)]
/// EnumItem.
pub struct EnumItem {
/// Struct.
    pub variants: Vec<Variant>,
/// Struct.
    pub generic_params: Vec<GenericParam>,
/// Struct.
    pub where_clauses: Vec<crate::where_clause::WhereClause>,
}

#[derive(Clone, Debug)]
/// Variant.
pub struct Variant {
/// Struct.
    pub name: Name,
/// Struct.
    pub fields: Vec<Field>,
/// Struct.
    pub kind: StructKind,
/// Struct.
    pub span: Span,
}

#[derive(Clone, Debug)]
/// TraitMethod.
pub struct TraitMethod {
/// Struct.
    pub name: Name,
/// Struct.
    pub params: Vec<Param>,
/// Struct.
    pub return_ty: Option<TypeRef>,
/// Struct.
    pub default_body: Option<BodyId>,
}

#[derive(Clone, Debug)]
/// ImplMethod.
pub struct ImplMethod {
/// Struct.
    pub name: Name,
/// Struct.
    pub body: Option<BodyId>,
/// Struct.
    pub params: Vec<Param>,
/// Struct.
    pub generic_params: Vec<GenericParam>,
/// Struct.
    pub return_ty: Option<TypeRef>,
}

#[derive(Clone, Debug)]
/// AssociatedTy.
pub struct AssociatedTy {
/// Struct.
    pub name: Name,
/// Struct.
    pub bounds: Vec<TypeRef>,
/// Struct.
    pub default: Option<TypeRef>,
}

#[derive(Clone, Debug)]
/// TraitItem.
pub struct TraitItem {
/// Struct.
    pub associated_types: Vec<AssociatedTy>,
/// Struct.
    pub methods: Vec<TraitMethod>,
/// Struct.
    pub generic_params: Vec<GenericParam>,
/// Struct.
    pub where_clauses: Vec<crate::where_clause::WhereClause>,
}

#[derive(Clone, Debug)]
/// ImplItem.
pub struct ImplItem {
/// Struct.
    pub trait_ref: Option<Path>,
/// Struct.
    pub self_ty: TypeRef,
/// Struct.
    pub methods: Vec<ImplMethod>,
/// Struct.
    pub generic_params: Vec<GenericParam>,
/// Struct.
    pub where_clauses: Vec<crate::where_clause::WhereClause>,
    /// Associated type definitions, e.g. `type Output = i32;` inside
    /// `impl MyFuture for AddOne`. Captured so projection (`Self::Output`,
    /// `F::Output`) can be resolved to the defining type. Previously dropped,
    /// which is why associated-type projection had nothing to resolve against.
    pub associated_types: Vec<AssociatedTy>,
}

#[derive(Clone, Debug)]
/// TypeAliasItem.
pub struct TypeAliasItem {
/// Struct.
    pub ty: Option<TypeRef>,
/// Struct.
    pub generic_params: Vec<GenericParam>,
/// Struct.
    pub where_clauses: Vec<crate::where_clause::WhereClause>,
}

#[derive(Clone, Debug)]
/// ConstItem.
pub struct ConstItem {
/// Struct.
    pub ty: TypeRef,
/// Struct.
    pub body: Option<BodyId>,
    /// Root expression of the constant's initializer body, used by const
    /// evaluation (Part C: const value materialization) to evaluate the value.
    pub root_expr: Option<ExprId>,
}

#[derive(Clone, Debug)]
/// StaticItem.
pub struct StaticItem {
/// Struct.
    pub ty: TypeRef,
/// Struct.
    pub body: Option<BodyId>,
/// Struct.
    pub is_mut: bool,
}

#[derive(Clone, Debug)]
/// ModItem.
pub struct ModItem {
/// Struct.
    pub children: Vec<ItemId>,
}

#[derive(Clone, Debug)]
/// UseItem.
pub struct UseItem {
/// Struct.
    pub path: Path,
/// Struct.
    pub alias: Option<Name>,
}

#[derive(Clone, Debug)]
/// ExternBlockItem.
pub struct ExternBlockItem {
/// Struct.
    pub items: Vec<ItemId>,
/// Struct.
    pub abi: Option<Name>,
}

#[derive(Clone, Debug)]
/// Param.
pub struct Param {
/// Struct.
    pub name: Name,
/// Struct.
    pub ty: Option<TypeRef>,
/// Struct.
    pub span: Span,
}

#[derive(Clone, Debug)]
/// Field.
pub struct Field {
/// Struct.
    pub name: Name,
/// Struct.
    pub ty: TypeRef,
/// Struct.
    pub span: Span,
}

#[derive(Clone, Debug)]
/// GenericParam.
pub struct GenericParam {
/// Struct.
    pub name: Name,
/// Struct.
    pub kind: GenericParamKind,
/// Struct.
    pub span: Span,
}

#[derive(Clone, Debug)]
/// GenericParamKind.
pub enum GenericParamKind {
/// Variant.
    Type {
/// Struct.
        default: Option<TypeRef>,
        /// Trait bounds declared on the param, e.g. the `MyFuture` in
        /// `fn block_on<F: MyFuture>`. Captured so typeck can solve the bound
        /// and project associated types (`F::Output`); previously dropped.
        bounds: Vec<TypeRef>,
    },
/// Variant.
    Lifetime,
/// Variant.
    Const {
/// Struct.
        ty: TypeRef,
/// Struct.
        default: Option<ConstRef>,
    },
}

#[derive(Clone, Debug)]
/// TypeRef.
pub enum TypeRef {
#[allow(missing_docs)]
    Path(Path),
/// Variant.
    Fn {
/// Struct.
        params: Vec<TypeRef>,
/// Struct.
        ret: Option<Box<TypeRef>>,
    },
/// Variant.
    Ref {
/// Struct.
        inner: Box<TypeRef>,
/// Struct.
        mutability: Mutability,
    },
#[allow(missing_docs)]
    Slice(Box<TypeRef>),
/// Variant.
    Array {
/// Struct.
        inner: Box<TypeRef>,
/// Struct.
        len: ConstRef,
    },
#[allow(missing_docs)]
    Tuple(Vec<TypeRef>),
/// Variant.
    Never,
/// Variant.
    Infer,
    /// `dyn Trait` — an unsized trait object. The inner `TypeRef` is the
    /// trait (with its bounds). Lowered from `SyntaxKind::DynType` so that
    /// `dyn Trait` is distinguishable from merely naming the trait type.
    Dyn(Box<TypeRef>),
/// Variant.
    Error,
}

#[derive(Clone, Debug)]
/// ConstRef.
pub enum ConstRef {
#[allow(missing_docs)]
    Literal(Literal),
#[allow(missing_docs)]
    Path(Path),
/// Variant.
    Error,
}

#[derive(Clone, Debug)]
/// Path.
pub struct Path {
/// Struct.
    pub segments: Vec<PathSegment>,
/// Struct.
    pub kind: PathKind,
}

impl Path {
/// from_single.
    pub fn from_single(name: Name) -> Self {
        Self {
            segments: vec![PathSegment {
                name,
                generic_args: None,
            }],
            kind: PathKind::Plain,
        }
    }
/// as_name.
    pub fn as_name(&self) -> Option<Name> {
        if self.segments.len() == 1 && self.kind == PathKind::Plain {
            Some(self.segments[0].name)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
/// PathSegment.
pub struct PathSegment {
/// Struct.
    pub name: Name,
/// Struct.
    pub generic_args: Option<Vec<TypeRef>>,
}

#[derive(Clone, Debug)]
/// Body.
pub struct Body {
/// Struct.
    pub owner: LocalDefId,
/// Struct.
    pub exprs: IndexVec<ExprId, Expr>,
/// Struct.
    pub pats: IndexVec<PatId, Pat>,
/// Struct.
    pub params: Vec<PatId>,
/// Struct.
    pub span: Span,
/// Struct.
    pub expr_spans: IndexVec<ExprId, Span>, // Added field
}

#[derive(Clone, Debug)]
/// Expr.
pub enum Expr {
/// Variant.
    Missing,
#[allow(missing_docs)]
    Path(Path),
#[allow(missing_docs)]
    Literal(Literal),
/// Variant.
    Block {
/// Struct.
        stmts: Vec<ExprId>,
/// Struct.
        tail: Option<ExprId>,
    },
/// Variant.
    If {
/// Struct.
        cond: ExprId,
/// Struct.
        then_branch: ExprId,
/// Struct.
        else_branch: Option<ExprId>,
    },
/// Variant.
    While {
/// Struct.
        cond: ExprId,
/// Struct.
        body: ExprId,
    },
/// Variant.
    Loop {
/// Struct.
        body: ExprId,
    },
/// Variant.
    For {
/// Struct.
        pat: PatId,
/// Struct.
        iterable: ExprId,
/// Struct.
        body: ExprId,
    },
/// Variant.
    Match {
/// Struct.
        scrutinee: ExprId,
/// Struct.
        arms: Vec<MatchArm>,
    },
/// Variant.
    Call {
/// Struct.
        func: ExprId,
/// Struct.
        args: Vec<ExprId>,
    },
/// Variant.
    MethodCall {
/// Struct.
        receiver: ExprId,
/// Struct.
        method: Name,
/// Struct.
        args: Vec<ExprId>,
    },
/// Variant.
    Field {
/// Struct.
        receiver: ExprId,
/// Struct.
        field: Name,
    },
/// Variant.
    Index {
/// Struct.
        base: ExprId,
/// Struct.
        index: ExprId,
    },
/// Variant.
    Unary {
/// Struct.
        op: UnOp,
/// Struct.
        expr: ExprId,
    },
/// Variant.
    Binary {
/// Struct.
        op: BinOp,
/// Struct.
        lhs: ExprId,
/// Struct.
        rhs: ExprId,
    },
/// Variant.
    Cast {
/// Struct.
        expr: ExprId,
/// Struct.
        ty: TypeRef,
    },
/// Variant.
    Ref {
/// Struct.
        expr: ExprId,
/// Struct.
        mutability: Mutability,
    },
/// Variant.
    Assign {
/// Struct.
        lhs: ExprId,
/// Struct.
        rhs: ExprId,
    },
/// Variant.
    Return {
/// Struct.
        value: Option<ExprId>,
    },
/// Variant.
    Break {
/// Struct.
        value: Option<ExprId>,
    },
/// Variant.
    Continue,
/// Variant.
    Closure {
/// Struct.
        params: Vec<PatId>,
/// Struct.
        body: ExprId,
/// Struct.
        is_move: bool,
    },
#[allow(missing_docs)]
    Array(Vec<ExprId>),
#[allow(missing_docs)]
    Tuple(Vec<ExprId>),
    /// `let <pat> = <value>` — a named-binding statement (and the only place a
    /// new local is introduced). The pattern is bound into the local
    /// environment when this is converted to THIR `Stmt::Let`.
    Let {
/// Struct.
        pat: PatId,
/// Struct.
        value: ExprId,
    },
/// Variant.
    Struct {
/// Struct.
        path: Path,
#[doc = "field"]
        fields: Vec<(Name, ExprId)>,
/// Struct.
        spread: Option<ExprId>,
    },
/// Variant.
    Range {
/// Struct.
        start: Option<ExprId>,
/// Struct.
        end: Option<ExprId>,
/// Struct.
        inclusive: bool,
    },
    /// `e.await` — suspends until the future `e` resolves. Lowered by the
    /// async desugaring pass (`lower_async`) into a poll loop.
/// Struct.
    Await {
        /// expr field.
        expr: ExprId,
    },
/// Variant.
    Err,
}

#[derive(Clone, Debug)]
/// MatchArm.
pub struct MatchArm {
/// Struct.
    pub pat: PatId,
/// Struct.
    pub guard: Option<ExprId>,
/// Struct.
    pub body: ExprId,
}

#[derive(Clone, Debug)]
/// Pat.
pub enum Pat {
/// Variant.
    Wild,
/// Variant.
    Binding {
/// Struct.
        name: Name,
/// Struct.
        mutability: Mutability,
/// Struct.
        subpattern: Option<PatId>,
    },
/// Variant.
    Struct {
/// Struct.
        path: Path,
#[doc = "field"]
        fields: Vec<(Name, PatId)>,
/// Struct.
        rest: bool,
    },
#[allow(missing_docs)]
    Tuple(Vec<PatId>),
#[allow(missing_docs)]
    Slice(Vec<PatId>),
#[allow(missing_docs)]
    Or(Vec<PatId>),
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
    Path(Path),
/// Variant.
    Err,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Literal.
pub enum Literal {
#[allow(missing_docs)]
    Int(i128, Option<IntTy>),
#[allow(missing_docs)]
    Uint(u128, Option<UintTy>),
#[allow(missing_docs)]
    Float(u64, FloatTy),
#[allow(missing_docs)]
    Bool(bool),
#[allow(missing_docs)]
    Char(char),
#[allow(missing_docs)]
    String(Name),
/// Variant.
    Unit,
}

mod lower;

/// pipeline_api.
pub mod pipeline_api;
#[cfg(test)]
mod tests;
/// where_clause.
pub mod where_clause;

impl Body {
    /// Allocate a new expression with its span. This is the ONLY correct way
    /// to add an expression — it guarantees `exprs` and `expr_spans` stay
    /// in sync.
    #[inline]
    pub fn alloc_expr(&mut self, expr: Expr, span: Span) -> ExprId {
        debug_assert_eq!(
            self.exprs.len(),
            self.expr_spans.len(),
            "Body::alloc_expr: exprs and expr_spans out of sync before push"
        );
        let id = self.exprs.push(expr);
        self.expr_spans.push(span);
        id
    }

    /// Convenience: allocate a `Expr::Missing` placeholder.
    #[inline]
    pub fn alloc_missing(&mut self, span: Span) -> ExprId {
        self.alloc_expr(Expr::Missing, span)
    }

    /// Runtime invariant check. Call this after body construction.
    pub fn verify_invariants(&self) -> Result<(), String> {
        if self.exprs.len() != self.expr_spans.len() {
            return Err(format!(
                "Body invariant violated: exprs has {} entries but expr_spans has {}",
                self.exprs.len(),
                self.expr_spans.len()
            ));
        }
        Ok(())
    }
}
