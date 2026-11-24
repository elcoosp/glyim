//! MIR well-formedness validator (M5.5): the single semantic oracle for MIR.
//!
//! This is the *unified* validator the plan (§6 M5.5) calls for. Before it,
//! two validators lived in the tree: `glyim-mir` checked only
//! local-declaredness, and `glyim-opt` checked a few structural invariants
//! (CFG edges, drop glue, slice-projection terminality). Splitting the
//! invariant set across two crates duplicated the traversal logic and left
//! the optimizer's validator as the only place a "is this body ok?" question
//! could be answered — so the M4 property oracle and borrowck each had to
//! pick one. This module is now the one place that question is asked, and
//! `glyim-opt` re-exports it.
//!
//! # Contract
//!
//! [`validate_body`] returns `Result<(), Vec<Violation>>`: `Ok(())` when the
//! body satisfies every always-on invariant, or every violation found (in
//! deterministic block/statement order) otherwise. It **never panics** —
//! every index lookup is bounds-checked, and the drop-glue check (which reads
//! a place's type) is skipped for places whose local is undeclared so the
//! type lookup cannot index out of bounds. This is the M5.10 direction:
//! validators report typed violations instead of aborting.
//!
//! [`validate_no_subslice`] is the phase-specific post-condition for
//! `slice_desugar`: once that pass has run, *any* surviving `Subslice`
//! projection (in *any* rvalue operand, including `Aggregate`/`Cast`/`Repeat`
//! — the gap the audit flagged as MIR-31) is a compiler bug. It is the
//! post-condition arm of the unified validator, run by the pass manager
//! after the pass that [`glyim_opt::MirPass::ensures`] declares
//! `NoSubslice` for.
//!
//! # Invariant families
//!
//! 1. **CFG edge targets exist** — every `Goto`/`Drop`/`Assert`/`SwitchInt`/
//!    `Call` target (and every cleanup edge) references a block that exists.
//!    Catches the MIR-7 / MIR-10 class (`remap_terminator` / drop
//!    elaboration retargeting to a deleted block).
//! 2. **def-before-use / declaredness on locals** — every `LocalIdx` a
//!    statement or terminator references is declared in `body.locals`. This
//!    is the declaredness arm; flow-sensitive dominance (a use dominated by
//!    its `StorageLive`) is deferred to the M10.1 dataflow framework, which
//!    can build the dominator tree the flow-sensitive check needs.
//! 3. **`StorageLive`/`StorageDead` pairing** — a `StorageLive` of a local
//!    already live in the same block is a double-live bug; a `StorageDead` of
//!    a non-argument local that is never `StorageLive`d anywhere in the body
//!    is a dead-of-uninitialised bug (the MIR-6 class: DCE deleted the only
//!    init of a dropped local).
//! 4. **call/aggregate arity vs declarations** — `body.arg_count` must leave
//!    room for the return place: `locals.len() >= arg_count + 1`. This is the
//!    MIR-30 class (`Body::args` slices `[1..=arg_count]` without a length
//!    check and panics on an over-large `arg_count`). Per-signature call /
//!    per-variant aggregate arity needs the M6.4 body queries and is not
//!    checked here.
//! 5. **cleanup-edge validity** — a cleanup block (`is_cleanup == true`)
//!    must not itself carry a cleanup edge: cleanup does not unwind. Plus
//!    every cleanup target exists (family 1 reports the missing-block arm).
//! 6. **drop-flag / drop-glue consistency** — a `Drop` terminator whose
//!    place type needs no drop glue is a compiler bug (the MIR-22 class:
//!    dropping a non-droppable value). The place type is read only when the
//!    local is declared, so the lookup cannot panic.
//! 7. **phase-specific post-conditions** — `ConstantIndex`/`Subslice`
//!    projections must be terminal (always-on); no `Subslice` survives
//!    `slice_desugar` (post-condition arm, MIR-31).

use crate::{Body, LocalIdx, Operand, Place, Rvalue, StatementKind, TerminatorKind};
use glyim_span::Span;
use glyim_type::TyCtx;

/// Where a violation was found, for actionable diagnostics. The site is the
/// block (and, for statements, the statement index) the validator was
/// walking when it found the violation, plus a body-level site for
/// whole-body invariants like `arg_count`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationSite {
    /// A whole-body invariant (e.g. `arg_count` vs `locals`).
    Body,
    /// A block-level invariant (e.g. a cleanup edge on a cleanup block).
    Block(crate::BasicBlockIdx),
    /// A statement at `index` within `block`.
    Statement {
        /// The block the statement lives in.
        block: crate::BasicBlockIdx,
        /// The statement's position within `block.statements`.
        index: usize,
    },
    /// A block's terminator.
    Terminator(crate::BasicBlockIdx),
}

