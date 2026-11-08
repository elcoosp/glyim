use crate::{BorrowckCtx, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::primitives::Mutability;
use glyim_core::{CrateId, DefId, LocalDefId};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, BorrowKind, LocalDecl, LocalIdx, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::Ty;

struct LocalMockBorrowckCtx {
    ty_ctx: glyim_type::TyCtx,
    body: Body,
}

impl BorrowckCtx for LocalMockBorrowckCtx {
    fn ty_ctx(&self) -> &glyim_type::TyCtx {
        &self.ty_ctx
    }
    fn local_decl(&self, local: glyim_mir::LocalIdx) -> &glyim_mir::LocalDecl {
        &self.body.locals[local]
    }
    fn is_copy(&self, _ty: Ty) -> bool {
        false
    }
    fn local_name(&self, idx: LocalIdx) -> String {
        format!("_{}", idx.to_raw())
    }
}

fn make_body(statements: Vec<Statement>, locals: Vec<LocalDecl>) -> Body {
    let def_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut body = Body::dummy(def_id);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    })]);
    body.basic_blocks[BasicBlockIdx::from_raw(0)].statements = statements;
    body.locals = IndexVec::from_raw(locals);
    body
}

fn local_decl(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn assign_borrow(dest: LocalIdx, place: Place, kind: BorrowKind) -> Statement {
    Statement {
        kind: StatementKind::Assign(Place::new(dest), Rvalue::Ref(place, kind)),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn use_local(dest: LocalIdx, local: LocalIdx) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(dest),
            Rvalue::Use(Operand::Copy(Place::new(local))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn make_ref_ty(ctx_mut: &mut glyim_type::TyCtxMut, inner: Ty, mutable: bool) -> Ty {
    let mutability = if mutable {
        Mutability::Mut
    } else {
        Mutability::Not
    };
    ctx_mut.mk_ref(glyim_type::Region::Erased, inner, mutability)
}

#[test]
fn t01_no_borrows_no_errors() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let locals = vec![local_decl(unit)];
        make_body(vec![], locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty());
}

#[test]
fn t02_two_shared_borrows_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref1 = make_ref_ty(ctx_mut, unit, false);
        let shared_ref2 = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![
            local_decl(unit),
            local_decl(shared_ref1),
            local_decl(shared_ref2),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty());
}

#[test]
fn t03_shared_and_mutable_conflict_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let mut_ref = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(shared_ref),
            local_decl(mut_ref),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(!result.errors.is_empty(), "Expected borrow conflict error");
}

#[test]
fn t04_two_mutable_borrows_conflict_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref1 = make_ref_ty(ctx_mut, unit, true);
        let mut_ref2 = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(mut_ref1),
            local_decl(mut_ref2),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(!result.errors.is_empty(), "Expected borrow conflict error");
}

#[test]
fn t05_borrow_expires_after_last_use_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref1 = make_ref_ty(ctx_mut, unit, true);
        let mut_ref2 = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(mut_ref1),
            local_decl(unit),
            local_decl(mut_ref2),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                assign_borrow(
                    LocalIdx::from_raw(3),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Expected no error; borrow expired"
    );
}

#[test]
fn t07_error_diagnostics_include_span() {
    let span1 = Span::new(
        glyim_span::FileId::BOGUS,
        glyim_span::ByteIdx::from_raw(0),
        glyim_span::ByteIdx::from_raw(5),
        glyim_span::SyntaxContext::ROOT,
    );
    let span2 = Span::new(
        glyim_span::FileId::BOGUS,
        glyim_span::ByteIdx::from_raw(10),
        glyim_span::ByteIdx::from_raw(15),
        glyim_span::SyntaxContext::ROOT,
    );
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref1 = make_ref_ty(ctx_mut, unit, true);
        let mut_ref2 = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(mut_ref1),
            local_decl(mut_ref2),
            local_decl(unit),
        ];
        make_body(
            vec![
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Ref(
                            Place::new(LocalIdx::from_raw(0)),
                            BorrowKind::Mut {
                                allow_two_phase_borrow: false,
                            },
                        ),
                    ),
                    source_info: SourceInfo::new(span1),
                },
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Ref(
                            Place::new(LocalIdx::from_raw(0)),
                            BorrowKind::Mut {
                                allow_two_phase_borrow: false,
                            },
                        ),
                    ),
                    source_info: SourceInfo::new(span2),
                },
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(!result.errors.is_empty());
    let has_span = result.errors.iter().any(|d| d.span.primary != Span::DUMMY);
    assert!(
        has_span,
        "Expected at least one error with a non-dummy span"
    );
}

