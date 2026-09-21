//! Borrow checker using non-lexical lifetimes (NLL) with Polonius-style
//! region inference.
//!
//! This implementation tracks borrows across basic block boundaries using
//! a CFG-aware liveness analysis. Loans are tracked as sets, and conflicts
//! are detected between active borrows and place accesses.
//!
//! Two-phase borrow support: a `BorrowKind::Mut { allow_two_phase_borrow: true }`
//! starts in a "reservation" phase (acting as a shared borrow) and transitions
//! to "activated" when the reference is first used. During reservation, shared
//! reads and borrows of the same place are allowed; after activation, normal
//! mutable borrow rules apply.
//!
//! The analysis proceeds in three phases:
//! 1. **Loan collection**: Scan the MIR body for `Rvalue::Ref` assignments
//!    and record each as a `Loan` with the borrowed place, borrow kind,
//!    and the local holding the reference.
//! 2. **Liveness analysis**: Compute which locals are live at each program
//!    point using a standard backward dataflow analysis on the CFG.
//! 3. **Conflict detection**: For each statement, determine which loans are
//!    active (their dest local is live) and check for conflicts with
//!    place accesses in the statement. Two-phase borrows in reservation
//!    phase are treated as shared borrows for conflict purposes.

mod liveness;
pub use fixedbitset::FixedBitSet;
pub use liveness::LivenessResult;
pub use liveness::compute_liveness;
mod move_analysis;
mod twophase;
mod visitor;

use glyim_diag::{DiagSeverity, GlyimDiagnostic, MultiSpan, SubDiagnostic};
use glyim_mir::{BasicBlockIdx, Body, BorrowKind, LocalIdx, Place, Rvalue, StatementKind};
use glyim_span::Span;
use smallvec::SmallVec;
use tracing::{debug, trace};