/// Where an offending local is referenced within a statement or terminator,
/// used by the declaredness arm so the diagnostic names the offending use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorSite {
    /// A `StorageLive`/`StorageDead` statement.
    StorageStmt,
    /// The LHS place of an `Assign` statement.
    AssignPlace,
    /// A place inside an operand (`Copy`/`Move`) or an RHS place (`Ref`/…).
    Operand,
    /// A place in a terminator (`Call` destination, `Drop` place, …).
    Terminator,
}

/// The invariant a single violation broke. Variants carry the data a
/// diagnostic or test needs to name the offending element (block id, local
/// index, place, type). Adding a variant is a breaking change every
/// exhaustive match (the validator itself, codegen, the pass manager) must
/// absorb — by construction, the only exhaustive matches are in this crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViolationKind {
    /// A terminator targets a basic block that does not exist (family 1).
    /// Covers `Goto`/`Drop`/`Assert`/`SwitchInt`/`Call` targets and cleanup
    /// edges — the MIR-7 / MIR-10 / MIR-15 class.
    UnknownTarget(crate::BasicBlockIdx),
    /// A statement or terminator referenced a `LocalIdx` not declared in
    /// `body.locals` (family 2, the declaredness arm).
    UndeclaredLocal {
        /// The offending local index (`>= body.locals.len()`).
        local: LocalIdx,
        /// Where the offending reference lives.
        use_site: ErrorSite,
    },
    /// `StorageLive` of a local already live in the same block (family 3).
    DoubleStorageLive(LocalIdx),
    /// `StorageDead` of a non-argument local that is never `StorageLive`d
    /// anywhere in the body (family 3, the MIR-6 class).
    StorageDeadWithoutLive(LocalIdx),
    /// `body.arg_count` leaves no room for the return place (family 4, the
    /// MIR-30 class: `Body::args` slices `[1..=arg_count]` without a check).
    ArgCountExceedsLocals {
        /// The body's declared argument count.
        arg_count: usize,
        /// The number of declared locals (`locals.len()`).
        locals: usize,
    },
    /// A cleanup block carries a cleanup edge — cleanup does not unwind
    /// (family 5).
    CleanupEdgeOnCleanupBlock,
    /// A `Drop` terminator whose place type needs no drop glue (family 6,
    /// the MIR-22 class).
    UnnecessaryDrop {
        /// The place the `Drop` terminator targets.
        place: Place,
        /// The (drop-glue-free) type of that place.
        ty: glyim_type::Ty,
    },
    /// A `ConstantIndex`/`Subslice` projection appears mid-chain, not as the
    /// final element of its place's projection list (family 7, always-on).
    NonTerminalSliceProjection,
    /// A `ProjectionElem::Subslice` survives `slice_desugar` (family 7,
    /// post-condition; MIR-31). Scanned across *every* rvalue operand,
    /// including `Aggregate`/`Cast`/`Repeat`.
    SubsliceAfterDesugar,
}

/// A single well-formedness violation found by [`validate_body`] or
/// [`validate_no_subslice`]: what invariant broke, where, and the span of the
/// offending MIR so the rendered diagnostic points at the lowering that
/// produced the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// Which invariant was violated.
    pub kind: ViolationKind,
    /// Where the violation was found.
    pub site: ViolationSite,
    /// Source span of the offending statement/terminator (or `body.span` for
    /// whole-body invariants).
    pub span: Span,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ViolationKind::UnknownTarget(bb) => {
                write!(f, "MIR validation: terminator targets unknown block {bb:?}")
            }
            ViolationKind::UndeclaredLocal { local, use_site } => {
                write!(
                    f,
                    "MIR validation: reference to undeclared local {local:?} at {use_site:?}"
                )
            }
            ViolationKind::DoubleStorageLive(local) => {
                write!(f, "MIR validation: double StorageLive of {local:?}")
            }
            ViolationKind::StorageDeadWithoutLive(local) => {
                write!(
                    f,
                    "MIR validation: StorageDead of {local:?} with no StorageLive anywhere in the body"
                )
            }
            ViolationKind::ArgCountExceedsLocals { arg_count, locals } => {
                write!(
                    f,
                    "MIR validation: arg_count {arg_count} exceeds declared locals {locals} (no room for the return place)"
                )
            }
            ViolationKind::CleanupEdgeOnCleanupBlock => {
                write!(f, "MIR validation: cleanup block carries a cleanup edge")
            }
            ViolationKind::UnnecessaryDrop { place, ty } => {
                write!(
                    f,
                    "MIR validation: Drop terminator on a type that needs no drop glue: place {place:?} (ty {ty:?})"
                )
            }
            ViolationKind::NonTerminalSliceProjection => {
                write!(
                    f,
                    "MIR validation: ConstantIndex/Subslice projection is not terminal"
                )
            }
            ViolationKind::SubsliceAfterDesugar => {
                write!(
                    f,
                    "MIR validation: Subslice projection survived slice_desugar (compiler bug)"
                )
            }
        }
    }
}

