//! Tests for Stream U-Borrowck: unstubbing move analysis, two-phase borrow
//! cross-block tracking, and projection conflicts.

use crate::{BorrowckCtx, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_core::{AdtId, UintTy};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, BorrowKind, LocalDecl, LocalIdx, Operand, Place,
    ProjectionElem, Rvalue, SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_type::{Const, ConstKind, FieldIdx, Ty, TyCtx, TyKind};

// ---------------------------------------------------------------------------
// Test context
// ---------------------------------------------------------------------------

struct TestCtx {
    ty_ctx: TyCtx,
    locals: IndexVec<LocalIdx, LocalDecl>,
    names: Vec<String>,
}

impl TestCtx {
    fn new(ty_ctx: TyCtx) -> Self {
        Self {
            ty_ctx,
            locals: IndexVec::new(),
            names: Vec::new(),
        }
    }

    fn add_local(&mut self, ty: Ty, name: &str) -> LocalIdx {
        let idx = self.locals.push(LocalDecl {
            ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        self.names.push(name.to_string());
        idx
    }
}

impl BorrowckCtx for TestCtx {
    fn ty_ctx(&self) -> &TyCtx {
        &self.ty_ctx
    }

    fn local_decl(&self, local: LocalIdx) -> &LocalDecl {
        &self.locals[local]
    }

    fn local_name(&self, local: LocalIdx) -> String {
        self.names
            .get(local.to_raw() as usize)
            .cloned()
            .unwrap_or_else(|| format!("_{}", local.to_raw()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_body() -> Body {
    Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        basic_blocks: IndexVec::new(),
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    }
}

fn add_local_to_both(ctx: &mut TestCtx, body: &mut Body, ty: Ty, name: &str) -> LocalIdx {
    let ctx_idx = ctx.add_local(ty, name);
    let body_idx = body.locals.push(LocalDecl {
        ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    debug_assert_eq!(ctx_idx, body_idx, "local indices must match");
    ctx_idx
}

fn dummy_source_info() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

fn dummy_terminator() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: dummy_source_info(),
    }
}

// ---------------------------------------------------------------------------
// Test 1: places_conflict — Field vs Index are disjoint
// ---------------------------------------------------------------------------

#[test]
fn test_field_vs_index_disjoint() {
    let local = LocalIdx::from_usize(0);
    let field_place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
    };
    let index_place = Place {
        local,
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_usize(1))]),
    };

    assert!(
        !crate::visitor::places_conflict(&field_place, &index_place),
        "Field and Index on the same local should not conflict"
    );
    assert!(
        !crate::visitor::places_conflict(&index_place, &field_place),
        "Index and Field on the same local should not conflict"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Same field projections conflict
// ---------------------------------------------------------------------------

#[test]
fn test_same_field_conflicts() {
    let local = LocalIdx::from_usize(0);
    let place_a = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
    };
    let place_b = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
    };

    assert!(
        crate::visitor::places_conflict(&place_a, &place_b),
        "Same field on same local should conflict"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Different fields don't conflict
// ---------------------------------------------------------------------------

#[test]
fn test_different_fields_disjoint() {
    let local = LocalIdx::from_usize(0);
    let place_a = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
    };
    let place_b = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(1))]),
    };

    assert!(
        !crate::visitor::places_conflict(&place_a, &place_b),
        "Different fields on same local should not conflict"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Cross-block reservation extends to successor blocks
// ---------------------------------------------------------------------------

#[test]
fn test_cross_block_reservation_extends() {
    let mut body = empty_body();

    let local_data = body.locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });
    let local_ref = body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    let block0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(local_ref),
                Rvalue::Ref(
                    Place::new(local_data),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: true,
                    },
                ),
            ),
            source_info: dummy_source_info(),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_usize(1),
            },
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    };

    let block1 = BasicBlockData {
        statements: vec![],
        terminator: dummy_terminator(),
        is_cleanup: false,
    };

    body.basic_blocks.push(block0);
    body.basic_blocks.push(block1);

    let analysis = crate::twophase::ReservationAnalysis::compute(
        &body,
        BasicBlockIdx::from_usize(0),
        0,
        local_ref,
    );

    assert!(
        analysis.is_reservation(BasicBlockIdx::from_usize(1), 0),
        "Reservation should extend to successor block"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Cross-block reservation ends when dest_local is read
// ---------------------------------------------------------------------------

#[test]
fn test_cross_block_reservation_ends_on_activation() {
    let mut body = empty_body();

    let local_data = body.locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });
    let local_ref = body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });
    let local_tmp = body.locals.push(LocalDecl {
        ty: Ty::BOOL,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    let block0 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(local_ref),
                Rvalue::Ref(
                    Place::new(local_data),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: true,
                    },
                ),
            ),
            source_info: dummy_source_info(),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_usize(1),
            },
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    };

    let block1 = BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local_tmp),
                    Rvalue::Use(Operand::Copy(Place::new(local_ref))),
                ),
                source_info: dummy_source_info(),
            },
            Statement {
                kind: StatementKind::Nop,
                source_info: dummy_source_info(),
            },
        ],
        terminator: dummy_terminator(),
        is_cleanup: false,
    };

    body.basic_blocks.push(block0);
    body.basic_blocks.push(block1);

    let analysis = crate::twophase::ReservationAnalysis::compute(
        &body,
        BasicBlockIdx::from_usize(0),
        0,
        local_ref,
    );

    assert!(
        analysis.is_reservation(BasicBlockIdx::from_usize(1), 0),
        "Reservation should be active before activation read"
    );
    assert!(
        !analysis.is_reservation(BasicBlockIdx::from_usize(1), 1),
        "Reservation should end after activation read"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Move analysis correctly tracks ADT field count
// ---------------------------------------------------------------------------

#[test]
fn test_move_analysis_adt_field_count() {
    use glyim_test::test_ty_ctx;

    let mut ctx_mut = test_ty_ctx();
    let adt_id = AdtId::from_usize(0);
    let field_tys = vec![Ty::BOOL, Ty::BOOL, Ty::BOOL];
    ctx_mut.register_adt_repr(adt_id, field_tys);
    let substs = ctx_mut.intern_substitution(vec![]);
    let ty = ctx_mut.mk_adt(adt_id, substs);
    let frozen_ctx = ctx_mut.freeze();

    let mut ctx = TestCtx::new(frozen_ctx);
    let mut body = empty_body();
    let local_data = add_local_to_both(&mut ctx, &mut body, ty, "data");
    let local_ref = add_local_to_both(&mut ctx, &mut body, Ty::ERROR, "ref");

    let block0 = BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local_ref),
                    Rvalue::Ref(
                        Place {
                            local: local_data,
                            projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
                        },
                        BorrowKind::Mut {
                            allow_two_phase_borrow: false,
                        },
                    ),
                ),
                source_info: dummy_source_info(),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local_ref),
                    Rvalue::Ref(
                        Place {
                            local: local_data,
                            projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(1))]),
                        },
                        BorrowKind::Mut {
                            allow_two_phase_borrow: false,
                        },
                    ),
                ),
                source_info: dummy_source_info(),
            },
        ],
        terminator: dummy_terminator(),
        is_cleanup: false,
    };
    body.basic_blocks.push(block0);

    let result = check_borrows(&ctx, &body);
    assert!(
        result.errors.is_empty(),
        "Borrowing disjoint fields of an ADT should not conflict. Errors: {:?}",
        result.errors
    );
}

