//! MIR-level slice/array projection desugaring.
//!
//! `ProjectionElem::ConstantIndex` and `ProjectionElem::Subslice` compute a
//! new base pointer (and, for `Subslice`, a new *length*) from constant
//! offsets. Every consumer of `Place` projections downstream of this pass
//! -- `glyim-codegen-llvm`'s `place_ptr`, `glyim-codegen`'s
//! `emit_place_address`, and `glyim-mir-interp`'s place evaluator --
//! assumes these are only ever the *last* element of a place's projection
//! list. That's because unlike `Field`/`Index`/`Downcast`, which just walk
//! one step deeper into the *same* allocation, `Subslice`'s result is a
//! genuinely new value (`{ ptr, len }`) that has to be materialized
//! somewhere before it can be treated as "the base of further
//! projections". A `Subslice` in the middle of a projection chain (e.g.
//! matching a field of the subslice, `x[1..][0]` collapsed into one
//! place) is not something any backend can address directly.
//!
//! This pass makes "ConstantIndex/Subslice is always terminal" hold
//! everywhere *by construction*, so backends never have to defend against
//! it. Any place where one of these projections is *not* already the last
//! element gets split at that point:
//!
//! ```text
//! _1 = move (*_2)[1..].field[3]     // ILLEGAL: Subslice not terminal
//! =>
//! StorageLive(_tmp)
//! _tmp = (*_2)[1..]                 // materialize the subslice value
//! _1 = move _tmp.field[3]           // continue from the temporary
//! ```
//!
//! Bodies with no such projections -- the overwhelming common case, since
//! `ConstantIndex`/`Subslice` only show up from slice-pattern matching --
//! are left completely unchanged; this pass is a no-op for them.
//!
//! ## Dynamic range slicing (`arr[i..j]` with runtime bounds) — implemented
//!
//! `ConstantIndex`/`Subslice` still only express *compile-time-constant*
//! offsets (`offset: u64`, `from: u64`, `to: u64`) -- that's sufficient for
//! slice-pattern prefixes/suffixes (`[a, b, ..rest]`), whose lengths are
//! always known at the pattern's definition site. A genuinely dynamic
//! range-index expression like `arr[i..j]` where `i`/`j` are runtime locals
//! cannot be expressed as a `Place` projection (there is no projection that
//! can carry a `Place`/`Operand` as its bound), so it cannot go through
//! `ConstantIndex`/`Subslice` at all. That is **no longer a gap**: it is
//! lowered in `glyim-lower` at THIR->MIR build time, before this pass ever
//! runs. `MirBuilder::lower_dynamic_range_slice`
//! (`crates/glyim-lower/src/lower_rvalue.rs`) computes `data_ptr =
//! base_ptr + i * elem_size` and `len = j - i` via `Len`/`Mul`/`Add`/`Sub`
//! rvalues, inserts runtime bounds-check asserts (`start <= end`,
//! `end <= len`), and constructs the `{ ptr, len }` tuple aggregate --
//! exactly the shape this pass already produces for its intermediate
//! temporaries below, so this pass simply leaves that tuple alone. Both
//! constant-bound (`arr[1..3]`) and runtime-bound (`arr[i..j]`) ranges take
//! this same lowering; the constant case is *not* special-cased to a
//! `Subslice` projection because the codebase deliberately represents every
//! range slice as the `{ ptr, len }` tuple (see
//! `crates/glyim-lower/src/tests/dynamic_range_slice.rs` for the locked-in
//! behavior).

use glyim_core::primitives::Mutability;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::TyCtx;

pub(crate) fn run(ctx: &TyCtx, body: &mut Body) {
    for i in 0..body.basic_blocks.len() {
        let bb = BasicBlockIdx::from_raw(i as u32);
        desugar_block(ctx, body, bb);
    }
}