/// A local-use/declaration mismatch found by [`check_local_declaredness`]:
/// the back-compat single-violation view of the declaredness arm, kept so the
/// M4 property oracle ([`glyim_test::property::mir`]) and existing tests that
/// ask "is this one local declared?" need no change. The unified
/// [`validate_body`] collects the same information as `Vec<Violation>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDeclarednessError {
    /// The offending local index (`>= body.locals.len()`).
    pub local: LocalIdx,
    /// Where the offending reference lives.
    pub site: ErrorSite,
    /// Span of the offending statement/terminator.
    pub span: Span,
}

/// Validate `body` against every always-on invariant (families 1–6 plus the
/// terminality slice of family 7). Returns `Ok(())` if every invariant
/// holds, otherwise every violation found in deterministic block/statement
/// order. Never panics.
///
/// This does *not* run the `Subslice`-after-`slice_desugar` post-condition —
/// that is legal before `slice_desugar` runs, so it is a phase-specific
/// check, [`validate_no_subslice`], run by the pass manager after the pass
/// that establishes it.
pub fn validate_body(ctx: &TyCtx, body: &Body) -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();
    collect_arg_count(body, &mut violations);
    collect_cfg_and_cleanup(body, &mut violations);
    collect_declaredness(body, &mut violations);
    collect_storage_pairing(body, &mut violations);
    collect_drop_glue(ctx, body, &mut violations);
    collect_slice_terminality(body, &mut violations);
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Assert that no `ProjectionElem::Subslice` survives in `body` (family 7
/// post-condition). Scans *every* rvalue operand — `Use`/`Cast`/`Repeat`/
/// `UnaryOp`/`BinaryOp`/`Aggregate`/`Ref`/`Discriminant`/`Len` — and every
/// place a terminator reads (`Call` destination, `Drop` place, `SwitchInt`
/// discriminant, `Assert` condition). The prior validator missed
/// `Subslice` inside `Aggregate`/`Cast`/`Repeat` operands (audit MIR-31);
/// this traversal closes that gap.
pub fn validate_no_subslice(body: &Body) -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        for (idx, stmt) in data.statements.iter().enumerate() {
            let site = ViolationSite::Statement {
                block: bb,
                index: idx,
            };
            if let StatementKind::Assign(_, rvalue) = &stmt.kind {
                scan_rvalue_places(rvalue, &mut |place| {
                    if place
                        .projection
                        .iter()
                        .any(|e| matches!(e, crate::ProjectionElem::Subslice { .. }))
                    {
                        violations.push(Violation {
                            kind: ViolationKind::SubsliceAfterDesugar,
                            site,
                            span: stmt.source_info.span,
                        });
                    }
                });
            }
        }
        let term_site = ViolationSite::Terminator(bb);
        scan_terminator_places(&data.terminator.kind, &mut |place| {
            if place
                .projection
                .iter()
                .any(|e| matches!(e, crate::ProjectionElem::Subslice { .. }))
            {
                violations.push(Violation {
                    kind: ViolationKind::SubsliceAfterDesugar,
                    site: term_site,
                    span: data.terminator.source_info.span,
                });
            }
        });
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Check that every `LocalIdx` referenced by `body`'s statements and
/// terminators is declared in `body.locals`. Returns `Ok(())` if the body is
/// well-formed, or the first offending local otherwise (back-compat
/// single-violation view; the unified [`validate_body`] collects all). Never
/// panics. Kept as the public entry the M4 property oracle and existing tests
/// call — it is the declaredness arm, not a second implementation.
pub fn check_local_declaredness(body: &Body) -> Result<(), LocalDeclarednessError> {
    for (_bb, data) in body.basic_blocks.iter_enumerated() {
        for stmt in &data.statements {
            for (local, site) in locals_in_statement(&stmt.kind) {
                if local.index() >= body.locals.len() {
                    return Err(LocalDeclarednessError {
                        local,
                        site,
                        span: stmt.source_info.span,
                    });
                }
            }
        }
        for (local, site) in locals_in_terminator(&data.terminator.kind) {
            if local.index() >= body.locals.len() {
                return Err(LocalDeclarednessError {
                    local,
                    site,
                    span: data.terminator.source_info.span,
                });
            }
        }
    }
    Ok(())
}

// ---- invariant collectors (each pushes to `out` in deterministic order) ----

/// Family 4: `body.arg_count` must leave room for the return place.
fn collect_arg_count(body: &Body, out: &mut Vec<Violation>) {
    // `Body::args` slices `&self.locals.as_slice()[1..=self.arg_count]`, so an
    // `arg_count` with no room for local 0 (the return place) panics on args
    // access. `arg_count + 1 > locals.len()` is the exact condition.
    if body.arg_count + 1 > body.locals.len() {
        out.push(Violation {
            kind: ViolationKind::ArgCountExceedsLocals {
                arg_count: body.arg_count,
                locals: body.locals.len(),
            },
            site: ViolationSite::Body,
            span: body.span,
        });
    }
}

/// Families 1 and 5: every terminator target (and cleanup edge) exists, and a
/// cleanup block carries no cleanup edge.
fn collect_cfg_and_cleanup(body: &Body, out: &mut Vec<Violation>) {
    let n_blocks = body.basic_blocks.len();
    let block_exists = |bb: crate::BasicBlockIdx| (bb.to_raw() as usize) < n_blocks;

    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let span = data.terminator.source_info.span;
        let term_site = ViolationSite::Terminator(bb);
        // Family 5: a cleanup block must not carry a cleanup edge.
        if data.is_cleanup && terminator_has_cleanup(&data.terminator.kind) {
            out.push(Violation {
                kind: ViolationKind::CleanupEdgeOnCleanupBlock,
                site: term_site,
                span,
            });
        }
        // Family 1: every target (and cleanup edge) references an existing block.
        for target in terminator_targets(&data.terminator.kind) {
            if !block_exists(target) {
                out.push(Violation {
                    kind: ViolationKind::UnknownTarget(target),
                    site: term_site,
                    span,
                });
            }
        }
    }
}