use crate::visitor::{
    ReadVisitor, borrow_kind_label, places_conflict, walk_rvalue_reads, walk_terminator_reads,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
/// BorrowckResult.
pub struct BorrowckResult {
    /// Struct.
    pub errors: Vec<GlyimDiagnostic>,
}

/// BorrowckCtx.
pub trait BorrowckCtx {
    /// ty_ctx.
    fn ty_ctx(&self) -> &glyim_type::TyCtx;
    /// local_decl.
    fn local_decl(&self, local: LocalIdx) -> &glyim_mir::LocalDecl;
    /// is_copy.
    fn is_copy(&self, ty: glyim_type::Ty) -> bool {
        self.ty_ctx().is_copy(ty)
    }
    /// Returns a human-readable name for the local, for error messages.
    fn local_name(&self, local: LocalIdx) -> String;
}

// ---------------------------------------------------------------------------
// Loan representation
// ---------------------------------------------------------------------------

/// A loan represents a borrow of a place at a particular program point.
#[derive(Clone, Debug)]
struct Loan {
    /// The global index of this loan, used for consistent cache lookups.
    index: usize,
    /// The local that holds the reference (destination of the `Rvalue::Ref`).
    dest_local: LocalIdx,
    /// The full place that was borrowed (including projections like field access).
    borrowed_place: Place,
    /// The kind of borrow.
    kind: BorrowKind,
    /// The span for error reporting.
    span: Span,
    /// The (block, statement index) where this loan was created.
    creation_point: (BasicBlockIdx, usize),
}

/// Scan the MIR body for all `Rvalue::Ref` assignments and record loans.
fn collect_loans(body: &Body) -> Vec<Loan> {
    let mut loans = Vec::new();
    for (block_idx, block_data) in body.basic_blocks.iter_enumerated() {
        for (stmt_idx, stmt) in block_data.statements.iter().enumerate() {
            if let StatementKind::Assign(dest, Rvalue::Ref(borrowed, kind)) = &stmt.kind {
                let index = loans.len();
                loans.push(Loan {
                    index,
                    dest_local: dest.local,
                    borrowed_place: borrowed.clone(),
                    kind: *kind,
                    span: stmt.source_info.span,
                    creation_point: (block_idx, stmt_idx),
                });
            }
        }
    }
    debug!(num_loans = loans.len(), "collected loans");
    loans
}

// ---------------------------------------------------------------------------
// Two-phase borrow helpers
// ---------------------------------------------------------------------------

/// Returns `true` if this borrow kind is a two-phase mutable borrow.
fn is_two_phase(kind: &BorrowKind) -> bool {
    matches!(
        kind,
        BorrowKind::Mut {
            allow_two_phase_borrow: true
        }
    )
}

/// Determine whether a two-phase loan is still in its reservation phase
/// using precomputed cross-block analysis.
fn loan_is_in_reservation(
    loan: &Loan,
    current_block: BasicBlockIdx,
    current_stmt_idx: usize,
    activation: &twophase::ReservationAnalysis,
) -> bool {
    if !is_two_phase(&loan.kind) {
        return false;
    }
    activation.is_reservation(current_block, current_stmt_idx)
}

// ---------------------------------------------------------------------------
// Conflict detection
// ---------------------------------------------------------------------------

/// Does creating a borrow of `new_kind` conflict with an already active
/// borrow of `active_kind` on the same place?
///
/// When `active_in_reservation` is true, the active loan is a two-phase
/// mutable borrow still in its reservation phase. In this state it acts
/// like a shared borrow, so a new shared borrow does NOT conflict.
fn conflicts_with_active(
    active_kind: &BorrowKind,
    new_kind: &BorrowKind,
    active_in_reservation: bool,
) -> bool {
    match (active_kind, new_kind) {
        (BorrowKind::Shared, BorrowKind::Shared) => false,
        // Two-phase mut in reservation acts like shared — shared borrows are OK
        (BorrowKind::Mut { .. }, BorrowKind::Shared) if active_in_reservation => false,
        (BorrowKind::Mut { .. }, _) | (_, BorrowKind::Mut { .. }) => true,
        (BorrowKind::Unique, _) | (_, BorrowKind::Unique) => true,
    }
}

/// Helper to collect places read by an rvalue without capturing generic parameters.
struct PlaceCollector<'a> {
    places: &'a mut SmallVec<[Place; 4]>,
}

impl ReadVisitor for PlaceCollector<'_> {
    fn visit_read(&mut self, place: &Place) {
        self.places.push(place.clone());
    }
}

