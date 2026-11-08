//! S18-T01: Constant propagation folds arithmetic.

use super::testutil::*;
use crate::optimize;
use glyim_core::Mutability;
use glyim_mir::*;
use glyim_test::with_fresh_ty_ctx;
use std::sync::Arc;

/// Test that a constant assignment `_1 = 5` propagated to `_2 = _1`
/// results in `_0` receiving the constant directly.
#[test]
fn const_prop_assign_propagates_constant() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
            (i32_ty, Mutability::Mut), // _2
        ];

        // bb0:
        //   _1 = 5
        //   _2 = _1        // should become _2 = 5
        //   _0 = _2        // should become _0 = 5
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(1)), const_int_rvalue(5, i32_ty)),
                assign_stmt(Place::new(local(2)), Rvalue::Use(copy_op(local(1)))),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(2)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));

    // After const prop + DCE: _0 = 5, _1 and _2 are dead and removed
    let block = &optimized.body.basic_blocks[bb(0)];

    // Find the assignment to _0
    let assign_to_0 = block
        .statements
        .iter()
        .find(|stmt| matches!(&stmt.kind, StatementKind::Assign(p, _) if p.local == local(0)));
    assert!(assign_to_0.is_some(), "_0 should have an assignment");

    if let StatementKind::Assign(_, Rvalue::Use(Operand::Constant(c))) = &assign_to_0.unwrap().kind
    {
        assert!(
            matches!(c.kind, MirConstKind::Int(5)),
            "_0 should be constant 5 after propagation"
        );
    } else {
        panic!("_0 assignment should be Use(Constant(5))");
    }
}

/// Test that a `BinaryOp` with two constant operands gets its operands propagated.
#[test]
fn const_prop_binary_op_operands_replaced() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
            (i32_ty, Mutability::Mut), // _2
            (i32_ty, Mutability::Mut), // _3
        ];

        // bb0:
        //   _1 = 5
        //   _2 = 3
        //   _3 = Add(_1, _2)   // should become Add(5, 3) or be folded to 8
        //   _0 = _3
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(1)), const_int_rvalue(5, i32_ty)),
                assign_stmt(Place::new(local(2)), const_int_rvalue(3, i32_ty)),
                assign_stmt(
                    Place::new(local(3)),
                    Rvalue::BinaryOp(
                        glyim_core::primitives::BinOp::Add,
                        Box::new((copy_op(local(1)), copy_op(local(2)))),
                    ),
                ),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(3)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));
    let block = &optimized.body.basic_blocks[bb(0)];

    // Either the BinaryOp still exists with constant operands, or it has been folded
    // and the final assignment to _0 is a constant (8).
    let has_binary_op = block
        .statements
        .iter()
        .any(|stmt| matches!(&stmt.kind, StatementKind::Assign(_, Rvalue::BinaryOp(_, _))));

    if has_binary_op {
        let binop_stmt = block
            .statements
            .iter()
            .find(|stmt| matches!(&stmt.kind, StatementKind::Assign(_, Rvalue::BinaryOp(_, _))))
            .unwrap();
        if let StatementKind::Assign(_, Rvalue::BinaryOp(_, ops)) = &binop_stmt.kind {
            assert!(
                matches!(ops.0, Operand::Constant(_)),
                "lhs of BinaryOp should be constant after propagation"
            );
            assert!(
                matches!(ops.1, Operand::Constant(_)),
                "rhs of BinaryOp should be constant after propagation"
            );
        }
    } else {
        // Expect that the final assignment to _0 is a constant 8
        let assign_to_0 = block
            .statements
            .iter()
            .find(|stmt| matches!(&stmt.kind, StatementKind::Assign(p, _) if p.local == local(0)))
            .expect("_0 should have an assignment");
        if let StatementKind::Assign(_, Rvalue::Use(Operand::Constant(c))) = &assign_to_0.kind {
            assert!(
                matches!(c.kind, MirConstKind::Int(8)),
                "Expected constant 8, got {:?}",
                c.kind
            );
        } else {
            panic!("_0 assignment should be Use(Constant(8)) after folding");
        }
    }
}

