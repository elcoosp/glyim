use crate::{InterpValue, Interpreter};
use glyim_core::{CrateId, DefId, IndexVec, IntTy, Interner, LocalDefId, Mutability, UintTy};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Const, ConstKind, GenericArg, TyCtx, TyCtxMut, TyKind};

fn setup_test_body(f: impl FnOnce(&mut TyCtxMut) -> Body) -> (TyCtx, Body) {
    let mut ctx_mut = TyCtxMut::new(Interner::default());
    let body = f(&mut ctx_mut);
    (ctx_mut.freeze(), body)
}

#[test]
fn w1_c03_t03_len_array() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let usize_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize));
        let len_const = Const {
            kind: ConstKind::Int(3),
            ty: usize_ty,
        };
        let array_ty = ctx_mut.mk_ty(TyKind::Array(i32_ty, len_const));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let arr_local = locals.push(LocalDecl {
            ty: array_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let stmt_init = Statement {
            kind: StatementKind::Assign(
                Place::new(arr_local),
                Rvalue::Aggregate(
                    AggregateKind::Array(array_ty),
                    vec![
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
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(3),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        }),
                    ],
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let stmt_len = Statement {
            kind: StatementKind::Assign(Place::new(ret_local), Rvalue::Len(Place::new(arr_local))),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt_init, stmt_len],
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
    assert_eq!(ret, InterpValue::Int(3), "Expected len = 3, got {ret:?}");
}

#[test]
fn discriminant_variant_0() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let sub = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(sub));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let val_local = locals.push(LocalDecl {
            ty: tuple_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let stmt_init = Statement {
            kind: StatementKind::Assign(
                Place::new(val_local),
                Rvalue::Aggregate(
                    AggregateKind::Tuple,
                    vec![Operand::Constant(MirConst {
                        kind: MirConstKind::Int(0),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })],
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let stmt_disc = Statement {
            kind: StatementKind::Assign(
                Place::new(ret_local),
                Rvalue::Discriminant(Place::new(val_local)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt_init, stmt_disc],
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
    assert!(result.is_ok(), "Discriminant failed: {:?}", result.err());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Int(0),
        "Expected discriminant = 0, got {ret:?}"
    );
}

#[test]
fn discriminant_variant_1() {
    let (ctx, body) = setup_test_body(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let sub = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(sub));
        let mut locals = IndexVec::new();
        let ret_local = locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let val_local = locals.push(LocalDecl {
            ty: tuple_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let mut blocks = IndexVec::new();
        let stmt_init = Statement {
            kind: StatementKind::Assign(
                Place::new(val_local),
                Rvalue::Aggregate(
                    AggregateKind::Tuple,
                    vec![Operand::Constant(MirConst {
                        kind: MirConstKind::Int(5),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })],
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let stmt_disc = Statement {
            kind: StatementKind::Assign(
                Place::new(ret_local),
                Rvalue::Discriminant(Place::new(val_local)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let term = Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        };
        blocks.push(BasicBlockData {
            statements: vec![stmt_init, stmt_disc],
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
    assert!(result.is_ok(), "Discriminant failed: {:?}", result.err());
    let ret = interp.get_return_value().unwrap();
    assert_eq!(
        ret,
        InterpValue::Int(5),
        "Expected discriminant = 5, got {ret:?}"
    );
}
