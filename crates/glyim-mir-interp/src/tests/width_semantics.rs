//! MIR-13 / MIR-14 regression: interpreter arithmetic and casts must be
//! width-correct, matching the LLVM backend. Previously `InterpValue::Int(i128)`
//! never truncated, so `100i8 + 100i8` gave 200 (LLVM: -56) and
//! `(-1i32) as u8` gave -1 (LLVM: 255).

use crate::*;
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability, UintTy};
use glyim_mir::LocalIdx;
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyCtxMut, TyKind};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

fn local_decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn run_binop(tcx: &mut TyCtxMut, ty: Ty, op: BinOp, lhs: i128, rhs: i128) -> InterpValue {
    run_binop_kind(tcx, ty, op, MirConstKind::Int(lhs), MirConstKind::Int(rhs))
}

fn run_binop_kind(
    tcx: &mut TyCtxMut,
    ty: Ty,
    op: BinOp,
    lhs: MirConstKind,
    rhs: MirConstKind,
) -> InterpValue {
    let mut body = Body::dummy(dummy_def_id());
    let res = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(ty, Mutability::Mut),
    ]);
    let c1 = Operand::Constant(MirConst { kind: lhs, ty, span: Span::DUMMY });
    let c2 = Operand::Constant(MirConst { kind: rhs, ty, span: Span::DUMMY });
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(res),
                Rvalue::BinaryOp(op, Box::new((c1, c2))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator { kind: TerminatorKind::Return, source_info: SourceInfo::new(Span::DUMMY) },
        is_cleanup: false,
    }]);
    let frozen = tcx.freeze();
    let mut interp = Interpreter::new(&frozen);
    interp.run_body(&body).unwrap();
    interp.get_local_value(res).unwrap().clone()
}

/// MIR-13: `100i8 + 100i8` wraps to -56, not 200.
#[test]
fn i8_add_wraps_to_width() {
    let mut tcx = test_ty_ctx();
    let ty = tcx.mk_ty(TyKind::Int(IntTy::I8));
    let v = run_binop(&mut tcx, ty, BinOp::Add, 100, 100);
    assert_eq!(v, InterpValue::Int(-56), "100i8 + 100i8 must wrap to -56");
}

/// MIR-13: `200u8 + 100u8` wraps to 44.
#[test]
fn u8_add_wraps_to_width() {
    let mut tcx = test_ty_ctx();
    let ty = tcx.mk_ty(TyKind::Uint(UintTy::U8));
    let v = run_binop_kind(
        &mut tcx,
        ty,
        BinOp::Add,
        MirConstKind::Uint(200),
        MirConstKind::Uint(100),
    );
    assert_eq!(v, InterpValue::Uint(44), "200u8 + 100u8 must wrap to 44");
}

/// MIR-14: `(-1i32) as u8` == 255.
#[test]
fn int_to_int_cast_truncates() {
    let mut tcx = test_ty_ctx();
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let u8_ty = tcx.mk_ty(TyKind::Uint(UintTy::U8));
    let mut body = Body::dummy(dummy_def_id());
    let res = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(u8_ty, Mutability::Mut),
    ]);
    let c = Operand::Constant(MirConst { kind: MirConstKind::Int(-1), ty: i32_ty, span: Span::DUMMY });
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(res),
                Rvalue::Cast(glyim_mir::CastKind::IntToInt, c, u8_ty),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator { kind: TerminatorKind::Return, source_info: SourceInfo::new(Span::DUMMY) },
        is_cleanup: false,
    }]);
    let frozen = tcx.freeze();
    let mut interp = Interpreter::new(&frozen);
    interp.run_body(&body).unwrap();
    let v = interp.get_local_value(res).unwrap().clone();
    assert_eq!(v, InterpValue::Int(255), "(-1i32) as u8 must be 255");
}