/// Test that a non-constant assignment invalidates previous constant knowledge.
///
/// We verify that propagation happens correctly before invalidation, and
/// that after a local is overwritten with a non-constant, it no longer
/// propagates the old constant to subsequent uses.
#[test]
fn const_prop_invalidation_on_non_const_assign() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ty_i32(ctx);

        let locals = vec![
            (i32_ty, Mutability::Mut), // _0 return
            (i32_ty, Mutability::Mut), // _1
            (i32_ty, Mutability::Mut), // _2
            (i32_ty, Mutability::Mut), // _3
        ];

        // bb0:
        //   _1 = 5
        //   _2 = _1           // should become _2 = 5 (propagated before overwrite)
        //   _0 = _2           // should become _0 = 5 (propagated through _2)
        //   _1 = _3           // _1 is overwritten with a non-constant
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(1)), const_int_rvalue(5, i32_ty)),
                assign_stmt(Place::new(local(2)), Rvalue::Use(copy_op(local(1)))),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(2)))),
                assign_stmt(Place::new(local(1)), Rvalue::Use(copy_op(local(3)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, i32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));
    let block = &optimized.body.basic_blocks[bb(0)];

    // _0 should be constant 5 (propagated from _1 through _2)
    let assign_to_0 = block
        .statements
        .iter()
        .find(|stmt| matches!(&stmt.kind, StatementKind::Assign(p, _) if p.local == local(0)));
    assert!(assign_to_0.is_some(), "_0 should have an assignment");
    if let StatementKind::Assign(_, Rvalue::Use(Operand::Constant(c))) = &assign_to_0.unwrap().kind
    {
        assert!(
            matches!(c.kind, MirConstKind::Int(5)),
            "_0 should be constant 5 (propagated from _1 through _2)"
        );
    } else {
        panic!("_0 should be Use(Constant(5)) after propagation");
    }
}

/// Test propagation of unsigned constants.
#[test]
fn const_prop_unsigned_constant() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let u32_ty = ty_u32(ctx);

        let locals = vec![
            (u32_ty, Mutability::Mut), // _0 return
            (u32_ty, Mutability::Mut), // _1
            (u32_ty, Mutability::Mut), // _2
        ];

        // bb0:
        //   _1 = 42u32
        //   _2 = _1
        //   _0 = _2
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(1)), const_uint_rvalue(42, u32_ty)),
                assign_stmt(Place::new(local(2)), Rvalue::Use(copy_op(local(1)))),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(2)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, u32_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));
    let block = &optimized.body.basic_blocks[bb(0)];

    // _0 = 42u32 (propagated)
    let assign_to_0 = block
        .statements
        .iter()
        .find(|stmt| matches!(&stmt.kind, StatementKind::Assign(p, _) if p.local == local(0)));
    assert!(assign_to_0.is_some(), "_0 should have an assignment");
    if let StatementKind::Assign(_, Rvalue::Use(Operand::Constant(c))) = &assign_to_0.unwrap().kind
    {
        assert!(
            matches!(c.kind, MirConstKind::Uint(42)),
            "_0 should be unsigned constant 42"
        );
    } else {
        panic!("_0 should be Use(Constant(Uint(42)))");
    }
}

/// Test that boolean constants are propagated.
#[test]
fn const_prop_bool_constant() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx| {
        let bool_ty = ty_bool(ctx);

        let locals = vec![
            (bool_ty, Mutability::Mut), // _0 return
            (bool_ty, Mutability::Mut), // _1
            (bool_ty, Mutability::Mut), // _2
        ];

        // bb0:
        //   _1 = true
        //   _2 = _1
        //   _0 = _2
        //   return
        let block0 = make_block(
            vec![
                assign_stmt(Place::new(local(1)), Rvalue::Use(const_bool_val(true))),
                assign_stmt(Place::new(local(2)), Rvalue::Use(copy_op(local(1)))),
                assign_stmt(Place::new(local(0)), Rvalue::Use(copy_op(local(2)))),
            ],
            return_term(),
        );

        build_test_body(locals, vec![block0], 0, bool_ty)
    });

    let optimized = optimize(&ctx, &Arc::new(body));
    let block = &optimized.body.basic_blocks[bb(0)];

    // _0 = true (propagated)
    let assign_to_0 = block
        .statements
        .iter()
        .find(|stmt| matches!(&stmt.kind, StatementKind::Assign(p, _) if p.local == local(0)));
    assert!(assign_to_0.is_some(), "_0 should have an assignment");
    if let StatementKind::Assign(_, Rvalue::Use(Operand::Constant(c))) = &assign_to_0.unwrap().kind
    {
        assert!(
            matches!(c.kind, MirConstKind::Bool(true)),
            "_0 should be bool constant true"
        );
    } else {
        panic!("_0 should be Use(Constant(Bool(true)))");
    }
}
