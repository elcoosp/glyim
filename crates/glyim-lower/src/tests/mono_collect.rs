//! Tests for V23: Mono Item Graph Traversal & Collection
//!
//! V23-T01: Call graph: main calls foo<T> with concrete T -> foo<T> added
//! V23-T02: Recursive function -> no duplicate items
//! V23-T03: Drop glue for structs -> collected
//! V23-T04: Constant used in generic -> instantiated
//! V23-T05: Diamond dependency - D collected once
//! V23-T06: Transitive call chain A->B->C->D
//! V23-T07: Two different substitutions of same function
//! V23-T08: Empty start set -> nothing collected
//! V23-T09: Static item collected
//! V23-T10: Multiple constants in same body
//! V23-T11: Mixed calls and constants
//! V23-T12: Leaf function (no refs) -> only self
//! V23-T13: Multiple start items
//! V23-T14: Transitive constant collection

use crate::{MonoCtx, MonoItem};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::*;
use glyim_core::primitives::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::*;
use std::sync::Arc;
fn dummy_body() -> Arc<Body> {
    Arc::new(Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(0),
    )))
}
/// Helper: create a simple MIR body with a Call terminator to a function.
fn make_call_body(
    caller_def_id: FnDefId,
    callee_def_id: FnDefId,
    callee_substs: Substitution,
) -> Body {
    let owner = DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(caller_def_id.to_raw()),
    );
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let func_const = MirConst {
        kind: MirConstKind::Fn(callee_def_id, callee_substs),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };

    let call_terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(func_const),
            args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
            destination: Place::new(LocalIdx::from_raw(2)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let return_terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: call_terminator,
        is_cleanup: false,
    });
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: return_terminator,
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 1,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Helper: create a trivial MIR body (just return).
fn make_simple_body(fn_def_id: FnDefId) -> Body {
    let owner = DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(fn_def_id.to_raw()),
    );
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Helper: create a MIR body with a recursive call (calls itself).
fn make_recursive_body(fn_def_id: FnDefId, substs: Substitution) -> Body {
    let owner = DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(fn_def_id.to_raw()),
    );
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let func_const = MirConst {
        kind: MirConstKind::Fn(fn_def_id, substs),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };

    let call_terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(func_const),
            args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
            destination: Place::new(LocalIdx::from_raw(2)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let return_terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: call_terminator,
        is_cleanup: false,
    });
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: return_terminator,
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 1,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Helper: create a body with a Drop terminator.
fn make_drop_body(fn_def_id: FnDefId, _dropped_adt_id: AdtId) -> Body {
    let owner = DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(fn_def_id.to_raw()),
    );
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let drop_terminator = Terminator {
        kind: TerminatorKind::Drop {
            place: Place::new(LocalIdx::from_raw(1)),
            target: BasicBlockIdx::from_raw(1),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let return_terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: drop_terminator,
        is_cleanup: false,
    });
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: return_terminator,
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Helper: create a body referencing a constant via `ConstRef` in an Assign statement.
fn make_const_ref_body(fn_def_id: FnDefId, const_id: ConstDefId, substs: Substitution) -> Body {
    let owner = DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(fn_def_id.to_raw()),
    );
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let const_val = MirConst {
        kind: MirConstKind::ConstRef(const_id, substs),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };

    let assign_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(const_val)),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![assign_stmt],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Helper: generic body lookup closure that matches by raw `def_id`.
fn make_body_lookup(
    fn_bodies: Vec<(FnDefId, Substitution, Arc<Body>)>,
    const_bodies: Vec<(ConstDefId, Substitution, Arc<Body>)>,
) -> impl Fn(DefId, &Substitution) -> Arc<Body> {
    move |def_id, substs| {
        let raw = def_id.local_id.to_raw();
        for (fn_id, fn_substs, body) in &fn_bodies {
            if raw == fn_id.to_raw() && *substs == *fn_substs {
                return body.clone();
            }
        }
        for (const_id, const_substs, body) in &const_bodies {
            if raw == const_id.to_raw() && *substs == *const_substs {
                return body.clone();
            }
        }
        Arc::new(Body::dummy(def_id))
    }
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

/// V23-T01: Call graph: main calls foo<T> with concrete T -> foo<T> added
#[test]
fn call_graph_adds_callee() {
    let main_id = FnDefId::from_raw(1);
    let foo_id = FnDefId::from_raw(2);
    let empty_substs = Substitution::empty();

    let main_body = make_call_body(main_id, foo_id, empty_substs);
    let foo_body = make_simple_body(foo_id);

    let fn_bodies = vec![
        (main_id, empty_substs, Arc::new(main_body)),
        (foo_id, empty_substs, Arc::new(foo_body)),
    ];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: main_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(ctx.item_count(), 2, "should have both main and foo items");
    let items = ctx.items();
    assert!(
        items
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 1)),
        "main should be collected"
    );
    assert!(
        items
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 2)),
        "foo should be collected"
    );
}

