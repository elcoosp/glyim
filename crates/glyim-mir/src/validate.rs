//! MIR well-formedness validator (M5.5): the local-declaredness invariant.
//!
//! Every `LocalIdx` referenced by a statement or terminator's places/operands
//! must be declared in `body.locals`. This is the def-before-use slice of the
//! MIR validator v2 (plan §6 M5.5), extracted as a standalone, never-panicking
//! predicate so the M4 property oracle and the future full validator share a
//! single implementation. It returns `Result<(), LocalDeclarednessError>` —
//! never panics, advancing the M5.10 panic-ratchet direction.

use crate::{Body, LocalIdx, Operand, Rvalue, StatementKind, TerminatorKind};
use glyim_span::Span;

/// Where a declaredness violation was found within a statement/terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A local-use/declaration mismatch found by [`check_local_declaredness`]: a
/// statement or terminator referenced a `LocalIdx` that is not declared in
/// `body.locals`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDeclarednessError {
    /// The offending local index (`>= body.locals.len()`).
    pub local: LocalIdx,
    /// Where the offending reference lives.
    pub site: ErrorSite,
    /// Span of the offending statement/terminator.
    pub span: Span,
}

/// Check that every `LocalIdx` referenced by `body`'s statements and
/// terminators is declared in `body.locals`. Returns `Ok(())` if the body is
/// well-formed, or the first offending local otherwise. Never panics.
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
            let (a, b) = &**ops;
            out.extend(locals_in_operand(a));
            out.extend(locals_in_operand(b));
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                out.extend(locals_in_operand(op));
            }
        }
    }
    out
}

/// Enumerate every `LocalIdx` an operand references (via its place).
fn locals_in_operand(op: &Operand) -> Vec<(LocalIdx, ErrorSite)> {
    let mut out = Vec::new();
    match op {
        Operand::Copy(place) | Operand::Move(place) => {
            out.push((place.local, ErrorSite::Operand));
        }
        Operand::Constant(_) => {}
    }
    out
}

/// Enumerate every `LocalIdx` a terminator references via its operands/places.
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
        TerminatorKind::Drop { place, .. } => out.push((place.local, ErrorSite::Terminator)),
    }
    out
}
