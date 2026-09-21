//! MIR well-formedness validator (de-stubbing plan §8.8).
//!
//! After every optimization pass (and before codegen), a well-formed MIR body must
//! satisfy a small set of structural invariants. Violations indicate a compiler
//! bug introduced by an earlier pass, not bad user input, so the validator reports
//! them as internal compiler errors rather than "your program is wrong" diagnostics.
//!
//! Invariants checked:
//!   * `SwitchInt` targets reference blocks that exist in the body.
//!   * `Drop`/`Goto`/`Call`/`Assert` targets (and cleanup edges) reference existing blocks.
//!   * No `ConstantIndex`/`Subslice` projection appears except as the *last*
//!     element of a place's projection list — `glyim-opt::slice_desugar` exists
//!     precisely to make this hold, so a survivor mid-chain is a pass bug.
//!   * A `Drop` terminator's place type must need drop glue, per the canonical
//!     `TyCtx::needs_drop` (plan §15.1) — a `Drop` on a non-droppable type is a
//!     compiler bug and is flagged as `UnnecessaryDrop`.
//!
//! The validator is intentionally *not* wired into the required `optimize()` path
//! yet: it is exposed via [`validate_body`] and exercised by the unit tests below,
//! and by `cargo test`. Once the rest of the optimization pipeline is proven to keep
//! these invariants, `optimize()` can call `validate_body` (debug-gated) between
//! passes without risk to the green tree.

use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Ty, TyCtx};
use std::fmt;

/// A single well-formedness violation found by the validator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MirValidationError {
    /// Struct.
    pub kind: MirValidationErrorKind,
    /// Struct.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// MirValidationErrorKind.
pub enum MirValidationErrorKind {
    /// A terminator references a basic block that does not exist.
    UnknownTarget(BasicBlockIdx),
    /// A `Drop` terminator whose place type does not need drop glue, per the
    /// canonical `TyCtx::needs_drop` (plan §15.1). Holding the offending place
    /// and type makes the diagnostic actionable and lets tests assert on it.
    /// Struct.
    UnnecessaryDrop {
        /// place field.
        place: Place,
        /// ty field.
        ty: Ty,
    },
    /// A `ConstantIndex`/`Subslice` projection appears mid-chain (not terminal).
    NonTerminalSliceProjection,
    /// A `ProjectionElem::Subslice` survives past `slice_desugar` (plan §8.7).
    /// `slice_desugar` exists to remove every `Subslice` from MIR; any survivor
    /// reaching codegen is a compiler bug. When this fires, codegen's
    /// `unreachable!("Subslice")` (glyim-codegen-llvm/src/lower.rs) becomes true
    /// by construction rather than by hope.
    SubsliceAfterDesugar,
}

impl fmt::Display for MirValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            MirValidationErrorKind::UnknownTarget(bb) => {
                write!(f, "MIR validation: terminator targets unknown block {bb:?}")
            }
            MirValidationErrorKind::UnnecessaryDrop { place, ty } => {
                write!(
                    f,
                    "MIR validation: Drop terminator on a type that needs no drop glue: place {place:?} (ty {ty:?})"
                )
            }
            MirValidationErrorKind::NonTerminalSliceProjection => {
                write!(
                    f,
                    "MIR validation: ConstantIndex/Subslice projection is not terminal"
                )
            }
            MirValidationErrorKind::SubsliceAfterDesugar => {
                write!(
                    f,
                    "MIR validation: Subslice projection survived slice_desugar (compiler bug)"
                )
            }
        }
    }
}

