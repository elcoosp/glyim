//! V14-T05: Call with cleanup (landingpad stub)

use glyim_core::arena::IndexVec;
use glyim_core::{Abi, CrateId, DefId, Interner, LocalDefId, Mutability, Safety};
use glyim_mir::*;
use glyim_type::{FnSig, TyCtxMut, TyKind};

use crate::LlvmBackend;

fn make_cleanup_call_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I32));
    let fn_sig = FnSig {
        inputs: ctx.intern_substitution(vec![]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig.clone()));

    let return_ty = i32_ty;
    let arg_count = 1;

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: fn_ptr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let bb0 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                args: vec![],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: Some(BasicBlockIdx::from_raw(2)),
            },
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };

    let bb1 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };

    let bb2 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: true,
    };

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(4)),
        basic_blocks: IndexVec::from_raw(vec![bb0, bb1, bb2]),
        locals,
        arg_count,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn call_with_cleanup_compiles() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_cleanup_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(
        result.is_ok(),
        "cleanup call lowering failed: {:?}",
        result.err()
    );

    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("call"), "LLVM IR must contain 'call':\n{ir}");
}

#[test]
fn call_with_cleanup_has_landingpad() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_cleanup_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(result.is_ok());

    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("landingpad") || ir.contains("cleanup"),
        "LLVM IR with cleanup should contain landingpad or cleanup:\n{ir}"
    );
}
