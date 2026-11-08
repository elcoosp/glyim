//! Tests for move analysis and partial moves (Stream V10).
//!
//! Test plan:
//! - V10-T01: Move of whole struct, field access → error
//! - V10-T02: Partial move of one field, other fields still usable
//! - V10-T03: Move of copy type is not a move
//! - V10-T04: Move out of array element → error
//! - V10-T05: Move of field and later reassign → allowed
//! - V10-T06 through V10-T20: Additional coverage

use crate::{BorrowckCtx, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::primitives::Mutability;
use glyim_core::{CrateId, DefId, LocalDefId};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, BorrowKind, LocalDecl, LocalIdx, Operand, Place,
    ProjectionElem, Rvalue, SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::FieldIdx;
use glyim_type::Ty;

/// Mock context where all types are non-Copy by default.
struct MoveMockCtx {
    ty_ctx: glyim_type::TyCtx,
    body: Body,
    copy_types: Vec<Ty>,
}

impl BorrowckCtx for MoveMockCtx {
    fn ty_ctx(&self) -> &glyim_type::TyCtx {
        &self.ty_ctx
    }
    fn local_decl(&self, local: glyim_mir::LocalIdx) -> &glyim_mir::LocalDecl {
        &self.body.locals[local]
    }
    fn is_copy(&self, ty: Ty) -> bool {
        self.copy_types.contains(&ty)
    }
    fn local_name(&self, idx: LocalIdx) -> String {
        format!("_{}", idx.to_raw())
    }
}

fn local_decl(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn local_decl_mut(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
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

/// Create a place with a field projection: `local.field_idx`
fn field_place(local: LocalIdx, field_idx: u32) -> Place {
    Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(field_idx))]),
    }
}

/// Assign by moving a whole local: dest = move src
fn assign_move(dest: LocalIdx, src: LocalIdx) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(dest),
            Rvalue::Use(Operand::Move(Place::new(src))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Assign by moving a field: dest = move `src.field_idx`
fn assign_move_field(dest: LocalIdx, src: LocalIdx, field_idx: u32) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(dest),
            Rvalue::Use(Operand::Move(field_place(src, field_idx))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Assign by copying a whole local: dest = copy src
fn assign_copy(dest: LocalIdx, src: LocalIdx) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(dest),
            Rvalue::Use(Operand::Copy(Place::new(src))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Assign by copying a field: dest = copy `src.field_idx`
fn assign_copy_field(dest: LocalIdx, src: LocalIdx, field_idx: u32) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(dest),
            Rvalue::Use(Operand::Copy(field_place(src, field_idx))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Use a local by copying it into a sink: dest = copy src
fn use_local(dest: LocalIdx, src: LocalIdx) -> Statement {
    assign_copy(dest, src)
}

/// Use a field by copying it into a sink: dest = copy `src.field_idx`
fn use_field(dest: LocalIdx, src: LocalIdx, field_idx: u32) -> Statement {
    assign_copy_field(dest, src, field_idx)
}

// ==========================================================================
// V10-T01: Move of whole struct, then field access → error
// ==========================================================================

#[test]
fn v10_t01_move_whole_struct_then_field_access_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move destination
            local_decl(i32_ty),            // 3: sink for field0
            local_decl(i32_ty),            // 4: spare value
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T01: Expected use-after-move error when accessing field after whole-struct move"
    );
    let has_move_error = result
        .errors
        .iter()
        .any(|e| e.message.contains("moved") || e.message.contains("partially"));
    assert!(
        has_move_error,
        "V10-T01: Expected error about moved value, got: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T02: Partial move of one field, other fields still usable
// ==========================================================================

#[test]
fn v10_t02_partial_move_one_field_other_usable() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: move dest for field0
            local_decl(i32_ty),            // 3: use sink for field1
            local_decl(i32_ty),            // 4: spare value
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 1),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T02: Expected no error: partial move of field 0, field 1 still usable. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T03: Move of copy type is not a move
// ==========================================================================

#[test]
fn v10_t03_move_of_copy_type_not_a_move() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl(i32_ty),            // 1: the copy value
            local_decl(i32_ty),            // 2: move destination
            local_decl(i32_ty),            // 3: use sink
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
            ],
            locals,
        )
    });

    let i32_ty = body.locals[LocalIdx::from_raw(1)].ty;
    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![i32_ty],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T03: Move of Copy type should not produce use-after-move error. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T04: Move out of array element → error
