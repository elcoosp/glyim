use crate::*;
use glyim_core::{CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability, UintTy};
use glyim_span::Span;
use glyim_type::{Const, ConstKind, Ty, TyCtxMut, TyKind};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0))
}

fn local_decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn empty_body(ret_ty: Ty) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_vec(vec![local_decl(ret_ty, Mutability::Mut)]);
    body
}

fn add_local(body: &mut Body, ty: Ty, mutability: Mutability) -> LocalIdx {
    let idx = LocalIdx::from_usize(body.locals.len());
    body.locals.push(local_decl(ty, mutability));
    idx
}

fn add_statement(body: &mut Body, bb: BasicBlockIdx, stmt: StatementKind) {
    while body.basic_blocks.len() <= bb.index() {
        body.basic_blocks.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        });
    }
    body.basic_blocks[bb].statements.push(Statement {
        kind: stmt,
        source_info: SourceInfo::new(Span::DUMMY),
    });
}

fn set_terminator(body: &mut Body, bb: BasicBlockIdx, kind: TerminatorKind) {
    while body.basic_blocks.len() <= bb.index() {
        body.basic_blocks.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        });
    }
    body.basic_blocks[bb].terminator.kind = kind;
}

fn const_int(_tcx: &mut TyCtxMut, val: i128, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: Span::DUMMY,
    })
}

fn mk_array_ty(tcx: &mut TyCtxMut, elem_ty: Ty, len: u64) -> Ty {
    let usize_ty = tcx.mk_ty(TyKind::Uint(UintTy::Usize));
    let const_len = Const {
        kind: ConstKind::Uint(len.into()),
        ty: usize_ty,
    };
    tcx.mk_ty(TyKind::Array(elem_ty, const_len))
}

#[test]
fn test_t09_repeat_rvalue() {
    let mut tcx = glyim_test::test_ty_ctx();
    let elem_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = mk_array_ty(&mut tcx, elem_ty, 3);
    let mut body = empty_body(Ty::UNIT);
    let local = add_local(&mut body, array_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_usize(0);
    let repeat = Rvalue::Repeat(
        const_int(&mut tcx, 5, elem_ty),
        MirConst {
            kind: MirConstKind::Uint(3),
            ty: tcx.mk_ty(TyKind::Uint(UintTy::Usize)),
            span: Span::DUMMY,
        },
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local), repeat),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Return);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body.clone());
    interp.run_body(&body).unwrap();
    let ret_val = interp.get_local_value(local).cloned().unwrap();
    let expected = InterpValue::Aggregate(vec![InterpValue::Int(5); 3]);
    assert_eq!(ret_val, expected);
}
