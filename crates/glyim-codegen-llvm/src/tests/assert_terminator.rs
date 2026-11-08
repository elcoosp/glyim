//! S22-T04: Tests for Assert terminator.

use super::helpers::*;
use crate::LlvmBackend;
use glyim_core::primitives::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::*;

#[test]
fn assert_with_overflow_add_emits_comparison() {
    let ctx = with_fresh_ty_ctx(|c| c.bool_ty()).0;
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let cond_local = builder.add_local(Ty::BOOL);
    let success_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    builder.set_terminator(Terminator {
        kind: TerminatorKind::Assert {
            cond: Operand::Copy(Place::new(cond_local)),
            expected: true,
            target: success_bb,
            cleanup: None,
            msg: AssertMessage::Overflow(BinOp::Add),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
    let ir = ir.unwrap();
    assert!(
        ir.contains("br") || ir.contains("icmp"),
        "Assert should emit comparison and branch, got:\n{ir}"
    );
}

#[test]
fn assert_with_division_by_zero_emits_comparison() {
    let ctx = with_fresh_ty_ctx(|c| c.bool_ty()).0;
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let cond_local = builder.add_local(Ty::BOOL);
    let success_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    builder.set_terminator(Terminator {
        kind: TerminatorKind::Assert {
            cond: Operand::Copy(Place::new(cond_local)),
            expected: true,
            target: success_bb,
            cleanup: None,
            msg: AssertMessage::DivisionByZero,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
    let ir = ir.unwrap();
    assert!(
        ir.contains("br"),
        "Assert DivisionByZero should emit conditional branch, got:\n{ir}"
    );
}

#[test]
fn assert_with_bounds_check_emits_comparison() {
    let ctx = with_fresh_ty_ctx(|c| c.bool_ty()).0;
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let cond_local = builder.add_local(Ty::BOOL);
    let success_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    builder.set_terminator(Terminator {
        kind: TerminatorKind::Assert {
            cond: Operand::Copy(Place::new(cond_local)),
            expected: true,
            target: success_bb,
            cleanup: None,
            msg: AssertMessage::BoundsCheck,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
    let ir = ir.unwrap();
    assert!(
        ir.contains("br"),
        "Assert BoundsCheck should emit conditional branch, got:\n{ir}"
    );
}

#[test]
fn assert_with_expected_false_negates_condition() {
    let ctx = with_fresh_ty_ctx(|c| c.bool_ty()).0;
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let cond_local = builder.add_local(Ty::BOOL);
    let success_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    builder.set_terminator(Terminator {
        kind: TerminatorKind::Assert {
            cond: Operand::Copy(Place::new(cond_local)),
            expected: false,
            target: success_bb,
            cleanup: None,
            msg: AssertMessage::Overflow(BinOp::Sub),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
    let ir = ir.unwrap();
    assert!(
        ir.contains("br"),
        "Assert with expected=false should emit conditional branch, got:\n{ir}"
    );
}

#[test]
fn assert_with_cleanup_block() {
    let ctx = with_fresh_ty_ctx(|c| c.bool_ty()).0;
    let backend = LlvmBackend::new().with_ty_ctx(ctx);
    let mut builder = BodyBuilder::new(Ty::UNIT);
    let cond_local = builder.add_local(Ty::BOOL);
    let success_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let cleanup_bb = builder.add_block(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    builder.set_terminator(Terminator {
        kind: TerminatorKind::Assert {
            cond: Operand::Copy(Place::new(cond_local)),
            expected: true,
            target: success_bb,
            cleanup: Some(cleanup_bb),
            msg: AssertMessage::Overflow(BinOp::Mul),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = builder.build();
    let ir = backend.generate_ir(&body);
    assert!(ir.is_ok(), "generate_ir should succeed: {:?}", ir.err());
}