fn desugar_block(ctx: &TyCtx, body: &mut Body, bb: BasicBlockIdx) {
    // Rebuild the block's statement list, splicing in any intermediate
    // statements a rewrite needs immediately before the statement that
    // required it.
    let old_statements = std::mem::take(&mut body.basic_blocks[bb].statements);
    let mut new_statements = Vec::with_capacity(old_statements.len());
    for mut stmt in old_statements {
        let mut prelude = Vec::new();
        desugar_statement(ctx, body, &mut stmt, &mut prelude);
        new_statements.extend(prelude);
        new_statements.push(stmt);
    }
    body.basic_blocks[bb].statements = new_statements;

    // The terminator can also reference places with non-terminal
    // ConstantIndex/Subslice projections (call args/destination, assert
    // condition, switch discriminant, drop place). Desugar those too,
    // appending any needed statements to the end of the (already rebuilt)
    // statement list, immediately before the terminator runs.
    let span = body.basic_blocks[bb].terminator.source_info.span;
    let mut terminator = std::mem::replace(
        &mut body.basic_blocks[bb].terminator,
        Terminator {
            kind: TerminatorKind::Unreachable,
            source_info: SourceInfo::new(span),
        },
    );
    let mut prelude = Vec::new();
    desugar_terminator(ctx, body, &mut terminator, &mut prelude);
    body.basic_blocks[bb].statements.extend(prelude);
    body.basic_blocks[bb].terminator = terminator;
}

fn desugar_statement(
    ctx: &TyCtx,
    body: &mut Body,
    stmt: &mut Statement,
    prelude: &mut Vec<Statement>,
) {
    let span = stmt.source_info.span;
    match &mut stmt.kind {
        StatementKind::Assign(place, rvalue) => {
            desugar_rvalue(ctx, body, rvalue, prelude, span);
            *place = desugar_place(ctx, body, place, prelude, span);
        }
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {}
    }
}

fn desugar_terminator(
    ctx: &TyCtx,
    body: &mut Body,
    terminator: &mut Terminator,
    prelude: &mut Vec<Statement>,
) {
    let span = terminator.source_info.span;
    match &mut terminator.kind {
        TerminatorKind::SwitchInt { discr, .. } => {
            desugar_operand(ctx, body, discr, prelude, span);
        }
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            desugar_operand(ctx, body, func, prelude, span);
            for arg in args.iter_mut() {
                desugar_operand(ctx, body, arg, prelude, span);
            }
            *destination = desugar_place(ctx, body, destination, prelude, span);
        }
        TerminatorKind::Assert { cond, .. } => {
            desugar_operand(ctx, body, cond, prelude, span);
        }
        TerminatorKind::Drop { place, .. } => {
            *place = desugar_place(ctx, body, place, prelude, span);
        }
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
    }
}

fn desugar_operand(
    ctx: &TyCtx,
    body: &mut Body,
    operand: &mut Operand,
    prelude: &mut Vec<Statement>,
    span: Span,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            *place = desugar_place(ctx, body, place, prelude, span);
        }
        Operand::Constant(_) => {}
    }
}

fn desugar_rvalue(
    ctx: &TyCtx,
    body: &mut Body,
    rvalue: &mut Rvalue,
    prelude: &mut Vec<Statement>,
    span: Span,
) {
    match rvalue {
        Rvalue::Use(op) => desugar_operand(ctx, body, op, prelude, span),
        Rvalue::BinaryOp(_, operands) => {
            let (l, r) = operands.as_mut();
            desugar_operand(ctx, body, l, prelude, span);
            desugar_operand(ctx, body, r, prelude, span);
        }
        Rvalue::UnaryOp(_, op) => desugar_operand(ctx, body, op, prelude, span),
        Rvalue::Ref(place, _) => {
            *place = desugar_place(ctx, body, place, prelude, span);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops.iter_mut() {
                desugar_operand(ctx, body, op, prelude, span);
            }
        }
        Rvalue::Discriminant(place) => {
            *place = desugar_place(ctx, body, place, prelude, span);
        }
        Rvalue::Len(place) => {
            *place = desugar_place(ctx, body, place, prelude, span);
        }
        Rvalue::Cast(_, op, _) => desugar_operand(ctx, body, op, prelude, span),
        Rvalue::Repeat(op, _) => desugar_operand(ctx, body, op, prelude, span),
    }
}