// ==========================================================================

#[test]
fn v10_t04_move_out_of_array_element_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);
        let const_val = glyim_type::Const {
            kind: glyim_type::ConstKind::Uint(3),
            ty: ctx_mut.mk_ty(glyim_type::TyKind::Uint(
                glyim_core::primitives::UintTy::Usize,
            )),
        };
        let array_ty = ctx_mut.mk_ty(glyim_type::TyKind::Array(tuple_ty, const_val));

        let index_local = LocalIdx::from_raw(3);
        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(array_ty),      // 1: the array
            local_decl_mut(tuple_ty),      // 2: move destination
            local_decl(ctx_mut.mk_ty(glyim_type::TyKind::Uint(
                glyim_core::primitives::UintTy::Usize,
            ))), // 3: index
            local_decl(i32_ty),            // 4: sink for use
        ];

        make_body(
            vec![
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::Use(Operand::Move(Place {
                            local: LocalIdx::from_raw(1),
                            projection: Box::new([ProjectionElem::Index(index_local)]),
                        })),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                use_field(LocalIdx::from_raw(4), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T04: Expected error when using array after moving element from it"
    );
}

// ==========================================================================
// V10-T05: Move of field and later reassign → allowed
// ==========================================================================

#[test]
fn v10_t05_move_field_then_reassign_allowed() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct (mutable)
            local_decl(i32_ty),            // 2: move dest for field0
            local_decl(i32_ty),            // 3: use sink
            local_decl(i32_ty),            // 4: spare value
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                Statement {
                    kind: StatementKind::Assign(
                        field_place(LocalIdx::from_raw(1), 0),
                        Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(4)))),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T05: Expected no error: field moved then reassigned, then used. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-Extra: Whole struct move then use whole struct → error
// ==========================================================================

#[test]
fn v10_extra_whole_struct_move_then_use_whole() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move destination
            local_decl_mut(tuple_ty),      // 3: use sink
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                assign_move(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-Extra: Expected use-after-move error for whole struct"
    );
}

// ==========================================================================
// V10-Extra: Partial move, then use whole struct → error (partially moved)
// ==========================================================================

#[test]
fn v10_extra_partial_move_then_use_whole_struct_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: move dest for field0
            local_decl_mut(tuple_ty),      // 3: use sink for whole struct
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                assign_move(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-Extra: Expected error when using whole struct after partial move"
    );
}

// ==========================================================================
// V10-Extra: No move, use field — no error
// ==========================================================================

#[test]
fn v10_extra_no_move_use_field_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: use sink for field0
            local_decl(i32_ty),            // 3: use sink for field1
        ];

        make_body(
            vec![
                use_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 1),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-Extra: No moves, just copies — no error expected. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-Extra: Reassign whole struct after partial move, then use — OK
// ==========================================================================

#[test]
fn v10_extra_reassign_whole_after_partial_move_then_use() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct (mutable)
            local_decl(i32_ty),            // 2: move dest for field0
            local_decl(i32_ty),            // 3: use sink for field0
            local_decl_mut(tuple_ty),      // 4: spare struct for reinit
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(4)))),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-Extra: Reassign whole struct after partial move, then use — OK. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T06: Cross-block move — move in one block, use in successor → error
// ==========================================================================

#[test]
fn v10_t06_cross_block_move_then_use_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move dest
            local_decl(i32_ty),            // 3: use sink for field
        ];

        let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
        body.locals = IndexVec::from_raw(locals);

        let mut bb0 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb0.statements = vec![assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1))];

        let mut bb1 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb1.statements = vec![use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0)];

        body.basic_blocks = IndexVec::from_raw(vec![bb0, bb1]);
        body
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T06: Expected use-after-move error across blocks"
    );
}

