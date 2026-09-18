//! Per-function type-checking engine.

use glyim_core::interner::Name;
use std::collections::HashMap;

use glyim_core::def_id::{DefId, LocalDefId};
use glyim_diag::GlyimDiagnostic;
use glyim_hir::*;
use glyim_solve::{InferenceTable, Obligation, TraitContext};
use glyim_type::{Ty, TyCtxMut};

use crate::env::LocalEnv;
use crate::thir;

#[allow(dead_code)]
pub struct FnCtxt<'a> {
    pub ctx: &'a mut TyCtxMut,
    pub infer: &'a mut InferenceTable,
    pub diagnostics: &'a mut Vec<GlyimDiagnostic>,
    pub pending_obligations: &'a mut Vec<Obligation>,
    pub hir: &'a CrateHir,
    pub body: &'a Body,
    pub env: LocalEnv,
    pub return_ty: Ty,
    pub owner: DefId,
    pub expr_cache: HashMap<ExprId, (thir::Expr, Ty)>,
    pub trait_ctx: &'a TraitContext,
    pub def_map: &'a glyim_def_map::CrateDefMap,
    /// Module that declares the function currently being checked. Used as the
    /// starting point for path resolution so bare names resolve against the
    /// function's own module (and walk up to the crate root), matching Rust's
    /// lexical scoping for paths rather than only the root scope.
    pub current_module: glyim_def_map::ModuleId,
    /// Per-body capture log: every `VarRef` id/type resolved while checking a
    /// `let`/closure body, in resolution order. Used by closure capture
    /// analysis (Tier 1.1) to classify captures by mutability and to filter
    /// out bindings that belong to the closure's own scope via the
    /// `LocalVarId` boundary.
    pub capture_log: Vec<(thir::LocalVarId, Ty, bool /* is_mut_use */)>,
    /// Maps each impl-method `BodyId` (HIR def-counter) to the `LocalDefId`
    /// typeck allocated for its MIR body, so trait-method static dispatch can
    /// resolve to the body key stored during monomorphization.
    pub body_owner_map: &'a HashMap<glyim_hir::BodyId, LocalDefId>,
    /// Generic parameters in scope for the body being checked (the enclosing
    /// fn's own params, plus any impl-level params for methods). Used by
    /// `check_expr` when resolving a `TypeRef` written *inside* the body
    /// (cast target, struct-literal path, …) so `T` / `F` resolve to the
    /// body's rigid type param rather than an "unresolved type" error.
    pub param_map: HashMap<Name, Ty>,
    /// Expected `FnPtr` signature for a closure literal about to be checked.
    /// `Expr::MethodCall` sets this before invoking `check_expr` on a closure
    /// argument whose formal type is a `FnPtr`; the `Expr::Closure` arm
    /// consumes it to seed the closure's own param/return types from the
    /// expected signature.
    pub pending_closure_expectation: Option<Ty>,
}