#[test]
fn t08_local_ty_via_borrowck_ctx() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let locals = vec![local_decl(unit)];
        make_body(vec![], locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let decl = mock.local_decl(LocalIdx::from_raw(0));
    assert_eq!(decl.ty, Ty::UNIT);
}

#[test]
fn t09_multiple_conflicts_in_one_body() {
    // With NLL, borrows only live until their last use. This test ensures
    // that overlapping active borrows are detected, while dead borrows
    // (whose dest local is overwritten before being read) are correctly
    // excluded from conflict checking.
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref_ty = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),       // 0
            local_decl(unit),       // 1
            local_decl(mut_ref_ty), // 2
            local_decl(mut_ref_ty), // 3
            local_decl(unit),       // 4
            local_decl(unit),       // 5
        ];
        make_body(
            vec![
                // _2 = &mut _0  (first mutable borrow of _0)
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                // _3 = &mut _0  (second mutable borrow of _0 — CONFLICTS
                // because _2 is still live: it's used at stmt 2)
                assign_borrow(
                    LocalIdx::from_raw(3),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                // Use both borrows to keep them live at stmt 1
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
                use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "Expected at least one borrow conflict: two active mutable borrows of same local"
    );
}
#[test]
fn t10_shared_borrow_after_mutable_expires_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref = make_ref_ty(ctx_mut, unit, true);
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![
            local_decl(unit),
            local_decl(mut_ref),
            local_decl(unit),
            local_decl(shared_ref),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                assign_borrow(
                    LocalIdx::from_raw(3),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Expected no error: mutable borrow expired before shared"
    );
}

#[test]
fn t11_mutable_borrow_after_shared_expires_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let mut_ref = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(shared_ref),
            local_decl(unit),
            local_decl(mut_ref),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                use_local(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                assign_borrow(
                    LocalIdx::from_raw(3),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Expected no error: shared borrow expired before mutable"
    );
}

#[test]
fn t12_borrow_of_different_locals_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref1 = make_ref_ty(ctx_mut, unit, true);
        let mut_ref2 = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(unit),
            local_decl(mut_ref1),
            local_decl(mut_ref2),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                assign_borrow(
                    LocalIdx::from_raw(3),
                    Place::new(LocalIdx::from_raw(1)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Expected no error: borrows of different locals"
    );
}

#[test]
fn t13_error_message_contains_borrow_kind_info() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref1 = make_ref_ty(ctx_mut, unit, true);
        let mut_ref2 = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(mut_ref1),
            local_decl(mut_ref2),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(!result.errors.is_empty());
    let msg = &result.errors[0].message;
    assert!(
        msg.contains("mutable"),
        "Error message should mention 'mutable': {msg}"
    );
    assert!(
        msg.contains("borrow"),
        "Error message should mention 'borrow': {msg}"
    );
}

#[test]
fn t15_no_borrow_check_for_copy_types_ignored() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let shared_ref = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(i32_ty),
            local_decl(shared_ref),
            local_decl(i32_ty),
            local_decl(shared_ref),
            local_decl(i32_ty),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                use_local(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                assign_borrow(
                    LocalIdx::from_raw(3),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty());
}

#[test]
fn t16_empty_body_no_panics() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        make_body(vec![], vec![local_decl(unit)])
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty());
}

#[test]
fn t17_borrow_dest_never_used_expires_immediately() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref1 = make_ref_ty(ctx_mut, unit, true);
        let mut_ref2 = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![local_decl(unit), local_decl(mut_ref1), local_decl(mut_ref2)];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Borrows never used expire immediately, no conflict"
    );
}

