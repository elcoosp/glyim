use super::helpers::*;
use crate::Interpreter;
use glyim_core::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{FieldIdx, Region, Substitution, Ty, TyKind};

// Reproduces the Phase-1 for-loop scenario at the MIR level:
// `main` builds an aggregate (Counter-like) and a `&mut` ref into its own
// frame's local, then CALLS `next` passing that ref. `next` derefs the ref to
// read a field. With a frame-local `Ref(target)` model this should FAIL with
// "deref of uninitialized local N" — proving the interpreter needs a
// frame-aware ref model.
#[test]
fn xref_probe_cross_frame_mut_self_deref() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Mut);
    let agg_ty = ctx.mk_ty(TyKind::Tuple(Substitution::empty()));

    // ---- next: fn next(&mut self) -> i32 ----
    // arg 1 = &mut self (points into caller frame, target local).
    // body: return_place = (self.deref).field0
    let mut next = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)));
    next.return_ty = i32_ty;
    next.arg_count = 1;
    next.locals = IndexVec::from_raw(vec![
        LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: ref_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let nbb0 = BasicBlockIdx::from_raw(0);
    next.basic_blocks = IndexVec::from_raw(vec![BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    })]);
    add_statement(
        &mut next,
        nbb0,
        StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Copy(Place {
                local: LocalIdx::from_raw(1),
                projection: Box::new([
                    ProjectionElem::Deref,
                    ProjectionElem::Field(FieldIdx::from_raw(0)),
                ]),
            })),
        ),
    );

    // ---- main ----
    let mut main = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    main.return_ty = Ty::UNIT;
    main.arg_count = 0;
    let main_agg = LocalIdx::from_raw(1);
    let main_ref = LocalIdx::from_raw(2);
    let main_tmp = LocalIdx::from_raw(3);
    main.locals = IndexVec::from_raw(vec![
        LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: agg_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: ref_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    main.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }),
        BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }),
    ]);
    add_statement(
        &mut main,
        bb0,
        StatementKind::Assign(
            Place::new(main_agg),
            Rvalue::Aggregate(AggregateKind::Tuple, vec![const_int(7), const_int(9)]),
        ),
    );
    add_statement(
        &mut main,
        bb0,
        StatementKind::Assign(
            Place::new(main_ref),
            Rvalue::Ref(
                Place::new(main_agg),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
        ),
    );
    add_statement(
        &mut main,
        bb0,
        StatementKind::Assign(
            Place::new(main_tmp),
            Rvalue::Use(Operand::Copy(Place::new(main_ref))),
        ),
    );
    set_terminator(
        &mut main,
        bb0,
        TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(FnDefId::from_raw(1), Substitution::empty()),
                ty: ref_ty,
                span: Span::DUMMY,
            }),
            args: vec![Operand::Copy(Place::new(main_ref))],
            destination: Place::new(main_tmp),
            target: Some(bb1),
            cleanup: None,
        },
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    let main_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    interp.add_function(main_id, main);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        next,
    );
    let main_body = interp.function_table.get(&main_id).unwrap().clone();
    let res = interp.run_body(&main_body);
    println!("XREF_PROBE_RESULT={:?}", res);
    assert!(
        res.is_ok(),
        "cross-frame &mut self deref must work: {:?}",
        res
    );
}
