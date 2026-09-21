//! V14-T01: Call a defined function produces LLVM `call` instruction

use glyim_core::arena::IndexVec;
use glyim_core::def_id::ClosureId;
use glyim_core::{Abi, CrateId, DefId, Interner, LocalDefId, Mutability, Safety};
use glyim_mir::*;
use glyim_type::{FnSig, GenericArg, TyCtxMut, TyKind};
use std::sync::Arc;

use crate::LlvmBackend;

fn make_simple_call_body(ctx: &mut TyCtxMut) -> Body {
    let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I32));
    let fn_sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig.clone()));

    let return_ty = i32_ty;
    let arg_count = 2;

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: fn_ptr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let const_42 = MirConst {
        kind: MirConstKind::Uint(42),
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };

    let bb0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(2)),
                Rvalue::Use(Operand::Constant(const_42)),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
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
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::from_raw(vec![bb0, bb1]),
        locals,
        arg_count,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

#[test]
fn call_direct_produces_call_instruction() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_simple_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(
        result.is_ok(),
        "lower_body_to_module failed: {:?}",
        result.err()
    );

    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call"),
        "LLVM IR must contain 'call' instruction:\n{}",
        ir
    );
}

#[test]
fn call_direct_has_function_with_body() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = make_simple_call_body(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(result.is_ok());

    let module = result.unwrap();
    let func = module.get_first_function();
    assert!(func.is_some(), "Module must contain at least one function");
    let f = func.unwrap();
    assert!(
        f.count_basic_blocks() >= 1,
        "Function must have at least one basic block"
    );
}

/// V14-T01 extension: a call via a `MirConstKind::Fn(FnDefId, _)` constant must
/// resolve to a function whose emitted name matches the canonical
/// `__glyim_fn_{fndefid}` contract (the name `lower_call` / `MirConstKind::Fn`
/// look up). This locks in the emit/lookup naming unification: previously
/// `lower_body` emitted `func_{krate}_{local}` which no caller could resolve.
#[test]
fn call_via_fndef_const_resolves_to_glyim_fn_symbol() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::IntTy::I32));
    let fn_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(fn_sig.clone()));
    // The callee's FnDefId — by convention it equals the function's local def id.
    let callee_id = glyim_core::def_id::FnDefId::from_raw(7);
    ctx_mut.register_fn_sig(callee_id, fn_sig);

    let return_ty = i32_ty;
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: fn_ptr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let fn_const = MirConst {
        kind: MirConstKind::Fn(callee_id, glyim_type::Substitution::empty()),
        ty: fn_ptr_ty,
        span: glyim_span::Span::DUMMY,
    };
    let bb0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(2)),
                Rvalue::Use(Operand::Constant(fn_const)),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
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
    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::from_raw(vec![bb0, bb1]),
        locals,
        arg_count: 2,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let ctx = ctx_mut.freeze();
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let result = backend.lower_body_to_module(&inkwell_ctx, &body);
    assert!(
        result.is_ok(),
        "lower_body_to_module with FnDef const call failed: {:?}",
        result.err()
    );
    let module = result.unwrap();
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("call") && ir.contains("__glyim_fn_7"),
        "call via FnDef constant must target the emitted __glyim_fn_7 symbol:\n{}",
        ir
    );
    // The symbol must be a *defined* function (not merely declared), so a
    // later link step can resolve the call.
    assert!(
        ir.contains("define") && ir.contains("__glyim_fn_7"),
        "call target __glyim_fn_7 must be a defined function:\n{}",
        ir
    );
}

/// P6.2: a closure value (aggregate of captures) can be *called*. The closure
/// body is emitted as `__glyim_fn_{closure_id}` (matching the naming contract),
/// and the call lowers to a `call` against that defined symbol with the captures
/// passed as leading arguments.
#[test]
fn call_closure_value_lowers_to_defined_closure_fn() {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::IntTy::I32));

    // Register the synthetic closure ADT (one captured i32). The closure id
    // convention maps AdtId <-> ClosureId by their raw value.
    let closure_adt =
        ctx_mut.register_closure(vec![(ctx_mut.resolver().intern("capture_0"), i32_ty)]);
    let closure_id = ClosureId::from_raw(closure_adt.to_raw());
    // The closure *value* type is `TyKind::Closure`; its `substs` carry the
    // capture types so the value lays out as a struct of captures (matching the
    // synthetic ADT's fields used for call-site extraction).
    let closure_substs = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let closure_ty = ctx_mut.mk_ty(TyKind::Closure(closure_id, closure_substs));

    // Register the closure's full signature so the codegen can recover the
    // `[captures..., params] -> ret` shape at call sites.
    let closure_sig = FnSig {
        inputs: ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(i32_ty)]),
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    ctx_mut.register_closure_sig(closure_id, closure_sig);

    let return_ty = i32_ty;

    // --- Caller body ---
    // locals: 0=ret, 1=explicit i32 arg, 2=closure aggregate
    let mut caller_locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    caller_locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    caller_locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    caller_locals.push(LocalDecl {
        ty: closure_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    // Build the closure aggregate: captures only (no fn-ptr field).
    let capture_const = MirConst {
        kind: MirConstKind::Uint(10),
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    let build_closure = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(2)),
            Rvalue::Aggregate(
                AggregateKind::Closure(closure_id, glyim_type::Substitution::empty()),
                vec![Operand::Constant(capture_const)],
            ),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };
    let arg_const = MirConst {
        kind: MirConstKind::Uint(32),
        ty: i32_ty,
        span: glyim_span::Span::DUMMY,
    };
    let bb0 = BasicBlockData {
        statements: vec![build_closure],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                args: vec![Operand::Constant(arg_const)],
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
    let caller_body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::from_raw(vec![bb0, bb1]),
        locals: caller_locals,
        arg_count: 0,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    // --- Closure body: fn(capture: i32, param: i32) -> i32, returns capture ---
    // locals: 0=ret, 1=capture, 2=param. arg_count=2 (1 capture + 1 param).
    let mut clo_locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    clo_locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    clo_locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    clo_locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    let clo_bb = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        is_cleanup: false,
    };
    let closure_body = Body {
        owner: DefId::new(
            CrateId::from_raw(0),
            LocalDefId::from_raw(closure_id.to_raw()),
        ),
        basic_blocks: IndexVec::from_raw(vec![clo_bb]),
        locals: clo_locals,
        arg_count: 2,
        return_ty,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let ctx = ctx_mut.freeze();
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let inkwell_ctx = inkwell::context::Context::create();
    let module = backend
        .lower_bodies_to_module(
            &inkwell_ctx,
            &[Arc::new(closure_body), Arc::new(caller_body)],
        )
        .expect("lowering caller + closure body failed");
    let ir = module.print_to_string().to_string();

    // The call must target the *defined* closure function.
    let fn_name = format!("__glyim_fn_{}", closure_id.to_raw());
    assert!(
        ir.contains("call") && ir.contains(&fn_name),
        "closure call must target the emitted {} symbol:\n{}",
        fn_name,
        ir
    );
    assert!(
        ir.contains("define") && ir.contains(&fn_name),
        "closure body {} must be a defined function:\n{}",
        fn_name,
        ir
    );
}