#[test]
fn t18_is_copy_delegates_to_ctx() {
    struct CopyMockCtx {
        ty_ctx: glyim_type::TyCtx,
        body: Body,
    }
    impl BorrowckCtx for CopyMockCtx {
        fn ty_ctx(&self) -> &glyim_type::TyCtx {
            &self.ty_ctx
        }
        fn local_decl(&self, local: glyim_mir::LocalIdx) -> &glyim_mir::LocalDecl {
            &self.body.locals[local]
        }
        fn is_copy(&self, _ty: Ty) -> bool {
            true
        }
        fn local_name(&self, idx: LocalIdx) -> String {
            format!("_{}", idx.to_raw())
        }
    }

    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        make_body(vec![], vec![local_decl(unit)])
    });
    let mock = CopyMockCtx { ty_ctx: ctx, body };
    assert!(mock.is_copy(Ty::UNIT));
}

#[test]
fn t19_unique_vs_shared_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let unique_ref = make_ref_ty(ctx_mut, unit, true);
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![
            local_decl(unit),
            local_decl(unique_ref),
            local_decl(shared_ref),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Unique,
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "Unique and shared borrow conflict"
    );
}

#[test]
fn t20_unique_vs_mutable_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let unique_ref = make_ref_ty(ctx_mut, unit, true);
        let mut_ref = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(unique_ref),
            local_decl(mut_ref),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Unique,
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "Unique and mutable borrow conflict"
    );
}

#[test]
fn t21_unique_vs_unique_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let unique1 = make_ref_ty(ctx_mut, unit, true);
        let unique2 = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(unique1),
            local_decl(unique2),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Unique,
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Unique,
                ),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(!result.errors.is_empty(), "Two unique borrows conflict");
}

#[test]
fn t22_shared_after_unique_expires_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let unique_ref = make_ref_ty(ctx_mut, unit, true);
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![
            local_decl(unit),
            local_decl(unique_ref),
            local_decl(unit),
            local_decl(shared_ref),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Unique,
                ),
                use_local(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                assign_borrow(
                    LocalIdx::from_raw(3),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Unique borrow expired before shared borrow"
    );
}

#[test]
fn t23_multi_block_borrows_across_blocks_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref_ty = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(mut_ref_ty),
            local_decl(unit),
            local_decl(mut_ref_ty),
            local_decl(unit),
        ];
        let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
        body.locals = IndexVec::from_raw(locals);

        let mut bb0 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb0.statements = vec![
            assign_borrow(
                LocalIdx::from_raw(1),
                Place::new(LocalIdx::from_raw(0)),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
            use_local(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
        ];

        let mut bb1 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb1.statements = vec![
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(0)),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
        ];

        body.basic_blocks = IndexVec::from_raw(vec![bb0, bb1]);
        body
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Multi-block: no overlap, no conflict"
    );
}

#[test]
fn t24_multi_block_overlap_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref_ty = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![
            local_decl(unit),
            local_decl(mut_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(unit),
        ];
        let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
        body.locals = IndexVec::from_raw(locals);

        let mut bb0 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb0.statements = vec![assign_borrow(
            LocalIdx::from_raw(1),
            Place::new(LocalIdx::from_raw(0)),
            BorrowKind::Mut {
                allow_two_phase_borrow: false,
            },
        )];

        let mut bb1 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        // Second borrow happens before use of first borrow, so they overlap.
        bb1.statements = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(0)),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ),
            use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
        ];

        body.basic_blocks = IndexVec::from_raw(vec![bb0, bb1]);
        body
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "Multi-block overlapping mutable borrows should conflict"
    );
}

#[test]
fn t25_borrow_of_reference_type_locals() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let mut_ref_of_ref = make_ref_ty(ctx_mut, ref_ty, true);
        let locals = vec![
            local_decl(ref_ty),
            local_decl(mut_ref_of_ref),
            local_decl(i32_ty),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Borrow of a reference local should be tracked and expire normally"
    );
}

#[test]
fn t26_many_locals_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let count = 10;
        let locals = vec![local_decl(unit); count * 2 + 1];
        let mut stmts = Vec::new();
        for i in 0..count {
            let dest = LocalIdx::from_raw((i * 2 + 1) as u32);
            let src = LocalIdx::from_raw((i) as u32);
            stmts.push(assign_borrow(
                dest,
                Place::new(src),
                BorrowKind::Mut {
                    allow_two_phase_borrow: false,
                },
            ));
            stmts.push(use_local(LocalIdx::from_raw((i * 2 + 2) as u32), dest));
        }
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Borrows of distinct locals should not conflict"
    );
}

