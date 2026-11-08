use glyim_core::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Const, ConstKind, Ty, TyCtxMut, TyKind};

pub(crate) fn empty_body(return_ty: Ty) -> Body {
    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    body.return_ty = return_ty;
    let mut locals = IndexVec::new();
    // local 0 is return place
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals = locals;
    body.basic_blocks = IndexVec::new();
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body
}

pub(crate) fn add_local(body: &mut Body, ty: Ty, mutability: Mutability) -> LocalIdx {
    let idx = body.locals.len();
    body.locals.push(LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    LocalIdx::from_raw(idx as u32)
}

pub(crate) fn add_statement(body: &mut Body, bb: BasicBlockIdx, kind: StatementKind) {
    body.basic_blocks[bb].statements.push(Statement {
        kind,
        source_info: SourceInfo::new(Span::DUMMY),
    });
}

pub(crate) fn set_terminator(body: &mut Body, bb: BasicBlockIdx, terminator: TerminatorKind) {
    body.basic_blocks[bb].terminator.kind = terminator;
}

pub(crate) fn const_int(val: i128) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(val),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    })
}

pub(crate) fn const_bool(b: bool) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Bool(b),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    })
}

pub(crate) fn place_with_proj(local: LocalIdx, proj: Vec<ProjectionElem>) -> Place {
    Place {
        local,
        projection: proj.into_boxed_slice(),
    }
}

pub(crate) fn mk_array_ty(ctx: &mut TyCtxMut, elem_ty: Ty, len: u64) -> Ty {
    let len_const = Const {
        kind: ConstKind::Int(i128::from(len)),
        ty: ctx.mk_ty(TyKind::Int(IntTy::I64)),
    };
    ctx.mk_ty(TyKind::Array(elem_ty, len_const))
}