/// Validate `body`. Returns `Ok(())` if every invariant holds, otherwise the first
/// violation found (in deterministic block/statement order).
pub fn validate_body(ctx: &TyCtx, body: &Body) -> Result<(), MirValidationError> {
    let n_blocks = body.basic_blocks.len();

    let block_exists = |bb: BasicBlockIdx| (bb.to_raw() as usize) < n_blocks;

    for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
        // Validate each statement's places (slice-projection terminality).
        for stmt in &bb.statements {
            if let StatementKind::Assign(_, rvalue) = &stmt.kind {
                check_rvalue_places(ctx, rvalue, &mut |e| Err(e))?;
            }
        }

        // Validate the terminator.
        match &bb.terminator.kind {
            TerminatorKind::Goto { target } => {
                if !block_exists(*target) {
                    return Err(MirValidationError {
                        kind: MirValidationErrorKind::UnknownTarget(*target),
                        span: bb.terminator.source_info.span,
                    });
                }
            }
            TerminatorKind::Drop {
                target, cleanup, ..
            }
            | TerminatorKind::Assert {
                target, cleanup, ..
            } => {
                if !block_exists(*target) {
                    return Err(MirValidationError {
                        kind: MirValidationErrorKind::UnknownTarget(*target),
                        span: bb.terminator.source_info.span,
                    });
                }
                if let Some(c) = cleanup
                    && !block_exists(*c)
                {
                    return Err(MirValidationError {
                        kind: MirValidationErrorKind::UnknownTarget(*c),
                        span: bb.terminator.source_info.span,
                    });
                }
                // Drop-needs-drop consistency (plan §15.1). `needs_drop` is now
                // the single canonical implementation (`TyCtx::needs_drop`, §7.1),
                // so this assertion is safe to enable: a `Drop` terminator whose
                // place type needs no drop glue is a compiler bug (e.g. the
                // array drop-glue stub of §16.1 would be caught here).
                if let TerminatorKind::Drop { place, .. } = &bb.terminator.kind {
                    let ty = place.ty(ctx, &body.locals);
                    if !ctx.needs_drop(ty) {
                        return Err(MirValidationError {
                            kind: MirValidationErrorKind::UnnecessaryDrop {
                                place: place.clone(),
                                ty,
                            },
                            span: bb.terminator.source_info.span,
                        });
                    }
                }
            }
            TerminatorKind::SwitchInt { targets, .. } => {
                if !block_exists(targets.otherwise()) {
                    return Err(MirValidationError {
                        kind: MirValidationErrorKind::UnknownTarget(targets.otherwise()),
                        span: bb.terminator.source_info.span,
                    });
                }
                for (_, tgt) in targets.iter() {
                    if !block_exists(tgt) {
                        return Err(MirValidationError {
                            kind: MirValidationErrorKind::UnknownTarget(tgt),
                            span: bb.terminator.source_info.span,
                        });
                    }
                }
            }
            TerminatorKind::Call {
                target, cleanup, ..
            } => {
                if let Some(t) = target
                    && !block_exists(*t)
                {
                    return Err(MirValidationError {
                        kind: MirValidationErrorKind::UnknownTarget(*t),
                        span: bb.terminator.source_info.span,
                    });
                }
                if let Some(c) = cleanup
                    && !block_exists(*c)
                {
                    return Err(MirValidationError {
                        kind: MirValidationErrorKind::UnknownTarget(*c),
                        span: bb.terminator.source_info.span,
                    });
                }
            }
            TerminatorKind::Return | TerminatorKind::Unreachable => {}
        }

        let _ = bb_idx;
    }

    Ok(())
}