/// If `place` contains a `ConstantIndex`/`Subslice` element that is *not*
/// the last projection element, split the place at that point: push a
/// fresh local + `StorageLive` + `Assign` (copying everything up to and
/// including that element) into `prelude`, and return a new, shorter
/// place rooted at that fresh local for the remaining projections.
/// Recurses in case more than one such violation exists in the same
/// chain. Returns a clone of `place` unchanged if it was already fine
/// (the common case, and the only case for the vast majority of bodies,
/// which contain no `ConstantIndex`/`Subslice` at all).
fn desugar_place(
    ctx: &TyCtx,
    body: &mut Body,
    place: &Place,
    prelude: &mut Vec<Statement>,
    span: Span,
) -> Place {
    let proj = &place.projection;
    let split_at = proj.iter().enumerate().find_map(|(i, elem)| {
        let is_value_producing = matches!(
            elem,
            ProjectionElem::ConstantIndex { .. } | ProjectionElem::Subslice { .. }
        );
        (is_value_producing && i + 1 < proj.len()).then_some(i)
    });

    let Some(split_at) = split_at else {
        return place.clone();
    };

    let prefix_place = Place {
        local: place.local,
        projection: proj[..=split_at].to_vec().into_boxed_slice(),
    };
    let prefix_ty = prefix_place.ty(ctx, &body.locals);

    let tmp_local = body.locals.push(LocalDecl {
        ty: prefix_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(span),
    });
    prelude.push(Statement {
        kind: StatementKind::StorageLive(tmp_local),
        source_info: SourceInfo::new(span),
    });
    prelude.push(Statement {
        kind: StatementKind::Assign(
            Place::new(tmp_local),
            Rvalue::Use(Operand::Copy(prefix_place)),
        ),
        source_info: SourceInfo::new(span),
    });

    let rest_place = Place {
        local: tmp_local,
        projection: proj[split_at + 1..].to_vec().into_boxed_slice(),
    };
    // Recurse: the remainder of the chain might contain a *further*
    // violation (e.g. two Subslice projections chained together).
    desugar_place(ctx, body, &rest_place, prelude, span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::primitives::IntTy;
    use glyim_mir::BorrowKind;
    use glyim_type::{Region, TyCtxMut, TyKind};

    use crate::tests::testutil::build_test_body;

    /// Build a `&[i32]` local and a single-block body whose block-0 terminator is
    /// `Return`. Returns (ctx, body, the local index).
    fn body_with_ref_slice(ctx: &mut TyCtxMut) -> (Body, LocalIdx) {
        let elem = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let slice_ty = ctx.mk_ty(TyKind::Slice(elem));
        let ref_slice_ty = ctx.mk_ref(Region::Erased, slice_ty, Mutability::Not);
        let local = LocalIdx::from_raw(1);
        let body = build_test_body(
            vec![
                (ctx.unit_ty(), Mutability::Not),
                (ref_slice_ty, Mutability::Not),
            ],
            vec![BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            }],
            0,
            ctx.unit_ty(),
        );
        (body, local)
    }

    fn count_storage_live(body: &Body) -> usize {
        body.basic_blocks
            .iter()
            .flat_map(|b| b.statements.iter())
            .filter(|s| matches!(s.kind, StatementKind::StorageLive(_)))
            .count()
    }

    #[test]
    fn terminal_subslice_is_unchanged() {
        // A place whose only Subslice is already the last element must be left
        // completely untouched (no new locals, no new statements).
        let mut ctx_mut = glyim_test::test_ty_ctx();
        let (mut body, _) = body_with_ref_slice(&mut ctx_mut);
        let ctx = ctx_mut.freeze();
        let n_locals_before = body.locals.len();
        let n_storage_before = count_storage_live(&body);

        // Statement: `_2 = move (*_1)[1..]` — Subslice is terminal here.
        let place = Place {
            local: LocalIdx::from_raw(1),
            projection: Box::new([
                ProjectionElem::Deref,
                ProjectionElem::Subslice {
                    from: 1,
                    to: 2,
                    from_end: false,
                },
            ]),
        };
        body.basic_blocks[BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Move(place)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });

        crate::slice_desugar::run(&ctx, &mut body);

        assert_eq!(body.locals.len(), n_locals_before, "no new locals expected");
        assert_eq!(
            count_storage_live(&body),
            n_storage_before,
            "no StorageLive expected for terminal subslice"
        );
    }

    #[test]
    fn non_terminal_subslice_is_split() {
        // A place with Subslice NOT last (`(*_1)[1..]` followed by Deref) must be
        // split: a temporary is materialized and the remainder continues from it.
        let mut ctx_mut = glyim_test::test_ty_ctx();
        let (mut body, _) = body_with_ref_slice(&mut ctx_mut);
        let ctx = ctx_mut.freeze();
        let n_locals_before = body.locals.len();
        let n_storage_before = count_storage_live(&body);

        // Statement: `_2 = move (*_1)[1..][0]` — Subslice is NOT terminal.
        let place = Place {
            local: LocalIdx::from_raw(1),
            projection: Box::new([
                ProjectionElem::Deref,
                ProjectionElem::Subslice {
                    from: 1,
                    to: 2,
                    from_end: false,
                },
                ProjectionElem::Index(LocalIdx::from_raw(0)),
            ]),
        };
        body.basic_blocks[BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Move(place)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });

        crate::slice_desugar::run(&ctx, &mut body);

        // A new temporary must have been introduced.
        assert_eq!(
            body.locals.len(),
            n_locals_before + 1,
            "expected exactly one new temporary local"
        );
        assert_eq!(
            count_storage_live(&body),
            n_storage_before + 1,
            "expected one StorageLive for the materialized subslice"
        );
        // The original statement's destination place must now root at the temp,
        // and the subslice projection must appear only once (in the prelude assign).
        let stmt = &body.basic_blocks[BasicBlockIdx::from_raw(0)].statements;
        let assign = stmt
            .iter()
            .find(|s| matches!(s.kind, StatementKind::Assign(_, _)))
            .unwrap();
        if let StatementKind::Assign(dst, _) = &assign.kind {
            assert!(
                dst.projection
                    .iter()
                    .all(|e| { !matches!(e, ProjectionElem::Subslice { .. }) }),
                "RHS subslice must have been moved into the prelude; destination has no subslice"
            );
        }
    }

    #[test]
    fn no_slice_projection_is_untouched() {
        // A body with no ConstantIndex/Subslice at all must be a complete no-op.
        let mut ctx_mut = glyim_test::test_ty_ctx();
        let (mut body, _) = body_with_ref_slice(&mut ctx_mut);
        let ctx = ctx_mut.freeze();
        let n_locals_before = body.locals.len();
        let n_storage_before = count_storage_live(&body);

        // Add a plain copy statement with no slice projection at all.
        body.basic_blocks[BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });

        crate::slice_desugar::run(&ctx, &mut body);

        assert_eq!(body.locals.len(), n_locals_before);
        assert_eq!(count_storage_live(&body), n_storage_before);
    }

    #[test]
    fn subslice_in_rvalue_ref_operand_is_split() {
        // A Subslice appearing inside an `Rvalue::Ref`'s operand (e.g. taking a
        // reference to a subslice) must also be desugared on the operand side.
        let mut ctx_mut = glyim_test::test_ty_ctx();
        let (mut body, _) = body_with_ref_slice(&mut ctx_mut);
        let ctx = ctx_mut.freeze();
        let n_locals_before = body.locals.len();

        // `_2 = &(*_1)[1..][0]` — Subslice inside the Ref operand, not terminal.
        let place = Place {
            local: LocalIdx::from_raw(1),
            projection: Box::new([
                ProjectionElem::Deref,
                ProjectionElem::Subslice {
                    from: 1,
                    to: 2,
                    from_end: false,
                },
                ProjectionElem::Index(LocalIdx::from_raw(0)),
            ]),
        };
        body.basic_blocks[BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Ref(place, BorrowKind::Shared),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });

        crate::slice_desugar::run(&ctx, &mut body);

        assert_eq!(
            body.locals.len(),
            n_locals_before + 1,
            "expected a temporary for the operand-side subslice"
        );
    }
}
