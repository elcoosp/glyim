//! Call with multiple arguments of different types

use glyim_core::arena::IndexVec;
use glyim_core::{Abi, CrateId, DefId, FloatTy, IntTy, Interner, LocalDefId, Mutability, Safety};
use glyim_mir::*;
use glyim_type::{FnSig, GenericArg, TyCtxMut, TyKind};

use crate::LlvmBackend;

fn make_multi_arg_call_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let i64_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
    let f64_ty = ctx.mk_ty(TyKind::Float(FloatTy::F64));
    let bool_ty = ctx.mk_ty(TyKind::Bool);

    let fn_sig = FnSig {
        inputs: ctx.intern_substitution(vec![
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(i64_ty),
            GenericArg::Ty(f64_ty),
            GenericArg::Ty(bool_ty),
        ]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig.clone()));

    let return_ty = i32_ty;
    let arg_count = 4; // the function takes 4 arguments

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    // local 0: return place
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    // local 1..4: parameters
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: i64_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: f64_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    // local 5: function pointer (not a parameter)
    locals.push(LocalDecl {
        ty: fn_ptr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let bb0 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Copy(Place::new(LocalIdx::from_raw(5))),
                args: vec![
                    Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                    Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                    Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                    Operand::Copy(Place::new(LocalIdx::from_raw(4))),
                ],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
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

    Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(10)),
        basic_blocks: IndexVec::from_raw(vec![bb0, bb1]),
        locals,
        arg_count,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn call_with_multiple_args_compiles() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_multi_arg_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(
        result.is_ok(),
        "multi-arg call lowering failed: {:?}",
        result.err()
    );
}

#[test]
fn call_with_multiple_args_has_call_ir() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_multi_arg_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(result.is_ok());

    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("call"), "LLVM IR must contain 'call':\n{ir}");
}