/// Family 2: every referenced `LocalIdx` is declared.
fn collect_declaredness(body: &Body, out: &mut Vec<Violation>) {
    let n_locals = body.locals.len();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        for (idx, stmt) in data.statements.iter().enumerate() {
            let site = ViolationSite::Statement {
                block: bb,
                index: idx,
            };
            for (local, use_site) in locals_in_statement(&stmt.kind) {
                if local.index() >= n_locals {
                    out.push(Violation {
                        kind: ViolationKind::UndeclaredLocal { local, use_site },
                        site,
                        span: stmt.source_info.span,
                    });
                }
            }
        }
        let term_site = ViolationSite::Terminator(bb);
        for (local, use_site) in locals_in_terminator(&data.terminator.kind) {
            if local.index() >= n_locals {
                out.push(Violation {
                    kind: ViolationKind::UndeclaredLocal { local, use_site },
                    site: term_site,
                    span: data.terminator.source_info.span,
                });
            }
        }
    }
}

/// Family 3: `StorageLive`/`StorageDead` pairing. A `StorageLive` of a local
/// already live in the same block is a double-live bug; a `StorageDead` of a
/// non-argument local that is never `StorageLive`d anywhere in the body is a
/// dead-of-uninitialised bug.
fn collect_storage_pairing(body: &Body, out: &mut Vec<Violation>) {
    let n_locals = body.locals.len();
    // Non-argument locals are `arg_count + 1 ..` (local 0 is the return place,
    // locals 1..=arg_count are arguments, live at entry without StorageLive).
    let first_non_arg = body.arg_count + 1;
    // First pass: which non-arg locals are ever StorageLive'd anywhere?
    let mut ever_lived = vec![false; n_locals];
    for data in body.basic_blocks.iter() {
        for stmt in &data.statements {
            if let StatementKind::StorageLive(local) = stmt.kind {
                let i = local.index();
                if i < n_locals {
                    ever_lived[i] = true;
                }
            }
        }
    }
    // Second pass: per-block double-live, and body-wide dead-without-live.
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let mut live_in_block = vec![false; n_locals];
        for (idx, stmt) in data.statements.iter().enumerate() {
            let site = ViolationSite::Statement {
                block: bb,
                index: idx,
            };
            match stmt.kind {
                StatementKind::StorageLive(local) => {
                    let i = local.index();
                    if i < n_locals {
                        if live_in_block[i] {
                            out.push(Violation {
                                kind: ViolationKind::DoubleStorageLive(local),
                                site,
                                span: stmt.source_info.span,
                            });
                        } else {
                            live_in_block[i] = true;
                        }
                    }
                }
                StatementKind::StorageDead(local) => {
                    let i = local.index();
                    if i < n_locals {
                        // Reset intra-block liveness so a later StorageLive is
                        // not a double-live.
                        live_in_block[i] = false;
                        // A non-argument local that is never StorageLive'd
                        // anywhere cannot be legitimately StorageDead.
                        if i >= first_non_arg && !ever_lived[i] {
                            out.push(Violation {
                                kind: ViolationKind::StorageDeadWithoutLive(local),
                                site,
                                span: stmt.source_info.span,
                            });
                        }
                    }
                }
                StatementKind::Nop | StatementKind::Assign(_, _) => {}
            }
        }
    }
}