/// Check for borrow conflicts at a single statement.
///
/// `active_loans` contains all loans whose `dest_local` is live at this
/// program point, meaning the reference is still in use and the borrow
/// is still active.
#[allow(clippy::too_many_arguments)]
fn check_terminator_conflicts(
    activation_cache: &std::collections::HashMap<usize, twophase::ReservationAnalysis>,
    ctx: &dyn BorrowckCtx,
    terminator: &glyim_mir::Terminator,
    active_loans: &[&Loan],
    current_block: BasicBlockIdx,
    current_stmt_idx: usize,
    errors: &mut Vec<GlyimDiagnostic>,
) {
    let mut read_places: SmallVec<[Place; 4]> = SmallVec::new();
    {
        let mut collector = PlaceCollector {
            places: &mut read_places,
        };
        walk_terminator_reads(&terminator.kind, &mut collector);
    }

    for place in &read_places {
        for loan in active_loans.iter().copied() {
            if places_conflict(place, &loan.borrowed_place) {
                let activation = activation_cache.get(&loan.index);
                let in_reservation = if let Some(act) = activation {
                    loan_is_in_reservation(loan, current_block, current_stmt_idx, act)
                } else {
                    false
                };
                if matches!(loan.kind, BorrowKind::Mut { .. } | BorrowKind::Unique)
                    && !in_reservation
                {
                    let name = ctx.local_name(place.local);
                    let msg = format!(
                        "cannot use `{name}` because it is {} borrowed",
                        borrow_kind_label(&loan.kind)
                    );
                    let mut diag = GlyimDiagnostic::borrow_error(terminator.source_info.span, msg);
                    diag = diag.with_sub(SubDiagnostic {
                        severity: DiagSeverity::Note,
                        message: format!("{} borrow occurs here", borrow_kind_label(&loan.kind)),
                        span: Some(MultiSpan::from_span(loan.span)),
                    });
                    errors.push(diag);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_stmt_conflicts(
    activation_cache: &std::collections::HashMap<usize, twophase::ReservationAnalysis>,
    ctx: &dyn BorrowckCtx,
    stmt: &glyim_mir::Statement,
    active_loans: &[&Loan],
    current_block: BasicBlockIdx,
    current_stmt_idx: usize,
    errors: &mut Vec<GlyimDiagnostic>,
) {
    match &stmt.kind {
        StatementKind::Assign(dest, rvalue) => {
            match rvalue {
                Rvalue::Ref(borrowed, kind) => {
                    // Creating a new borrow — check if it conflicts with
                    // any *already active* loan on the same place.
                    for loan in active_loans.iter().copied() {
                        if places_conflict(borrowed, &loan.borrowed_place) {
                            let activation = activation_cache.get(&loan.index);
                            let in_reservation = if let Some(act) = activation {
                                loan_is_in_reservation(loan, current_block, current_stmt_idx, act)
                            } else {
                                false
                            };
                            let conflict = conflicts_with_active(&loan.kind, kind, in_reservation);
                            if conflict {
                                let name = ctx.local_name(borrowed.local);
                                let msg = format!(
                                    "cannot borrow `{name}` as {} because it is also borrowed as {}",
                                    borrow_kind_label(kind),
                                    borrow_kind_label(&loan.kind)
                                );
                                let mut diag =
                                    GlyimDiagnostic::borrow_error(stmt.source_info.span, msg);
                                diag = diag.with_sub(SubDiagnostic {
                                    severity: DiagSeverity::Note,
                                    message: format!(
                                        "previous {} borrow occurs here",
                                        borrow_kind_label(&loan.kind)
                                    ),
                                    span: Some(MultiSpan::from_span(loan.span)),
                                });
                                errors.push(diag);
                            }
                        }
                    }
                }
                _ => {
                    // Not a borrow creation. Check two things:
                    // (a) reads of a place that is mutably/unique-borrowed
                    // (b) writes to a place that is borrowed at all

                    // (a) Read conflicts — collect places read by the rvalue
                    let mut read_places: SmallVec<[Place; 4]> = SmallVec::new();
                    {
                        let mut collector = PlaceCollector {
                            places: &mut read_places,
                        };
                        walk_rvalue_reads(rvalue, &mut collector);
                    }

                    for place in &read_places {
                        for loan in active_loans.iter().copied() {
                            if places_conflict(place, &loan.borrowed_place) {
                                let activation = activation_cache.get(&loan.index);
                                let in_reservation = if let Some(act) = activation {
                                    loan_is_in_reservation(
                                        loan,
                                        current_block,
                                        current_stmt_idx,
                                        act,
                                    )
                                } else {
                                    false
                                };
                                if matches!(loan.kind, BorrowKind::Mut { .. } | BorrowKind::Unique)
                                    && !in_reservation
                                {
                                    let name = ctx.local_name(place.local);
                                    let msg = format!(
                                        "cannot use `{name}` because it is {} borrowed",
                                        borrow_kind_label(&loan.kind)
                                    );
                                    let mut diag =
                                        GlyimDiagnostic::borrow_error(stmt.source_info.span, msg);
                                    diag = diag.with_sub(SubDiagnostic {
                                        severity: DiagSeverity::Note,
                                        message: format!(
                                            "{} borrow occurs here",
                                            borrow_kind_label(&loan.kind)
                                        ),
                                        span: Some(MultiSpan::from_span(loan.span)),
                                    });
                                    errors.push(diag);
                                }
                            }
                        }
                    }

                    // (b) Write conflicts — assigning to a borrowed place.
                    // In MIR, direct writes to a shared-borrowed place are ALWAYS illegal.
                    // Interior mutability (e.g. Cell) does NOT manifest as a direct `Assign`
                    // to the borrowed local; it uses method calls and `&mut` derived from
                    // `&UnsafeCell`. Therefore, any active loan (Shared, Mut, or Unique)
                    // conflicts with a write to an overlapping place.
                    for loan in active_loans.iter() {
                        if places_conflict(dest, &loan.borrowed_place) {
                            let name = ctx.local_name(dest.local);
                            let msg = format!("cannot assign to `{name}` because it is borrowed",);
                            let mut diag =
                                GlyimDiagnostic::borrow_error(stmt.source_info.span, msg);
                            diag = diag.with_sub(SubDiagnostic {
                                severity: DiagSeverity::Note,
                                message: format!(
                                    "{} borrow occurs here",
                                    borrow_kind_label(&loan.kind)
                                ),
                                span: Some(MultiSpan::from_span(loan.span)),
                            });
                            errors.push(diag);
                        }
                    }
                }
            }
        }
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {}
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Check borrow conflicts using Polonius-style region inference.
///
/// The analysis:
/// 1. Collects all loans (borrows) from the MIR body.
/// 2. Computes local liveness across the CFG.
/// 3. Walks each basic block, determining active loans at each statement,
///    and flags conflicts between active loans and place accesses.
///
/// Two-phase mutable borrows (`BorrowKind::Mut { allow_two_phase_borrow: true }`)
/// start in a reservation phase where shared access to the borrowed place
/// is still allowed. They transition to activated (full mutable semantics)
/// when the reference (`dest_local`) is first read.
pub fn check_borrows(ctx: &dyn BorrowckCtx, body: &Body) -> BorrowckResult {
    trace!("starting borrow checking");
    let loans = collect_loans(body);
    let liveness_result = liveness::compute_liveness(body);
    // Precompute two‑phase activation analysis for all loans
    let mut activation_cache: std::collections::HashMap<usize, twophase::ReservationAnalysis> =
        std::collections::HashMap::new();
    for (loan_idx, loan) in loans.iter().enumerate() {
        if is_two_phase(&loan.kind) {
            let activation = twophase::ReservationAnalysis::compute(
                body,
                loan.creation_point.0,
                loan.creation_point.1,
                loan.dest_local,
            );
            activation_cache.insert(loan_idx, activation);
        }
    }
    let mut errors = Vec::new();

    // Precompute index: local -> loans that have this local as dest.
    // This avoids re-iterating all loans for every statement.
    let mut loans_by_dest: Vec<SmallVec<[usize; 2]>> = vec![SmallVec::new(); body.locals.len()];
    for (i, loan) in loans.iter().enumerate() {
        loans_by_dest[loan.dest_local.to_raw() as usize].push(i);
    }

    for (block_idx, block_data) in body.basic_blocks.iter_enumerated() {
        let block_usize = block_idx.to_raw() as usize;
        let live_out = &liveness_result.live_out[block_usize];
        let stmt_liveness = liveness::compute_stmt_liveness(body, block_idx, live_out);

        for (stmt_idx, stmt) in block_data.statements.iter().enumerate() {
            let live_locals = &stmt_liveness[stmt_idx];

            // A loan is active at this point if the local holding the
            // reference is live (will be used later).
            let mut active_loans: SmallVec<[&Loan; 4]> = SmallVec::new();
            for local_idx in live_locals.ones() {
                for &loan_idx in &loans_by_dest[local_idx] {
                    active_loans.push(&loans[loan_idx]);
                }
            }

            check_stmt_conflicts(
                &activation_cache,
                ctx,
                stmt,
                &active_loans,
                block_idx,
                stmt_idx,
                &mut errors,
            );
        }

        // Check terminator reads against active loans at the end of the block
        let num_stmts = block_data.statements.len();
        let live_locals = &stmt_liveness[num_stmts];
        let mut active_loans: SmallVec<[&Loan; 4]> = SmallVec::new();
        for local_idx in live_locals.ones() {
            for &loan_idx in &loans_by_dest[local_idx] {
                active_loans.push(&loans[loan_idx]);
            }
        }
        check_terminator_conflicts(
            &activation_cache,
            ctx,
            &block_data.terminator,
            &active_loans,
            block_idx,
            num_stmts,
            &mut errors,
        );
    }

    let move_errors = move_analysis::check_moves(ctx, body);
    errors.extend(move_errors);

    debug!(num_errors = errors.len(), "borrow checking complete");
    BorrowckResult { errors }
}

#[cfg(test)]
mod terminator_conflict_tests {
    use super::*;
    use glyim_core::arena::IndexVec;
    use glyim_core::def_id::{CrateId, DefId, LocalDefId};
    use glyim_core::primitives::Mutability;
    use glyim_mir::{
        BasicBlockData, BasicBlockIdx, Body, BorrowKind, LocalDecl, Operand, Place, Rvalue,
        SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
    };
    use glyim_span::Span;
    use glyim_type::{Ty, TyCtx, TyCtxMut};

    struct MockCtx {
        locals: IndexVec<LocalIdx, LocalDecl>,
        ty_ctx: TyCtx,
    }

    impl BorrowckCtx for MockCtx {
        fn ty_ctx(&self) -> &TyCtx {
            &self.ty_ctx
        }
        fn local_decl(&self, local: LocalIdx) -> &LocalDecl {
            &self.locals[local]
        }
        fn local_name(&self, local: LocalIdx) -> String {
            format!("local_{}", local.to_raw())
        }
    }

    #[test]
    fn test_terminator_read_conflict() {
        let ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
        let ty_ctx = ctx_mut.freeze();

        let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
        let local_0 = locals.push(LocalDecl {
            // return place
            ty: Ty::ERROR,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let local_1 = locals.push(LocalDecl {
            // borrowed place
            ty: Ty::BOOL,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let local_2 = locals.push(LocalDecl {
            // reference
            ty: Ty::ERROR,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });

        let borrow_stmt = Statement {
            kind: StatementKind::Assign(
                Place::new(local_2),
                Rvalue::Ref(
                    Place::new(local_1),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };

        let block0 = BasicBlockData {
            statements: vec![borrow_stmt],
            terminator: Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(local_1)),
                    switch_ty: Ty::BOOL,
                    targets: glyim_mir::SwitchTargets::new(
                        Box::new([]),
                        BasicBlockIdx::from_raw(1),
                    ),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };

        let assign_stmt = Statement {
            kind: StatementKind::Assign(
                Place::new(local_0),
                Rvalue::Use(Operand::Move(Place::new(local_2))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };

        let block1 = BasicBlockData {
            statements: vec![assign_stmt],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };

        let mut body = Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: IndexVec::new(),
            locals,
            arg_count: 0,
            return_ty: Ty::ERROR,
            span: Span::DUMMY,
            var_debug_info: Vec::new(),
        };
        body.basic_blocks.push(block0);
        body.basic_blocks.push(block1);

        let mock_ctx = MockCtx {
            locals: body.locals.clone(),
            ty_ctx,
        };
        let result = check_borrows(&mock_ctx, &body);

        assert!(
            result.errors.iter().any(|d| d
                .message
                .contains("cannot use `local_1` because it is mutable borrowed")),
            "Expected a borrow error for using local_1 in terminator, got: {:?}",
            result.errors
        );
    }
}

#[cfg(test)]
mod tests;