// ==========================================================================
// V10-T07: Cross-block move — move in one block, no use → no error
// ==========================================================================

#[test]
fn v10_t07_cross_block_move_no_use_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move dest
        ];

        let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
        body.locals = IndexVec::from_raw(locals);

        let mut bb0 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb0.statements = vec![assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1))];

        let bb1 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });

        body.basic_blocks = IndexVec::from_raw(vec![bb0, bb1]);
        body
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T07: Move across blocks with no subsequent use — no error. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T08: Multiple partial moves of different fields
// ==========================================================================

#[test]
fn v10_t08_multiple_partial_moves_different_fields() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: dest for field0
            local_decl(i32_ty),            // 3: dest for field1
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                assign_move_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 1),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T08: Multiple partial moves of different fields — no error. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T09: Multiple partial moves then use whole struct → error
// ==========================================================================

#[test]
fn v10_t09_multiple_partial_moves_then_use_whole_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: dest for field0
            local_decl(i32_ty),            // 3: dest for field1
            local_decl_mut(tuple_ty),      // 4: use sink for whole
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                assign_move_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 1),
                assign_move(LocalIdx::from_raw(4), LocalIdx::from_raw(1)),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T09: Expected error when using whole struct after moving all fields"
    );
}

// ==========================================================================
// V10-T10: Reassign only moved field, then use it → OK
// ==========================================================================

#[test]
fn v10_t10_reassign_only_moved_field_then_use_ok() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct (mutable)
            local_decl(i32_ty),            // 2: dest for field0 move
            local_decl(i32_ty),            // 3: spare value for reinit
            local_decl(i32_ty),            // 4: use sink for field0
            local_decl(i32_ty),            // 5: use sink for field1
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                Statement {
                    kind: StatementKind::Assign(
                        field_place(LocalIdx::from_raw(1), 0),
                        Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(3)))),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                use_field(LocalIdx::from_raw(4), LocalIdx::from_raw(1), 0),
                use_field(LocalIdx::from_raw(5), LocalIdx::from_raw(1), 1),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T10: Reassign only moved field, then use — OK. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T11: Use after move via binary op → error
// ==========================================================================

#[test]
fn v10_t11_use_after_move_in_binary_op_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move dest
            local_decl(i32_ty),            // 3: other operand
            local_decl(i32_ty),            // 4: result sink
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(4)),
                        Rvalue::BinaryOp(
                            glyim_core::primitives::BinOp::Add,
                            Box::new((
                                Operand::Copy(field_place(LocalIdx::from_raw(1), 0)),
                                Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                            )),
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T11: Expected use-after-move error in binary op"
    );
}

// ==========================================================================
// V10-T12: Use after move via aggregate → error
// ==========================================================================

#[test]
fn v10_t12_use_after_move_in_aggregate_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move dest
            local_decl(i32_ty),            // 3: other value
            local_decl(tuple_ty),          // 4: aggregate result
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(4)),
                        Rvalue::Aggregate(
                            glyim_mir::AggregateKind::Tuple,
                            vec![
                                Operand::Copy(field_place(LocalIdx::from_raw(1), 0)),
                                Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                            ],
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T12: Expected use-after-move error in aggregate"
    );
}

// ==========================================================================
// V10-T13: Move then StorageDead — no use-after-move error
// ==========================================================================