/// Family 6: a `Drop` terminator whose place type needs no drop glue. The
/// place type is read only when the local is declared, so the type lookup
/// cannot index out of bounds.
fn collect_drop_glue(ctx: &TyCtx, body: &Body, out: &mut Vec<Violation>) {
    let n_locals = body.locals.len();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if let TerminatorKind::Drop { place, .. } = &data.terminator.kind {
            // Skip places whose local is undeclared — declaredness already
            // reported them, and `place.ty` would index out of bounds.
            if place.local.index() >= n_locals {
                continue;
            }
            let ty = place.ty(ctx, &body.locals);
            if !ctx.needs_drop(ty) {
                out.push(Violation {
                    kind: ViolationKind::UnnecessaryDrop {
                        place: place.clone(),
                        ty,
                    },
                    site: ViolationSite::Terminator(bb),
                    span: data.terminator.source_info.span,
                });
            }
        }
    }
}

/// Family 7 (terminality slice): a `ConstantIndex`/`Subslice` projection
/// must be the final element of its place's projection list. Always-on (the
/// post-condition arm in [`validate_no_subslice`] adds the "no `Subslice` at
/// all" check).
fn collect_slice_terminality(body: &Body, out: &mut Vec<Violation>) {
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        for (idx, stmt) in data.statements.iter().enumerate() {
            let site = ViolationSite::Statement {
                block: bb,
                index: idx,
            };
            if let StatementKind::Assign(_, rvalue) = &stmt.kind {
                scan_rvalue_places(rvalue, &mut |place| {
                    let proj = &place.projection;
                    for (i, elem) in proj.iter().enumerate() {
                        let is_slice = matches!(
                            elem,
                            crate::ProjectionElem::ConstantIndex { .. }
                                | crate::ProjectionElem::Subslice { .. }
                        );
                        if is_slice && i + 1 != proj.len() {
                            out.push(Violation {
                                kind: ViolationKind::NonTerminalSliceProjection,
                                site,
                                span: stmt.source_info.span,
                            });
                        }
                    }
                });
            }
        }
    }
}

// ---- traversal helpers ----

/// Enumerate every `LocalIdx` a statement references, tagged with the site.
fn locals_in_statement(kind: &StatementKind) -> Vec<(LocalIdx, ErrorSite)> {
    let mut out = Vec::new();
    match kind {
        StatementKind::Nop => {}
        StatementKind::StorageLive(local) | StatementKind::StorageDead(local) => {
            out.push((*local, ErrorSite::StorageStmt));
        }
        StatementKind::Assign(place, rvalue) => {
            out.push((place.local, ErrorSite::AssignPlace));
            out.extend(locals_in_rvalue(rvalue));
        }
    }
    out
}

/// Enumerate every `LocalIdx` an rvalue references via its operands/places.
fn locals_in_rvalue(rvalue: &Rvalue) -> Vec<(LocalIdx, ErrorSite)> {
    let mut out = Vec::new();
    match rvalue {
        Rvalue::Use(op)
        | Rvalue::Cast(_, op, _)
        | Rvalue::Repeat(op, _)
        | Rvalue::UnaryOp(_, op) => {
            out.extend(locals_in_operand(op));
        }
        Rvalue::Ref(place, _) | Rvalue::Discriminant(place) | Rvalue::Len(place) => {
            out.push((place.local, ErrorSite::Operand));
        }
        Rvalue::BinaryOp(_, ops) => {
            out.extend(locals_in_operand(&ops[0]));
            out.extend(locals_in_operand(&ops[1]));
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                out.extend(locals_in_operand(op));
            }
        }
    }
    out
}

/// Enumerate every `LocalIdx` an operand references (via its place). Also
/// surfaces the `Index` projection's local (a `Place { local, [Index(i)] }`
/// reads local `i` as the index value).
fn locals_in_operand(op: &Operand) -> Vec<(LocalIdx, ErrorSite)> {
    let mut out = Vec::new();
    match op {
        Operand::Copy(place) | Operand::Move(place) => {
            out.push((place.local, ErrorSite::Operand));
            for elem in place.projection.iter() {
                if let crate::ProjectionElem::Index(idx_local) = elem {
                    out.push((*idx_local, ErrorSite::Operand));
                }
            }
        }
        Operand::Constant(_) => {}
    }
    out
}

/// Enumerate every `LocalIdx` a terminator references via its
/// operands/places.
fn locals_in_terminator(kind: &TerminatorKind) -> Vec<(LocalIdx, ErrorSite)> {
    let mut out = Vec::new();
    match kind {
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
        TerminatorKind::SwitchInt { discr, .. } => out.extend(locals_in_operand(discr)),
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            out.extend(locals_in_operand(func));
            for arg in args {
                out.extend(locals_in_operand(arg));
            }
            out.push((destination.local, ErrorSite::Terminator));
        }
        TerminatorKind::Assert { cond, .. } => out.extend(locals_in_operand(cond)),
        TerminatorKind::Drop { place, .. } => {
            out.push((place.local, ErrorSite::Terminator));
            for elem in place.projection.iter() {
                if let crate::ProjectionElem::Index(idx_local) = elem {
                    out.push((*idx_local, ErrorSite::Terminator));
                }
            }
        }
    }
    out
}