/// Assert that no `ProjectionElem::Subslice` survives in `body` (plan §8.7).
///
/// `slice_desugar` exists precisely to eliminate every `Subslice` from MIR.
/// This is the post-condition check: once `slice_desugar` has run, a surviving
/// `Subslice` is a compiler bug, not bad user input. Calling this right after
/// `slice_desugar::run` makes codegen's `unreachable!("Subslice")`
/// (glyim-codegen-llvm/src/lower.rs) true by construction rather than by hope.
pub fn validate_no_subslice(body: &Body) -> Result<(), MirValidationError> {
    for bb in body.basic_blocks.iter() {
        for stmt in &bb.statements {
            if let StatementKind::Assign(_, rvalue) = &stmt.kind
                && let Some(place) = rvalue_place(rvalue)
                && place
                    .projection
                    .iter()
                    .any(|e| matches!(e, ProjectionElem::Subslice { .. }))
            {
                return Err(MirValidationError {
                    kind: MirValidationErrorKind::SubsliceAfterDesugar,
                    span: bb
                        .statements
                        .first()
                        .map(|s| s.source_info.span)
                        .unwrap_or(Span::DUMMY),
                });
            }
        }
        // Terminator operands / places don't carry Subslice (only statement
        // Assign RHS places do in this MIR), but scan defensively anyway.
        if let TerminatorKind::Drop { place, .. } = &bb.terminator.kind
            && place
                .projection
                .iter()
                .any(|e| matches!(e, ProjectionElem::Subslice { .. }))
        {
            return Err(MirValidationError {
                kind: MirValidationErrorKind::SubsliceAfterDesugar,
                span: bb.terminator.source_info.span,
            });
        }
    }
    Ok(())
}

