use crate::{InterpValue, Interpreter};
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, Interner, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{TyCtx, TyCtxMut, TyKind};

fn setup_test_body(f: impl FnOnce(&mut TyCtxMut) -> Body) -> (TyCtx, Body) {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = f(&mut ctx_mut);
    (ctx_mut.freeze(), body)
}

#[test]
fn w1_c03_t01_left_shift() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::BinaryOp(
            BinOp::Shl,
            Box::new((
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(2),
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
    let result = interp.run_body(&body);
    assert!(result.is_ok(), "Interpreter failed: {:?}", result.err());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(ret, InterpValue::Int(4), "Expected 1 << 2 = 4, got {ret:?}");
}

#[test]
fn bitwise_and() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::BinaryOp(
            BinOp::BitAnd,
            Box::new((
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0b1100),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0b1010),
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
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Int(0b1000),
        "Expected 0b1100 & 0b1010 = 0b1000, got {ret:?}"
    );
}

#[test]
fn bitwise_or() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::BinaryOp(
            BinOp::BitOr,
            Box::new((
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0b1100),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0b1010),
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
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Int(0b1110),
        "Expected 0b1100 | 0b1010 = 0b1110, got {ret:?}"
    );
}

#[test]
fn bitwise_xor() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::BinaryOp(
            BinOp::BitXor,
            Box::new((
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0b1100),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0b1010),
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
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Int(0b0110),
        "Expected 0b1100 ^ 0b1010 = 0b0110, got {ret:?}"
    );
}

#[test]
fn right_shift() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let rvalue = Rvalue::BinaryOp(
            BinOp::Shr,
            Box::new((
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(16),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(2),
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
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Int(4),
        "Expected 16 >> 2 = 4, got {ret:?}"
    );
}