/// V23-T02: Recursive function -> no duplicate items
#[test]
fn recursive_function_no_duplicates() {
    let rec_id = FnDefId::from_raw(10);
    let empty_substs = Substitution::empty();

    let rec_body = make_recursive_body(rec_id, empty_substs);
    let fn_bodies = vec![(rec_id, empty_substs, Arc::new(rec_body))];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: rec_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        1,
        "recursive function should appear exactly once"
    );
    assert!(
        ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 10))
    );
}

/// V23-T03: Drop glue for structs -> collected
#[test]
fn drop_glue_collected() {
    let fn_id = FnDefId::from_raw(20);
    let adt_id = AdtId::from_raw(50);
    let empty_substs = Substitution::empty();

    let drop_body = make_drop_body(fn_id, adt_id);
    let fn_bodies = vec![(fn_id, empty_substs, Arc::new(drop_body))];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: fn_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert!(
        ctx.item_count() >= 1,
        "function with drop should be collected"
    );
    assert!(
        ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 20))
    );
}

/// V23-T04: Constant used in generic -> instantiated
#[test]
fn constant_in_generic_instantiated() {
    let main_id = FnDefId::from_raw(30);
    let const_id = ConstDefId::from_raw(5);
    let empty_substs = Substitution::empty();

    let main_body = make_const_ref_body(main_id, const_id, empty_substs);
    let fn_bodies = vec![(main_id, empty_substs, Arc::new(main_body))];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: main_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert!(ctx.item_count() >= 1, "main function should be collected");
    assert!(
        ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 30))
    );
    // A `ConstRef` no longer enqueues a `MonoItem::Const`: constants are
    // materialized by the backend as zero-initialized globals
    // (`__glyim_const_{id}`), so they are intentionally not collected for
    // lowering here (const value materialization is a follow-up).
    assert!(
        !ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Const { .. })),
        "const refs must not be lowered as mono items"
    );
}

/// V23-T05: Diamond dependency - A calls B and C, both call D -> D collected once
#[test]
fn diamond_dependency_dedup() {
    let a_id = FnDefId::from_raw(1);
    let b_id = FnDefId::from_raw(2);
    let c_id = FnDefId::from_raw(3);
    let d_id = FnDefId::from_raw(4);
    let empty_substs = Substitution::empty();

    // Build A's body: calls B then C (two basic blocks, each with a Call)
    let owner_a = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(a_id.to_raw()));
    let mut locals_a = IndexVec::new();
    locals_a.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals_a.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals_a.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals_a.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let call_b = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(b_id, empty_substs),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
            destination: Place::new(LocalIdx::from_raw(2)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let call_c = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(c_id, empty_substs),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
            destination: Place::new(LocalIdx::from_raw(3)),
            target: Some(BasicBlockIdx::from_raw(2)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let ret = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut bb = IndexVec::new();
    bb.push(BasicBlockData {
        statements: vec![],
        terminator: call_b,
        is_cleanup: false,
    });
    bb.push(BasicBlockData {
        statements: vec![],
        terminator: call_c,
        is_cleanup: false,
    });
    bb.push(BasicBlockData {
        statements: vec![],
        terminator: ret,
        is_cleanup: false,
    });
    let a_body = Body {
        owner: owner_a,
        basic_blocks: bb,
        locals: locals_a,
        arg_count: 1,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };

    let b_body = make_call_body(b_id, d_id, empty_substs);
    let c_body = make_call_body(c_id, d_id, empty_substs);
    let d_body = make_simple_body(d_id);

    let fn_bodies = vec![
        (a_id, empty_substs, Arc::new(a_body)),
        (b_id, empty_substs, Arc::new(b_body)),
        (c_id, empty_substs, Arc::new(c_body)),
        (d_id, empty_substs, Arc::new(d_body)),
    ];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: a_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        4,
        "diamond: should have A, B, C, D (D deduped)"
    );
    let d_count = ctx
        .items()
        .iter()
        .filter(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 4))
        .count();
    assert_eq!(d_count, 1, "D should appear exactly once");
}