#[test]
fn v10_t13_move_then_storage_dead_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move dest
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                Statement {
                    kind: StatementKind::StorageDead(LocalIdx::from_raw(1)),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T13: Move then StorageDead — no use-after-move. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T14: Partial move in one block, use other field in successor → OK
// ==========================================================================

#[test]
fn v10_t14_partial_move_in_one_branch_use_other_field_ok() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: dest for field0 move
            local_decl(i32_ty),            // 3: use sink for field1
        ];

        let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
        body.locals = IndexVec::from_raw(locals);

        let mut bb0 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Goto {
                target: BasicBlockIdx::from_raw(1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb0.statements = vec![assign_move_field(
            LocalIdx::from_raw(2),
            LocalIdx::from_raw(1),
            0,
        )];

        let mut bb1 = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        bb1.statements = vec![use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 1)];

        body.basic_blocks = IndexVec::from_raw(vec![bb0, bb1]);
        body
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T14: Partial move field 0, use field 1 in successor block — OK. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T15: Move field of Copy type — no actual move
// ==========================================================================

#[test]
fn v10_t15_move_field_of_copy_type_no_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: dest for field0 move
            local_decl(i32_ty),            // 3: use sink for field0 after move
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let i32_ty = body.locals[LocalIdx::from_raw(2)].ty;
    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![i32_ty],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T15: Move field of Copy type — no move occurs. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T16: Nested tuple — partial move of inner field
// ==========================================================================

#[test]
fn v10_t16_nested_tuple_move_inner_field() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let inner_tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let inner_tuple_ty = ctx_mut.mk_tuple(inner_tuple_substs);
        let outer_tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(inner_tuple_ty),
        ]);
        let outer_tuple_ty = ctx_mut.mk_tuple(outer_tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()),  // 0: return
            local_decl_mut(outer_tuple_ty), // 1: the outer struct
            local_decl_mut(inner_tuple_ty), // 2: dest for inner tuple move
            local_decl(i32_ty),             // 3: use sink for field0 of outer
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 1),
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T16: Partial move of inner tuple field, other field usable — OK. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T17: Use after move via unary op → error
// ==========================================================================

#[test]
fn v10_t17_use_after_move_in_unary_op_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move dest
            local_decl(i32_ty),            // 3: result sink
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(3)),
                        Rvalue::UnaryOp(
                            glyim_core::primitives::UnOp::Neg,
                            Operand::Copy(field_place(LocalIdx::from_raw(1), 0)),
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T17: Expected use-after-move error in unary op"
    );
}

// ==========================================================================
// V10-T18: StorageLive reinitializes local after move
// ==========================================================================

#[test]
fn v10_t18_storage_live_reinitializes_after_move() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl_mut(tuple_ty),      // 2: move dest
            local_decl(i32_ty),            // 3: use sink
        ];

        make_body(
            vec![
                assign_move(LocalIdx::from_raw(2), LocalIdx::from_raw(1)),
                Statement {
                    kind: StatementKind::StorageLive(LocalIdx::from_raw(1)),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                use_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T18: StorageLive reinitializes after move — OK. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T19: Copy operand doesn't move — no error
// ==========================================================================

#[test]
fn v10_t19_copy_operand_no_move_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: copy dest
            local_decl(i32_ty),            // 3: second use
        ];

        make_body(
            vec![
                assign_copy_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                assign_copy_field(LocalIdx::from_raw(3), LocalIdx::from_raw(1), 0),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        result.errors.is_empty(),
        "V10-T19: Operand::Copy doesn't move — no error. Errors: {:?}",
        result.errors
    );
}

// ==========================================================================
// V10-T20: Partial move then borrow whole struct → error
// ==========================================================================

#[test]
fn v10_t20_partial_move_then_borrow_whole_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
        let tuple_substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let tuple_ty = ctx_mut.mk_tuple(tuple_substs);
        let ref_ty = ctx_mut.mk_ref(glyim_type::Region::Erased, tuple_ty, Mutability::Not);

        let locals = vec![
            local_decl(ctx_mut.unit_ty()), // 0: return
            local_decl_mut(tuple_ty),      // 1: the struct
            local_decl(i32_ty),            // 2: dest for field0 move
            local_decl(ref_ty),            // 3: ref dest
            local_decl(i32_ty),            // 4: use sink for ref
        ];

        make_body(
            vec![
                assign_move_field(LocalIdx::from_raw(2), LocalIdx::from_raw(1), 0),
                Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(3)),
                        Rvalue::Ref(Place::new(LocalIdx::from_raw(1)), BorrowKind::Shared),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            ],
            locals,
        )
    });

    let mock = MoveMockCtx {
        ty_ctx: ctx,
        body,
        copy_types: vec![],
    };
    let result = check_borrows(&mock, &mock.body);
    assert!(
        !result.errors.is_empty(),
        "V10-T20: Expected error when borrowing whole struct after partial move"
    );
}
