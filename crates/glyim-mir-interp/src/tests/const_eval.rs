//! §4: const-evaluation entry point.
//!
//! A `const` / `const fn` is evaluated by running its MIR body and reading the
//! return slot. `Interpreter::const_eval` is the explicit wrapper that does
//! exactly that.

use crate::{InterpValue, Interpreter};
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, Interner, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{TyCtx, TyCtxMut, TyKind};

fn build_const_body(f: impl FnOnce(&mut TyCtxMut) -> Body) -> (TyCtx, Body) {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = f(&mut ctx_mut);
    (ctx_mut.freeze(), body)
}

/// A `const FOO: i32 = 3 + 4;` style expression evaluates to 7 via `const_eval`.
#[test]
fn const_eval_additive_constant() {
    let (ctx, body) = build_const_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::BinaryOp(
            BinOp::Add,
            Box::new((
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(3),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(4),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
            )),
        );
        let stmt = Statement {
            kind: StatementKind::Assign(Place::new(ret_local), rvalue),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt],
            terminator: term,
            is_cleanup: false,
        });
        Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: blocks,
            locals,
            arg_count: 0,
            return_ty: i32_ty,
            span: Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let mut interp = Interpreter::new(&ctx);
    let val = interp
        .const_eval(&body)
        .expect("const-eval of 3 + 4 should succeed");
    assert_eq!(
        val,
        InterpValue::Int(7),
        "Expected const-eval to fold 3 + 4 = 7, got {val:?}"
    );
}

/// A const expression that divides by zero must surface `DivisionByZero`
/// rather than panic the host.
#[test]
fn const_eval_division_by_zero_is_error() {
    let (ctx, body) = build_const_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::BinaryOp(
            BinOp::Div,
            Box::new((
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(10),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
            )),
        );
        let stmt = Statement {
            kind: StatementKind::Assign(Place::new(ret_local), rvalue),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt],
            terminator: term,
            is_cleanup: false,
        });
        Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: blocks,
            locals,
            arg_count: 0,
            return_ty: i32_ty,
            span: Span::DUMMY,
            var_debug_info: vec![],
        }
    });

    let mut interp = Interpreter::new(&ctx);
    let result = interp.const_eval(&body);
    assert!(
        matches!(result, Err(crate::InterpError::DivisionByZero)),
        "const-eval division by zero must be a DivisionByZero error, got {result:?}"
    );
}
