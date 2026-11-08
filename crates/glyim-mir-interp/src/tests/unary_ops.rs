use crate::{InterpValue, Interpreter};
use glyim_core::{CrateId, DefId, IndexVec, IntTy, Interner, LocalDefId, Mutability, UnOp};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{TyCtx, TyCtxMut, TyKind};

fn setup_test_body(f: impl FnOnce(&mut TyCtxMut) -> Body) -> (TyCtx, Body) {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = f(&mut ctx_mut);
    (ctx_mut.freeze(), body)
}

#[test]
fn w1_c03_t02_not_bool() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let bool_ty = ctx_mut.mk_ty(TyKind::Bool);
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: bool_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::UnaryOp(
            UnOp::Not,
            Operand::Constant(MirConst {
                kind: MirConstKind::Bool(true),
                ty: bool_ty,
                span: Span::DUMMY,
            }),
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
            return_ty: bool_ty,
            span: Span::DUMMY,
            var_debug_info: vec![],
        }
    });
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok(), "Interpreter failed: {:?}", result.err());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Bool(false),
        "Expected !true = false, got {ret:?}"
    );
}

#[test]
fn not_int() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::UnaryOp(
            UnOp::Not,
            Operand::Constant(MirConst {
                kind: MirConstKind::Int(0),
                ty: i32_ty,
                span: Span::DUMMY,
            }),
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
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Int(-1),
        "Expected !0i32 = -1, got {ret:?}"
    );
}

#[test]
fn neg_int() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::UnaryOp(
            UnOp::Neg,
            Operand::Constant(MirConst {
                kind: MirConstKind::Int(42),
                ty: i32_ty,
                span: Span::DUMMY,
            }),
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
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(ret, InterpValue::Int(-42), "Expected -42, got {ret:?}");
}
