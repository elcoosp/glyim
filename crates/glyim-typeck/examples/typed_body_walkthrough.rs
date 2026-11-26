//! Typed-THIR boundary walkthrough: the plan M5.2 seam between type
//! checking and MIR lowering.
//!
//! `glyim-typeck` had no consumer walkthrough for its new typed boundary so
//! far: since M5.2, the only body form that leaves type checking is
//! [`TypedBody`] — constructible exclusively through the checked
//! [`TypedBody::try_from_body`], which refuses any body whose declared
//! signature slots (parameters, return type, deeply through ADT/fn/closure
//! substitutions) still carry an inference variable or the post-zonk
//! `Ty::ERROR` sentinel. MIR lowering's public entry point
//! (`glyim_lower::lower_typed_body`) accepts only this form, so an unverified
//! body is unrepresentable at the lowering boundary (constitution C3). This
//! walkthrough builds the two shapes a pipeline author actually makes: a
//! fully resolved body that passes the gate, and the unbound-type-variable
//! body it rejects. Run with:
//!
//! ```text
//! cargo run -p glyim-typeck --example typed_body_walkthrough
//! ```

use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::interner::Interner;
use glyim_solve::InferenceTable;
use glyim_span::Span;
use glyim_type::{InferVar, Ty, TyCtxMut, TyKind};
use glyim_typeck::TypedBody;
use glyim_typeck::thir;

fn main() {
    let interner = Interner::new();
    let param_name = interner.intern("x");
    let ctx = TyCtxMut::new(interner);

    // A fully resolved body: `fn add(x: i32) -> i32 { x + 1 }`-shaped, with
    // every declared slot concrete. The checked constructor accepts it and
    // the signature slots come back as `ResolvedTy` (plan M5.2: "TypedBody
    // holds ResolvedTy").
    let clean = thir::Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        params: vec![thir::Param {
            name: param_name,
            ty: Ty::I32,
            span: Span::DUMMY,
            pat: thir::Pattern {
                kind: thir::PatternKind::Wild,
                ty: Ty::I32,
                span: Span::DUMMY,
            },
            local: thir::LocalVarId::from_usize(0),
        }],
        return_ty: Ty::I32,
        stmts: vec![thir::Stmt::Return {
            value: Some(thir::Expr {
                kind: thir::ExprKind::Literal(thir::Literal::Int(1, None)),
                ty: Ty::I32,
                span: Span::DUMMY,
            }),
            span: Span::DUMMY,
        }],
        span: Span::DUMMY,
    };
    let frozen = ctx.freeze();
    let typed =
        TypedBody::try_from_body(clean, &frozen).expect("a concrete-i32 body is fully resolved");
    println!(
        "accepted: return_ty={:?} params={}",
        typed.return_ty().get(),
        typed.param_tys().count()
    );

    // The rejection path: an unbound `Infer(Ty(_))` in the return slot —
    // the shape zonk folds to `Ty::ERROR` and that historically reached
    // codegen as `UnknownType(Ty(0))` inside ADT substitutions. The gate
    // refuses it with the offending slot so the pipeline can diagnose it
    // (reachability-gated "type annotations needed") instead of lowering it.
    let mut ctx2 = TyCtxMut::new(glyim_core::interner::Interner::new());
    let mut infer = InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx2);
    let unbound = ctx2.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let error_body = thir::Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(1)),
        params: Vec::new(),
        return_ty: unbound,
        stmts: vec![],
        span: Span::DUMMY,
    };
    let frozen2 = ctx2.freeze();
    match TypedBody::try_from_body(error_body, &frozen2) {
        Err(unresolved) => println!(
            "rejected: slot type {:?} must never reach lowering (plan M5.2)",
            unresolved.ty
        ),
        Ok(_) => panic!("an unbound inference variable must be rejected"),
    }
}