/// Every basic-block target a terminator references (normal + cleanup), for
/// the CFG edge existence check.
fn terminator_targets(kind: &TerminatorKind) -> Vec<crate::BasicBlockIdx> {
    let mut out = Vec::new();
    match kind {
        TerminatorKind::Goto { target } => out.push(*target),
        TerminatorKind::Drop {
            target, cleanup, ..
        }
        | TerminatorKind::Assert {
            target, cleanup, ..
        } => {
            out.push(*target);
            if let Some(c) = cleanup {
                out.push(*c);
            }
        }
        TerminatorKind::SwitchInt { targets, .. } => {
            out.push(targets.otherwise());
            out.extend(targets.iter().map(|(_, t)| t));
        }
        TerminatorKind::Call {
            target, cleanup, ..
        } => {
            if let Some(t) = target {
                out.push(*t);
            }
            if let Some(c) = cleanup {
                out.push(*c);
            }
        }
        TerminatorKind::Return | TerminatorKind::Unreachable => {}
    }
    out
}

/// Whether a terminator carries a cleanup edge (for the cleanup-on-cleanup
/// check).
fn terminator_has_cleanup(kind: &TerminatorKind) -> bool {
    matches!(
        kind,
        TerminatorKind::Drop {
            cleanup: Some(_),
            ..
        } | TerminatorKind::Assert {
            cleanup: Some(_),
            ..
        } | TerminatorKind::Call {
            cleanup: Some(_),
            ..
        }
    )
}

/// Walk every place an rvalue reads/writes, including places nested inside
/// `Aggregate`/`Cast`/`Repeat`/`BinaryOp` operands. The closure receives
/// each place by reference. This is the traversal the MIR-31 fix added: the
/// prior `rvalue_place` helper returned only the *first* place of a few
/// rvalue kinds, so a `Subslice` (or non-terminal slice projection) inside an
/// `Aggregate`/`Cast`/`Repeat` operand was invisible.
fn scan_rvalue_places(rvalue: &Rvalue, on_place: &mut impl FnMut(&Place)) {
    let mut visit = |op: &Operand| {
        if let Operand::Copy(p) | Operand::Move(p) = op {
            on_place(p);
        }
    };
    match rvalue {
        Rvalue::Use(op)
        | Rvalue::UnaryOp(_, op)
        | Rvalue::Cast(_, op, _)
        | Rvalue::Repeat(op, _) => {
            visit(op);
        }
        Rvalue::Ref(p, _) => on_place(p),
        Rvalue::BinaryOp(_, ops) => {
            visit(&ops[0]);
            visit(&ops[1]);
        }
        Rvalue::Aggregate(_, operands) => {
            for op in operands {
                visit(op);
            }
        }
        Rvalue::Discriminant(p) | Rvalue::Len(p) => on_place(p),
    }
}