/// Best-effort extraction of the single place an `Rvalue` reads/writes, used by
/// `validate_no_subslice`. Returns `None` for rvalues with no place operand.
fn rvalue_place(rvalue: &Rvalue) -> Option<&Place> {
    match rvalue {
        Rvalue::Ref(p, _) => Some(p),
        Rvalue::Discriminant(p) | Rvalue::Len(p) => Some(p),
        Rvalue::Use(Operand::Copy(p) | Operand::Move(p)) => Some(p),
        _ => None,
    }
}
/// projection that is not the final element of its place's projection list.
fn check_rvalue_places<F: FnMut(MirValidationError) -> Result<(), MirValidationError>>(
    ctx: &TyCtx,
    rvalue: &Rvalue,
    on_err: &mut F,
) -> Result<(), MirValidationError> {
    let mut visit_place = |place: &Place| -> Result<(), MirValidationError> {
        let proj = &place.projection;
        for (i, elem) in proj.iter().enumerate() {
            let is_slice = matches!(
                elem,
                ProjectionElem::ConstantIndex { .. } | ProjectionElem::Subslice { .. }
            );
            if is_slice && i + 1 != proj.len() {
                let err = MirValidationError {
                    kind: MirValidationErrorKind::NonTerminalSliceProjection,
                    span: Span::DUMMY,
                };
                return on_err(err);
            }
        }
        let _ = ctx;
        Ok(())
    };

    match rvalue {
        Rvalue::Use(op) | Rvalue::UnaryOp(_, op) => {
            if let Operand::Copy(p) | Operand::Move(p) = op {
                visit_place(p)?;
            }
        }
        Rvalue::Ref(p, _) => visit_place(p)?,
        Rvalue::BinaryOp(_, ops) => {
            if let Operand::Copy(p) | Operand::Move(p) = &ops.0 {
                visit_place(p)?;
            }
            if let Operand::Copy(p) | Operand::Move(p) = &ops.1 {
                visit_place(p)?;
            }
        }
        Rvalue::Aggregate(_, operands) => {
            for op in operands {
                if let Operand::Copy(p) | Operand::Move(p) = op {
                    visit_place(p)?;
                }
            }
        }
        Rvalue::Discriminant(p) | Rvalue::Len(p) => visit_place(p)?,
        Rvalue::Cast(_, op, _) => {
            if let Operand::Copy(p) | Operand::Move(p) = op {
                visit_place(p)?;
            }
        }
        Rvalue::Repeat(op, _) => {
            if let Operand::Copy(p) | Operand::Move(p) = op {
                visit_place(p)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::IndexVec;
    use glyim_core::Mutability;
    use glyim_test::with_fresh_ty_ctx;

    fn empty_body(local_ty: glyim_type::Ty, return_ty: glyim_type::Ty) -> Body {
        // Build a minimal 1-block body that just returns.
        let mut basic_blocks = IndexVec::new();
        basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }));
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            ty: local_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        Body {
            owner: glyim_core::def_id::DefId::new(
                glyim_core::def_id::CrateId::from_raw(0),
                glyim_core::def_id::LocalDefId::from_raw(0),
            ),
            basic_blocks,
            locals,
            arg_count: 0,
            return_ty,
            span: Span::DUMMY,
            var_debug_info: Vec::new(),
        }
    }

    #[test]
    fn validates_trivial_return_body() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let body = empty_body(i32_ty, i32_ty);
        assert!(validate_body(&ctx, &body).is_ok());
    }

    #[test]
    fn rejects_drop_to_missing_block() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let mut body = empty_body(i32_ty, i32_ty);
        // Inject a Drop terminator whose target block does not exist.
        body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(LocalIdx::from_raw(0)),
                target: glyim_mir::BasicBlockIdx::from_raw(99),
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let err = validate_body(&ctx, &body).expect_err("Drop to missing block should be flagged");
        assert_eq!(
            err.kind,
            MirValidationErrorKind::UnknownTarget(glyim_mir::BasicBlockIdx::from_raw(99))
        );
    }

    /// Build a body whose sole block ends in a `Drop` of local 0 targeting block 0
    /// (a valid, existing target) so the needs-drop check actually runs.
    fn body_with_drop(local_ty: glyim_type::Ty, return_ty: glyim_type::Ty) -> Body {
        let mut body = empty_body(local_ty, return_ty);
        body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(LocalIdx::from_raw(0)),
                target: glyim_mir::BasicBlockIdx::from_raw(0),
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        body
    }

    #[test]
    fn flags_drop_on_non_droppable_type() {
        // i32 needs no drop glue; a Drop terminator over it is a compiler bug.
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let body = body_with_drop(i32_ty, i32_ty);
        let err = validate_body(&ctx, &body).expect_err("Drop on i32 should be flagged");
        match err.kind {
            MirValidationErrorKind::UnnecessaryDrop { place, ty } => {
                assert_eq!(place, Place::new(LocalIdx::from_raw(0)));
                assert_eq!(ty, i32_ty);
            }
            other => panic!("expected UnnecessaryDrop, got {other:?}"),
        }
    }

    #[test]
    fn allows_drop_on_droppable_type() {
        // String needs drop glue; a Drop terminator over it is correct.
        let (ctx, str_ty) = with_fresh_ty_ctx(|c| c.mk_ty(glyim_type::TyKind::String));
        let body = body_with_drop(str_ty, str_ty);
        assert!(
            validate_body(&ctx, &body).is_ok(),
            "Drop on String must be allowed"
        );
    }

    #[test]
    fn rejects_terminator_to_missing_block() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let mut body = empty_body(i32_ty, i32_ty);
        body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Goto {
                target: glyim_mir::BasicBlockIdx::from_raw(99),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let err = validate_body(&ctx, &body).expect_err("missing block should be flagged");
        assert_eq!(
            err.kind,
            MirValidationErrorKind::UnknownTarget(glyim_mir::BasicBlockIdx::from_raw(99))
        );
    }

    #[test]
    fn validate_no_subslice_passes_clean_body() {
        let (_ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let body = empty_body(i32_ty, i32_ty);
        assert!(validate_no_subslice(&body).is_ok());
    }

    #[test]
    fn validate_no_subslice_flags_surviving_subslice() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let mut body = empty_body(i32_ty, i32_ty);
        // Inject an `Assign` whose RHS reads a place with a `Subslice` projection.
        let subslice_place = Place {
            local: glyim_mir::LocalIdx::from_raw(0),
            projection: Box::new([glyim_mir::ProjectionElem::Subslice {
                from: 1,
                to: 2,
                from_end: false,
            }]),
        };
        body.basic_blocks[glyim_mir::BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(glyim_mir::LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Copy(subslice_place)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });
        let err = validate_no_subslice(&body).expect_err("surviving Subslice should be flagged");
        assert_eq!(err.kind, MirValidationErrorKind::SubsliceAfterDesugar);
        // The general validate_body (pre-pass) must NOT flag a terminal Subslice.
        assert!(validate_body(&ctx, &body).is_ok());
    }
}
