use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::interner::Name;
/// Common utilities for typeck tests.
use glyim_diag::GlyimDiagnostic;
use glyim_hir::*;
use glyim_solve::{InferenceTable, TraitContext};
use glyim_span::Span;
use glyim_type::{Ty, TyCtxMut};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::check_body::FnCtxt;
use crate::env::LocalEnv;
use crate::thir;

// ------------------------------------------------------------------
// Global interner for all tests (prevents lasso key errors)
// ------------------------------------------------------------------
static GLOBAL_INTERNER: Lazy<Mutex<glyim_core::interner::Interner>> =
    Lazy::new(|| Mutex::new(glyim_core::interner::Interner::new()));

/// Get a clone of the global interner.
pub fn global_interner() -> glyim_core::interner::Interner {
    GLOBAL_INTERNER.lock().unwrap().clone()
}

/// Create a Name from a string using the global interner.
pub fn name(s: &str) -> glyim_core::interner::Name {
    let interner = GLOBAL_INTERNER.lock().unwrap();
    interner.intern(s)
}

/// Test helper: constructs FnCtxt and runs the check.
pub fn check_function_body(
    ctx: &mut TyCtxMut,
    infer: &mut InferenceTable,
    def_map: &glyim_def_map::CrateDefMap,
    hir: &CrateHir,
    body_id: BodyId,
    owner: DefId,
    return_ty: Ty,
    params: &[(Name, Ty, Span)],
) -> (thir::Body, Vec<GlyimDiagnostic>) {
    let body = &hir.bodies[body_id];
    let mut diagnostics = Vec::new();
    let mut obligations = Vec::new();
    let env = LocalEnv::new();

    let trait_ctx = TraitContext::new();
    let fn_ctxt = FnCtxt {
        ctx,
        infer,
        diagnostics: &mut diagnostics,
        pending_obligations: &mut obligations,
        hir,
        body,
        env,
        return_ty,
        owner,
        expr_cache: HashMap::new(),
        def_map,
        current_module: glyim_def_map::ModuleId::from_raw(0),
        trait_ctx: &trait_ctx,
        capture_log: Vec::new(),
        body_owner_map: &HashMap::new(),
    };

    let (thir_body, _expr_types) = fn_ctxt.check(params);
    (thir_body, diagnostics)
}

/// Build a minimal HIR with a single body from a list of expressions.
pub fn make_single_body_hir(
    exprs: Vec<glyim_hir::Expr>,
) -> (glyim_hir::CrateHir, glyim_hir::BodyId) {
    use glyim_core::arena::IndexVec;
    use glyim_core::def_id::LocalDefId;
    use glyim_hir::{Body, CrateHir};
    let mut hir = CrateHir {
        items: IndexVec::new(),
        bodies: IndexVec::new(),
        body_owners: IndexVec::new(),
        interner: Default::default(),
    };
    let mut expr_vec = IndexVec::new();
    for expr in exprs {
        expr_vec.push(expr);
    }
    let expr_spans = IndexVec::from_raw(vec![Span::DUMMY; expr_vec.len() + 1]);
    // Wrap the expression list in a root `Block` so the type checker's
    // root-Block driver (check_stmt.rs) iterates every expression as a
    // top-level statement — matching both real HIR lowering (which always
    // produces a root block) and the pre-flat-iteration test expectations.
    let block_expr = glyim_hir::Expr::Block {
        stmts: (0..expr_vec.len() as u32)
            .map(glyim_hir::ExprId::from_raw)
            .collect(),
        tail: None,
    };
    expr_vec.push(block_expr);
    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: expr_vec,
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans,
    };
    let body_id = hir.bodies.push(body);
    (hir, body_id)
}

/// Type‑check a single body by constructing a real `FnCtxt`.
pub fn typeck_single_body(hir: &CrateHir, body_id: BodyId) -> crate::thir::Body {
    let mut ctx = make_ty_ctx();
    let mut infer = InferenceTable::new();
    let def_map = empty_def_map();
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));

    let (thir_body, _diags) = check_function_body(
        &mut ctx,
        &mut infer,
        &def_map,
        hir,
        body_id,
        owner,
        Ty::UNIT,
        &[],
    );
    thir_body
}

/// Dummy empty def map (used by some old test files).
pub fn empty_def_map() -> glyim_def_map::CrateDefMap {
    use glyim_core::arena::IndexVec;
    use glyim_core::def_id::CrateId;
    use glyim_def_map::{CrateDefMap, ItemScope, ModuleData, ModuleId, ModuleOrigin};
    use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
    let interner = global_interner();
    let root = ModuleId::from_raw(0);
    let module_data = ModuleData {
        parent: None,
        children: vec![],
        scope: ItemScope::default(),
        origin: ModuleOrigin::CrateRoot,
        span: Span::new(
            FileId::from_raw(0),
            ByteIdx::from_raw(0),
            ByteIdx::from_raw(0),
            SyntaxContext::ROOT,
        ),
        def_id: glyim_core::def_id::LocalDefId::from_raw(0),
        visibility: glyim_core::primitives::Visibility::Public,
    };
    let modules = IndexVec::from_raw(vec![module_data]);
    CrateDefMap {
        root,
        modules,
        krate: CrateId::from_raw(0),
        interner,
    variant_map: Default::default(),
    }
}

/// Dummy type context for tests.
pub fn make_ty_ctx() -> glyim_type::TyCtxMut {
    glyim_type::TyCtxMut::new(global_interner())
}