/// Walk every place a terminator reads (the `Call` destination, `Drop` place,
/// `SwitchInt` discriminant operand, `Assert` condition operand).
fn scan_terminator_places(kind: &TerminatorKind, on_place: &mut impl FnMut(&Place)) {
    let mut visit_op = |op: &Operand| {
        if let Operand::Copy(p) | Operand::Move(p) = op {
            on_place(p);
        }
    };
    match kind {
        TerminatorKind::SwitchInt { discr, .. } => visit_op(discr),
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            visit_op(func);
            for arg in args {
                visit_op(arg);
            }
            on_place(destination);
        }
        TerminatorKind::Assert { cond, .. } => visit_op(cond),
        TerminatorKind::Drop { place, .. } => on_place(place),
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
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

    fn i32_body() -> (glyim_type::TyCtx, Body) {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        (ctx, empty_body(i32_ty, i32_ty))
    }

    // ---- historical bug-class tests (audit MIR-* rows) ----

    /// MIR-7 / MIR-10: a `remap_terminator`/drop-elaboration retargeting to a
    /// deleted block. The validator must flag the unknown target.
    #[test]
    fn flags_terminator_to_missing_block_mir7_mir10() {
        let (ctx, mut body) = i32_body();
        body.basic_blocks[crate::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Goto {
                target: crate::BasicBlockIdx::from_raw(99),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let errs = validate_body(&ctx, &body).expect_err("missing block must be flagged");
        assert!(errs.iter().any(|v| matches!(
            v.kind,
            ViolationKind::UnknownTarget(t) if t.to_raw() == 99
        )));
    }

    /// MIR-15: host panic after `Call { target: None }` in the last block /
    /// a cleanup edge to a missing block. The validator flags the missing
    /// cleanup target.
    #[test]
    fn flags_call_cleanup_to_missing_block_mir15() {
        let (ctx, mut body) = i32_body();
        body.basic_blocks[crate::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0),
                    ty: ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32)),
                    span: Span::DUMMY,
                }),
                args: Vec::new(),
                destination: Place::new(crate::LocalIdx::from_raw(0)),
                target: None,
                cleanup: Some(crate::BasicBlockIdx::from_raw(99)),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let errs = validate_body(&ctx, &body).expect_err("missing cleanup target must be flagged");
        assert!(errs.iter().any(|v| matches!(
            v.kind,
            ViolationKind::UnknownTarget(t) if t.to_raw() == 99
        )));
    }

    /// MIR-28: `loans_by_dest` indexes by `dest_local` without a bounds
    /// defense. The validator flags any reference to an undeclared local.
    #[test]
    fn flags_undeclared_local_mir28() {
        let (ctx, body) = i32_body();
        let mut bad = body.clone();
        let undeclared = crate::LocalIdx::from_raw(5);
        bad.basic_blocks[crate::BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::StorageLive(undeclared),
                source_info: SourceInfo::new(Span::DUMMY),
            });
        let errs = validate_body(&ctx, &bad).expect_err("undeclared local must be flagged");
        assert!(errs.iter().any(|v| matches!(
            v.kind,
            ViolationKind::UndeclaredLocal { local, .. } if local.to_raw() == 5
        )));
        // The back-compat declaredness entry catches it too.
        assert!(check_local_declaredness(&bad).is_err());
    }

    /// MIR-30: `Body::args` slices `[1..=arg_count]` without a length check.
    /// An `arg_count` with no room for the return place panics on args
    /// access; the validator flags it as `ArgCountExceedsLocals`.
    #[test]
    fn flags_arg_count_exceeds_locals_mir30() {
        let (ctx, mut body) = i32_body();
        // 1 declared local (the return place), but claim 3 args.
        body.arg_count = 3;
        let errs = validate_body(&ctx, &body).expect_err("arg_count must be flagged");
        assert!(errs.iter().any(|v| matches!(
            v.kind,
            ViolationKind::ArgCountExceedsLocals { arg_count, locals }
            if arg_count == 3 && locals == 1
        )));
    }

    /// MIR-6: DCE deleted the only `StorageLive` of a dropped local, leaving a
    /// `StorageDead` of a never-live'd local. The validator flags
    /// `StorageDeadWithoutLive`.
    #[test]
    fn flags_storage_dead_without_live_mir6() {
        let (ctx, mut body) = i32_body();
        // Add a non-arg local (index 1) and StorageDead it without ever
        // StorageLive'ing it anywhere.
        body.locals.push(LocalDecl {
            ty: ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32)),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        body.basic_blocks[crate::BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::StorageDead(crate::LocalIdx::from_raw(1)),
                source_info: SourceInfo::new(Span::DUMMY),
            });
        let errs = validate_body(&ctx, &body).expect_err("dead-without-live must be flagged");
        assert!(errs.iter().any(|v| matches!(
            v.kind,
            ViolationKind::StorageDeadWithoutLive(l) if l.to_raw() == 1
        )));
    }

    /// Storage pairing (double-live): two `StorageLive` of the same local in
    /// the same block without an intervening `StorageDead`.
    #[test]
    fn flags_double_storage_live() {
        let (ctx, mut body) = i32_body();
        body.locals.push(LocalDecl {
            ty: ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32)),
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let bb0 = crate::BasicBlockIdx::from_raw(0);
        let local = crate::LocalIdx::from_raw(1);
        body.basic_blocks[bb0].statements.push(Statement {
            kind: StatementKind::StorageLive(local),
            source_info: SourceInfo::new(Span::DUMMY),
        });
        body.basic_blocks[bb0].statements.push(Statement {
            kind: StatementKind::StorageLive(local),
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let errs = validate_body(&ctx, &body).expect_err("double-live must be flagged");
        assert!(errs.iter().any(|v| matches!(
            v.kind,
            ViolationKind::DoubleStorageLive(l) if l.to_raw() == 1
        )));
    }

    /// MIR-22: a `Drop` terminator on a non-droppable type (i32) is a compiler
    /// bug. The validator flags `UnnecessaryDrop`.
    #[test]
    fn flags_drop_on_non_droppable_type_mir22() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let mut body = empty_body(i32_ty, i32_ty);
        body.basic_blocks[crate::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(crate::LocalIdx::from_raw(0)),
                target: crate::BasicBlockIdx::from_raw(0),
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let errs = validate_body(&ctx, &body).expect_err("Drop on i32 must be flagged");
        assert!(
            errs.iter()
                .any(|v| matches!(v.kind, ViolationKind::UnnecessaryDrop { .. }))
        );
    }

    /// MIR-22 partner: a `Drop` on a droppable type (String) is allowed.
    #[test]
    fn allows_drop_on_droppable_type() {
        let (ctx, str_ty) = with_fresh_ty_ctx(|c| c.mk_ty(glyim_type::TyKind::String));
        let mut body = empty_body(str_ty, str_ty);
        body.basic_blocks[crate::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(crate::LocalIdx::from_raw(0)),
                target: crate::BasicBlockIdx::from_raw(0),
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        assert!(validate_body(&ctx, &body).is_ok());
    }

    /// MIR-31: `validate_no_subslice` missed `Subslice` inside
    /// `Aggregate`/`Cast`/`Repeat` operands. The unified validator scans
    /// every rvalue operand, so a `Subslice` inside an `Aggregate` operand is
    /// caught.
    #[test]
    fn flags_subslice_inside_aggregate_operand_mir31() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let mut body = empty_body(i32_ty, i32_ty);
        let subslice_place = Place {
            local: crate::LocalIdx::from_raw(0),
            projection: Box::new([crate::ProjectionElem::Subslice {
                from: 1,
                to: 2,
                from_end: false,
            }]),
        };
        body.basic_blocks[crate::BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(crate::LocalIdx::from_raw(0)),
                    Rvalue::Aggregate(
                        crate::AggregateKind::Tuple,
                        vec![Operand::Copy(subslice_place)],
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });
        let errs = validate_no_subslice(&body).expect_err("Subslice in Aggregate must be flagged");
        assert!(
            errs.iter()
                .any(|v| matches!(v.kind, ViolationKind::SubsliceAfterDesugar))
        );
        // The always-on validate_body must NOT flag a terminal Subslice
        // (it is legal before slice_desugar).
        assert!(validate_body(&ctx, &body).is_ok());
    }

    /// Family 7 terminality: a `ConstantIndex` mid-chain (not terminal) is
    /// flagged by the always-on validator.
    #[test]
    fn flags_non_terminal_slice_projection() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let mut body = empty_body(i32_ty, i32_ty);
        let place = Place {
            local: crate::LocalIdx::from_raw(0),
            projection: Box::new([
                crate::ProjectionElem::ConstantIndex {
                    offset: 0,
                    min_length: 0,
                    from_end: false,
                },
                crate::ProjectionElem::Field(glyim_type::FieldIdx::from_raw(0)),
            ]),
        };
        body.basic_blocks[crate::BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(crate::LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Copy(place)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });
        let errs =
            validate_body(&ctx, &body).expect_err("mid-chain slice projection must be flagged");
        assert!(
            errs.iter()
                .any(|v| matches!(v.kind, ViolationKind::NonTerminalSliceProjection))
        );
    }

    /// Family 5: a cleanup block carrying a cleanup edge is flagged.
    #[test]
    fn flags_cleanup_edge_on_cleanup_block() {
        let (ctx, i32_ty) = with_fresh_ty_ctx(|c| {
            c.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32))
        });
        let mut body = empty_body(i32_ty, i32_ty);
        // Two blocks: bb0 drops to bb1 (cleanup), bb1 is a cleanup block that
        // itself drops to bb0 with a cleanup edge.
        let bb0 = crate::BasicBlockIdx::from_raw(0);
        let bb1 = crate::BasicBlockIdx::from_raw(1);
        body.basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(crate::LocalIdx::from_raw(0)),
                target: bb0,
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        }));
        body.basic_blocks[bb0].is_cleanup = true;
        body.basic_blocks[bb0].terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(crate::LocalIdx::from_raw(0)),
                target: bb1,
                cleanup: Some(bb1),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let errs = validate_body(&ctx, &body).expect_err("cleanup-on-cleanup must be flagged");
        assert!(
            errs.iter()
                .any(|v| matches!(v.kind, ViolationKind::CleanupEdgeOnCleanupBlock))
        );
    }

    /// Sanity: a trivial return body passes the unified validator.
    #[test]
    fn validates_trivial_return_body() {
        let (ctx, body) = i32_body();
        assert!(validate_body(&ctx, &body).is_ok());
        assert!(validate_no_subslice(&body).is_ok());
        assert!(check_local_declaredness(&body).is_ok());
    }

    /// Never-panic guard: a body with an undeclared local in a `Drop`
    /// terminator's place must NOT panic when the drop-glue check runs (it
    /// skips undeclared locals). It reports the declaredness violation only.
    #[test]
    fn drop_on_undeclared_local_does_not_panic() {
        let (ctx, body) = i32_body();
        let mut bad = body.clone();
        bad.basic_blocks[crate::BasicBlockIdx::from_raw(0)].terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(crate::LocalIdx::from_raw(99)),
                target: crate::BasicBlockIdx::from_raw(0),
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let result = validate_body(&ctx, &bad);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        // Only declaredness violations, no panic, no drop-glue lookup.
        assert!(errs.iter().all(|v| matches!(
            v.kind,
            ViolationKind::UndeclaredLocal { .. } | ViolationKind::UnknownTarget(_)
        )));
    }
}