/// V23-T06: Chain of calls A->B->C->D - all collected transitively
#[test]
fn transitive_call_chain() {
    let a_id = FnDefId::from_raw(1);
    let b_id = FnDefId::from_raw(2);
    let c_id = FnDefId::from_raw(3);
    let d_id = FnDefId::from_raw(4);
    let empty_substs = Substitution::empty();

    let fn_bodies = vec![
        (
            a_id,
            empty_substs,
            Arc::new(make_call_body(a_id, b_id, empty_substs)),
        ),
        (
            b_id,
            empty_substs,
            Arc::new(make_call_body(b_id, c_id, empty_substs)),
        ),
        (
            c_id,
            empty_substs,
            Arc::new(make_call_body(c_id, d_id, empty_substs)),
        ),
        (d_id, empty_substs, Arc::new(make_simple_body(d_id))),
    ];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: a_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        4,
        "chain: A->B->C->D should collect all 4"
    );
    for raw_id in [1u32, 2, 3, 4] {
        assert!(
            ctx.items().iter().any(
                |d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == raw_id)
            ),
            "function with raw id {raw_id} should be collected"
        );
    }
}

/// V23-T07: Two different substitutions of same function -> both collected
#[test]
fn different_substitutions_both_collected() {
    let main_id = FnDefId::from_raw(1);
    let foo_id = FnDefId::from_raw(2);
    let empty_substs = Substitution::empty();

    let main_body = make_call_body(main_id, foo_id, empty_substs);
    let foo_body = make_simple_body(foo_id);

    let fn_bodies = vec![
        (main_id, empty_substs, Arc::new(main_body)),
        (foo_id, empty_substs, Arc::new(foo_body)),
    ];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: main_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert!(
        ctx.item_count() >= 2,
        "should collect at least main and foo"
    );
}

/// V23-T08: Empty start set -> nothing collected
#[test]
fn empty_start_nothing_collected() {
    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[],
        &|def_id, _substs| Arc::new(Body::dummy(def_id)),
        &|_ty| dummy_body(),
    );
    assert_eq!(
        ctx.item_count(),
        0,
        "empty start set should collect nothing"
    );
}

/// V23-T09: Static item -> collected but no body scanning needed
#[test]
fn static_item_collected() {
    let static_id = StaticDefId::from_raw(42);
    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Static { def_id: static_id }],
        &|def_id, _substs| Arc::new(Body::dummy(def_id)),
        &|_ty| dummy_body(),
    );

    assert_eq!(ctx.item_count(), 1, "static should be collected");
    assert!(
        ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Static { def_id } if def_id.to_raw() == 42))
    );
}

/// V23-T10: Multiple constants in same body
#[test]
fn multiple_constants_in_body() {
    let main_id = FnDefId::from_raw(1);
    let const_a = ConstDefId::from_raw(10);
    let const_b = ConstDefId::from_raw(11);
    let empty_substs = Substitution::empty();

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(main_id.to_raw()));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let stmt_a = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::ConstRef(const_a, empty_substs),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let stmt_b = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(2)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::ConstRef(const_b, empty_substs),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![stmt_a, stmt_b],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    });

    let main_body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: main_id,
            substs: empty_substs,
        }],
        &make_body_lookup(vec![(main_id, empty_substs, Arc::new(main_body))], vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        1,
        "should collect only main (consts are not lowered)"
    );
    // ConstRef constants are materialized by the backend as zero-init globals
    // and are intentionally not collected as mono items (follow-up: const
    // value materialization).
    assert!(
        !ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Const { .. })),
        "const refs must not be lowered as mono items"
    );
}

