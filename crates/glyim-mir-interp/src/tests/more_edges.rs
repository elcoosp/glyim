use crate::Interpreter;
use glyim_core::{CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyKind};

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

#[test]
fn fn_ptr_to_ptr_cast_returns_success() {
    let mut tcx = test_ty_ctx();
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let ptr_ty = tcx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));
    let const_val = MirConst {
        kind: MirConstKind::Int(42),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    let operand = Operand::Constant(const_val);
    let cast_rvalue = Rvalue::Cast(CastKind::PtrToPtr, operand, ptr_ty);
    let assign_stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_usize(0)), cast_rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let mut body = empty_body(ptr_ty);
    body.locals = IndexVec::from_vec(vec![local_decl(ptr_ty, Mutability::Mut)]);
    body.basic_blocks = IndexVec::from_vec(vec![BasicBlockData {
        statements: vec![assign_stmt],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body.clone());
    let res = interp.run_body(&body);
    assert!(res.is_ok());
}