#[test]
fn t27_empty_second_block_no_panic() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let _ = ctx_mut.unit_ty();
        let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
        body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT)]);
        let bb0 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let bb1 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        body.basic_blocks = IndexVec::from_raw(vec![bb0, bb1]);
        body
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty());
}

#[test]
fn t28_binary_op_with_borrowed_operands_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let _ = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let shared_ref = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(i32_ty),
            local_decl(shared_ref),
            local_decl(shared_ref),
            local_decl(i32_ty),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(3)),
                        Rvalue::BinaryOp(
                            glyim_core::primitives::BinOp::Add,
                            Box::new((
                                Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                                Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                            )),
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Two shared borrows used in binary op: no conflict"
    );
}

#[test]
fn t29_aggregate_with_borrowed_operands() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref1 = make_ref_ty(ctx_mut, unit, false);
        let shared_ref2 = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![
            local_decl(unit),
            local_decl(shared_ref1),
            local_decl(shared_ref2),
            local_decl(unit),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                assign_borrow(
                    LocalIdx::from_raw(2),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(3)),
                        Rvalue::Aggregate(
                            glyim_mir::AggregateKind::Tuple,
                            vec![
                                Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                                Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                            ],
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "Shared borrows used in aggregate: no conflict"
    );
}

#[test]
fn t30_discriminant_read_extends_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![local_decl(unit), local_decl(shared_ref), local_decl(unit)];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Discriminant(Place::new(LocalIdx::from_raw(1))),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty(), "Discriminant read: no conflict");
}

#[test]
fn t31_len_extends_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![local_decl(unit), local_decl(shared_ref), local_decl(unit)];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Len(Place::new(LocalIdx::from_raw(1))),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty(), "Len read: no conflict");
}

#[test]
fn t32_cast_extends_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let i64_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I64));
        let shared_ref = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(i32_ty),
            local_decl(shared_ref),
            local_decl(i64_ty),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Cast(
                            glyim_mir::CastKind::IntToInt,
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                            i64_ty,
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty(), "Cast read: no conflict");
}

#[test]
fn t33_repeat_extends_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let const_val = glyim_type::Const {
            kind: glyim_type::ConstKind::Uint(3),
            ty: ctx_mut.mk_ty(glyim_type::TyKind::Uint(
                glyim_core::primitives::UintTy::U32,
            )),
        };
        let array_ty = ctx_mut.mk_ty(glyim_type::TyKind::Array(unit, const_val));
        let locals = vec![
            local_decl(unit),
            local_decl(shared_ref),
            local_decl(array_ty),
        ];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Repeat(
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                            glyim_mir::MirConst {
                                kind: glyim_mir::MirConstKind::Uint(3),
                                ty: ctx_mut.mk_ty(glyim_type::TyKind::Uint(
                                    glyim_core::primitives::UintTy::U32,
                                )),
                                span: Span::DUMMY,
                            },
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty(), "Repeat read: no conflict");
}

#[test]
fn t34_operand_move_read_extends_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let shared_ref = make_ref_ty(ctx_mut, unit, false);
        let locals = vec![local_decl(unit), local_decl(shared_ref), local_decl(unit)];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Shared,
                ),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_raw(1)))),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty(), "Move operand: no conflict");
}

#[test]
fn t35_storage_dead_does_not_crash() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let mut_ref = make_ref_ty(ctx_mut, unit, true);
        let locals = vec![local_decl(unit), local_decl(mut_ref)];
        make_body(
            vec![
                assign_borrow(
                    LocalIdx::from_raw(1),
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
                Statement {
                    kind: StatementKind::StorageDead(LocalIdx::from_raw(1)),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "StorageDead should not cause panic"
    );
}

#[test]
fn t36_nop_ignored() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let locals = vec![local_decl(unit)];
        make_body(
            vec![Statement {
                kind: StatementKind::Nop,
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty(), "Nop statement should be ignored");
}

#[test]
fn t37_constant_operand_no_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let locals = vec![local_decl(i32_ty), local_decl(i32_ty)];
        make_body(
            vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(glyim_mir::MirConst {
                        kind: glyim_mir::MirConstKind::Int(42),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            locals,
        )
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert!(result.errors.is_empty());
}