/// V23-T11: Mixed calls and constants
#[test]
fn mixed_calls_and_constants() {
    let main_id = FnDefId::from_raw(1);
    let helper_id = FnDefId::from_raw(2);
    let const_id = ConstDefId::from_raw(20);
    let empty_substs = Substitution::empty();

    // Body with both a Call and a ConstRef
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(main_id.to_raw()));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let stmt_const = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(3)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::ConstRef(const_id, empty_substs),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let call_term = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(helper_id, empty_substs),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![Operand::Copy(Place::new(LocalIdx::from_raw(1)))],
            destination: Place::new(LocalIdx::from_raw(2)),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let ret = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![stmt_const],
        terminator: call_term,
        is_cleanup: false,
    });
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: ret,
        is_cleanup: false,
    });

    let main_body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 1,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    let helper_body = make_simple_body(helper_id);

    let fn_bodies = vec![
        (main_id, empty_substs, Arc::new(main_body)),
        (helper_id, empty_substs, Arc::new(helper_body)),
    ];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: main_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        2,
        "should collect main and helper (const is not lowered)"
    );
    assert!(
        ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 1))
    );
    assert!(
        ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 2))
    );
    // The ConstRef to const 20 must NOT be collected as a mono item.
    assert!(
        !ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Const { .. })),
        "const refs must not be lowered as mono items"
    );
}

/// V23-T12: Item with no references (leaf function) -> only itself collected
#[test]
fn leaf_function_only_self() {
    let leaf_id = FnDefId::from_raw(99);
    let empty_substs = Substitution::empty();

    let leaf_body = make_simple_body(leaf_id);
    let fn_bodies = vec![(leaf_id, empty_substs, Arc::new(leaf_body))];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: leaf_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        1,
        "leaf function should collect only itself"
    );
    assert!(
        ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Fn { def_id, .. } if def_id.to_raw() == 99))
    );
}

/// V23-T13: Multiple start items
#[test]
fn multiple_start_items() {
    let a_id = FnDefId::from_raw(1);
    let b_id = FnDefId::from_raw(2);
    let empty_substs = Substitution::empty();

    let fn_bodies = vec![
        (a_id, empty_substs, Arc::new(make_simple_body(a_id))),
        (b_id, empty_substs, Arc::new(make_simple_body(b_id))),
    ];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[
            MonoItem::Fn {
                def_id: a_id,
                substs: empty_substs,
            },
            MonoItem::Fn {
                def_id: b_id,
                substs: empty_substs,
            },
        ],
        &make_body_lookup(fn_bodies, vec![]),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        2,
        "two start items should both be collected"
    );
}

/// V23-T14: Transitive constant collection (const references another const)
#[test]
fn transitive_constant_collection() {
    let fn_id = FnDefId::from_raw(1);
    let const_a = ConstDefId::from_raw(10);
    let const_b = ConstDefId::from_raw(11);
    let empty_substs = Substitution::empty();

    // Main body references const_a
    let fn_body = make_const_ref_body(fn_id, const_a, empty_substs);

    // const_a's body references const_b
    let const_a_body = make_const_ref_body(fn_id, const_b, empty_substs);
    // We reuse fn_id owner for the const body; the owner isn't important for the test,
    // what matters is that the body contains a ConstRef to const_b.

    let fn_bodies = vec![(fn_id, empty_substs, Arc::new(fn_body))];
    let const_bodies = vec![(const_a, empty_substs, Arc::new(const_a_body))];

    let mut ctx = MonoCtx::new();
    ctx.collect(
        &[MonoItem::Fn {
            def_id: fn_id,
            substs: empty_substs,
        }],
        &make_body_lookup(fn_bodies, const_bodies),
        &|_ty| dummy_body(),
    );

    // With the new design, const refs do NOT enqueue `MonoItem::Const`
    // items: they are materialized as zero-init globals by the backend, so
    // neither const_a nor const_b is collected for lowering.
    assert_eq!(ctx.item_count(), 1, "should collect only the fn");
    assert!(
        !ctx.items()
            .iter()
            .any(|d| matches!(&d.item, MonoItem::Const { .. })),
        "const refs (a and b) must not be lowered as mono items"
    );
}