// ---------------------------------------------------------------------------
// Test 7: Index projection returns root move path
// ---------------------------------------------------------------------------

#[test]
fn test_index_projection_returns_root() {
    use glyim_test::test_ty_ctx;

    let mut ctx_mut = test_ty_ctx();
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let string_ty = ctx_mut.mk_ty(TyKind::String);
    let array_ty = ctx_mut.mk_ty(TyKind::Array(
        string_ty,
        Const {
            kind: ConstKind::Uint(1),
            ty: u32_ty,
        },
    ));
    let frozen_ctx = ctx_mut.freeze();

    let mut ctx = TestCtx::new(frozen_ctx);
    let mut body = empty_body();
    let local_arr = add_local_to_both(&mut ctx, &mut body, array_ty, "arr");
    let local_tmp = add_local_to_both(&mut ctx, &mut body, string_ty, "tmp");

    let block0 = BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local_tmp),
                    Rvalue::Use(Operand::Move(Place {
                        local: local_arr,
                        projection: Box::new([ProjectionElem::Index(local_tmp)]),
                    })),
                ),
                source_info: dummy_source_info(),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(local_tmp),
                    Rvalue::Use(Operand::Move(Place::new(local_arr))),
                ),
                source_info: dummy_source_info(),
            },
        ],
        terminator: dummy_terminator(),
        is_cleanup: false,
    };
    body.basic_blocks.push(block0);

    let result = check_borrows(&ctx, &body);
    assert!(
        !result.errors.is_empty(),
        "Moving an array element should move the whole array, causing a use-after-move error."
    );
}
