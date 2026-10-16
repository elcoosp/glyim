use crate::LlvmBackend;
use glyim_core::Interner;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, SourceInfo, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_type::{TyCtxMut, TyKind};

/// Build a trivial body whose owner is `FnDefId(7)`. No `FnSig` is registered
/// for that def id. A `Body` is self-describing (it carries its return type and
/// local declarations), so the codegen pass must lower it successfully by
/// deriving the signature from the body rather than erroring (Tier 5.3).
fn build_body_without_fn_sig(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let mut basic_blocks: IndexVec<glyim_mir::BasicBlockIdx, BasicBlockData> = IndexVec::new();
    let bb0 = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    basic_blocks.push(bb0);

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(7)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: i32_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

#[test]
fn t53_body_without_fn_sig_lowers_via_fallback() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let body = build_body_without_fn_sig(&mut ctx);
    let frozen = ctx.freeze();

    // Deliberately do NOT register an FnSig for FnDefId(7). The codegen pass
    // must derive the signature from the body and lower successfully.
    let backend = LlvmBackend::new();
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module_with_ctx(&context, &body, &frozen, None)
        .expect(
            "lowering a body without a registered FnSig must fall back to \
             the body-derived signature and succeed",
        );
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("@__glyim_fn_7"),
        "expected a function named __glyim_fn_7, got:\n{}",
        ir
    );
    // The default Glyim ABI emits the `fastcc` calling convention (distinct
    // from the C ABI used by `extern \"C\" fn`), which is why the prefix is
    // present in the IR.
    assert!(
        ir.contains("fastcc"),
        "expected the Glyim default (fastcc) calling convention, got:\n{}",
        ir
    );
}
