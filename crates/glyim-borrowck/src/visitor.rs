//! Shared MIR traversal infrastructure.
//!
//! Provides a single [`ReadVisitor`] trait and `walk_*` functions that replace
//! the four duplicate traversal implementations previously scattered across the
//! borrow checker. Every consumer implements `visit_read` and reuses the same
//! walk logic; adding a new `Rvalue` variant only requires updating this module.

use fixedbitset::FixedBitSet as BitSet;
use glyim_mir::{BorrowKind, LocalIdx, Operand, Place, ProjectionElem, Rvalue, TerminatorKind};

// ---------------------------------------------------------------------------
// Visitor trait
// ---------------------------------------------------------------------------

/// Callback invoked for each place that is *read* by an MIR construct.
///
/// Implementations decide what to do with each read — collect into a set,
/// check for a match, generate liveness facts, etc.
pub(crate) trait ReadVisitor {
    fn visit_read(&mut self, place: &Place);
}

// ---------------------------------------------------------------------------
// Walk functions — single source of truth for MIR traversal
// ---------------------------------------------------------------------------

/// Walk every place read by an rvalue.
pub(crate) fn walk_rvalue_reads(rvalue: &Rvalue, visitor: &mut dyn ReadVisitor) {
    match rvalue {
        Rvalue::Use(operand) => walk_operand_reads(operand, visitor),
        Rvalue::Ref(place, _) => visitor.visit_read(place),
        Rvalue::BinaryOp(_, pair) => {
            let (left, right) = pair.as_ref();
            walk_operand_reads(left, visitor);
            walk_operand_reads(right, visitor);
        }
        Rvalue::UnaryOp(_, operand) => walk_operand_reads(operand, visitor),
        Rvalue::Aggregate(_, operands) => {
            for op in operands {
                walk_operand_reads(op, visitor);
            }
        }
        Rvalue::Discriminant(place) | Rvalue::Len(place) => visitor.visit_read(place),
        Rvalue::Cast(_, operand, _) => walk_operand_reads(operand, visitor),
        Rvalue::Repeat(operand, _) => walk_operand_reads(operand, visitor),
    }
}

/// Walk every place read by an operand.
pub(crate) fn walk_operand_reads(operand: &Operand, visitor: &mut dyn ReadVisitor) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => visitor.visit_read(place),
        Operand::Constant(_) => {}
    }
}

/// Walk every place read by a terminator (excluding defs like `Call::destination`).
pub(crate) fn walk_terminator_reads(kind: &TerminatorKind, visitor: &mut dyn ReadVisitor) {
    match kind {
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
        TerminatorKind::SwitchInt { discr, .. } => walk_operand_reads(discr, visitor),
        TerminatorKind::Call { func, args, .. } => {
            walk_operand_reads(func, visitor);
            for arg in args {
                walk_operand_reads(arg, visitor);
            }
        }
        TerminatorKind::Assert { cond, .. } => walk_operand_reads(cond, visitor),
        TerminatorKind::Drop { .. } => {
            // Drop is a move-out (kill), not a shared read.
        }
    }
}

// ---------------------------------------------------------------------------
// Successor / predecessor helpers
// ---------------------------------------------------------------------------

/// Return the successor basic blocks of a terminator.
pub(crate) fn successor_blocks(kind: &TerminatorKind) -> Vec<glyim_mir::BasicBlockIdx> {
    use glyim_mir::BasicBlockIdx;
    match kind {
        TerminatorKind::Goto { target } => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => {
            let mut succs: Vec<BasicBlockIdx> = targets.iter().map(|(_, bb)| bb).collect();
            succs.push(targets.otherwise());
            succs
        }
        TerminatorKind::Return | TerminatorKind::Unreachable => vec![],
        TerminatorKind::Call {
            target, cleanup, ..
        } => {
            let mut succs = Vec::new();
            if let Some(t) = target {
                succs.push(*t);
            }
            if let Some(c) = cleanup {
                succs.push(*c);
            }
            succs
        }
        TerminatorKind::Assert {
            target, cleanup, ..
        } => {
            let mut succs = vec![*target];
            if let Some(c) = cleanup {
                succs.push(*c);
            }
            succs
        }
        TerminatorKind::Drop {
            target, cleanup, ..
        } => {
            let mut succs = vec![*target];
            if let Some(c) = cleanup {
                succs.push(*c);
            }
            succs
        }
    }
}

// ---------------------------------------------------------------------------
// Place conflict detection
// ---------------------------------------------------------------------------

/// Do two places conflict (overlap in memory)?
///
/// Two places conflict when one is a prefix of the other. Disjoint field
/// accesses on the same root local (e.g. `a.0` vs `a.1`) do **not** conflict.
/// Different variants of the same enum also do not conflict.
///
/// `Deref` projections are conservatively assumed to alias (without pointer
/// analysis we cannot prove otherwise). `Index` projections are likewise
/// conservatively assumed to alias because the index value is unknown at
/// compile time.
pub(crate) fn places_conflict(a: &Place, b: &Place) -> bool {
    if a.local != b.local {
        return false;
    }

    let a_proj: &[ProjectionElem] = &a.projection;
    let b_proj: &[ProjectionElem] = &b.projection;
    let min_len = a_proj.len().min(b_proj.len());

    for i in 0..min_len {
        match (&a_proj[i], &b_proj[i]) {
            // Same deref — conservatively assume same target
            (ProjectionElem::Deref, ProjectionElem::Deref) => continue,

            (ProjectionElem::Field(f1), ProjectionElem::Field(f2)) => {
                if f1 == f2 {
                    continue; // same field — not yet disjoint
                } else {
                    return false; // different fields — disjoint
                }
            }

            (ProjectionElem::Downcast(v1), ProjectionElem::Downcast(v2)) => {
                if v1 == v2 {
                    continue; // same variant
                } else {
                    return false; // different variants — disjoint
                }
            }

            // Index projections (MIR-21): two `Index` projections on the same
            // base MAY alias at runtime — different index locals can hold the
            // same value (`let i = 0; let j = i; arr[i] vs arr[j]`). The old
            // code treated different locals as disjoint, which is unsound.
            // Be conservative: keep checking the rest of the projection list.
            (ProjectionElem::Index(_), ProjectionElem::Index(_)) => {
                continue;
            }

            // ConstantIndex projections: statically known offsets.
            (
                ProjectionElem::ConstantIndex {
                    offset: o1,
                    from_end: false,
                    ..
                },
                ProjectionElem::ConstantIndex {
                    offset: o2,
                    from_end: false,
                    ..
                },
            ) => {
                if o1 != o2 {
                    return false; // provably disjoint, e.g. arr[0] vs arr[1]
                }
                // same offset -> keep checking the rest of the projection chain
            }

            (
                ProjectionElem::ConstantIndex {
                    offset: _,
                    from_end: true,
                    ..
                },
                ProjectionElem::ConstantIndex {
                    offset: _,
                    from_end: true,
                    ..
                },
            ) => {
                // Both from_end: true. We don't know the length statically here,
                // so we cannot prove they are disjoint. Conservatively conflict.
                return true;
            }

            (ProjectionElem::ConstantIndex { .. }, ProjectionElem::ConstantIndex { .. }) => {
                // Mixed from_end: one true, one false. Conservatively conflict.
                return true;
            }

            (ProjectionElem::Field(_), ProjectionElem::Index(_))
            | (ProjectionElem::Index(_), ProjectionElem::Field(_)) => return false,

            // Subslice vs Subslice (both from_end: false)
            (
                ProjectionElem::Subslice {
                    from: f1,
                    to: t1,
                    from_end: false,
                },
                ProjectionElem::Subslice {
                    from: f2,
                    to: t2,
                    from_end: false,
                },
            ) => {
                // Check if [f1, t1) and [f2, t2) overlap
                if f1 < t2 && f2 < t1 {
                    continue; // Overlap, keep checking rest
                } else {
                    return false; // Disjoint
                }
            }

            // Subslice vs ConstantIndex (both from_end: false)
            (
                ProjectionElem::Subslice {
                    from,
                    to,
                    from_end: false,
                },
                ProjectionElem::ConstantIndex {
                    offset,
                    from_end: false,
                    ..
                },
            )
            | (
                ProjectionElem::ConstantIndex {
                    offset,
                    from_end: false,
                    ..
                },
                ProjectionElem::Subslice {
                    from,
                    to,
                    from_end: false,
                },
            ) => {
                if *offset >= *from && *offset < *to {
                    continue; // Index is inside subslice
                } else {
                    return false; // Disjoint
                }
            }

            // Subslice with from_end: true or mixed, or other mixed projection types
            // at the same depth — conservatively conflict without full type info.
            _ => return true,
        }
    }

    // One projection is a prefix of the other — they overlap
    true
}

// ---------------------------------------------------------------------------
// Concrete visitors
// ---------------------------------------------------------------------------

/// Checks whether a specific local is read (used for two-phase activation).
pub(crate) struct LocalReadChecker {
    target: LocalIdx,
    found: bool,
}

impl LocalReadChecker {
    pub(crate) fn new(target: LocalIdx) -> Self {
        Self {
            target,
            found: false,
        }
    }

    pub(crate) fn found(&self) -> bool {
        self.found
    }
}

impl ReadVisitor for LocalReadChecker {
    fn visit_read(&mut self, place: &Place) {
        if place.local == self.target {
            self.found = true;
        }
    }
}

/// Collects block-level "use" facts: records `place.local` into the uses set
/// only if it has not already been defined earlier in the same block.
pub(crate) struct LivenessUseCollector<'a> {
    pub uses: &'a mut BitSet,
    pub defs: &'a BitSet,
}

impl ReadVisitor for LivenessUseCollector<'_> {
    fn visit_read(&mut self, place: &Place) {
        let idx = place.local.to_raw() as usize;
        if !self.defs.contains(idx) {
            self.uses.insert(idx);
        }
    }
}

/// Unconditionally adds `place.local` to a liveness "gen" set.
pub(crate) struct LivenessGen<'a> {
    pub live: &'a mut BitSet,
}

impl ReadVisitor for LivenessGen<'_> {
    fn visit_read(&mut self, place: &Place) {
        self.live.insert(place.local.to_raw() as usize);
    }
}

/// Human-readable label for a borrow kind.
pub(crate) fn borrow_kind_label(kind: &BorrowKind) -> &'static str {
    match kind {
        BorrowKind::Shared => "shared",
        BorrowKind::Mut { .. } => "mutable",
        BorrowKind::Unique => "unique",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_mir::{LocalIdx, Place, ProjectionElem};
    use glyim_type::FieldIdx;

    fn make_place(local: u32, projections: &[ProjectionElem]) -> Place {
        Place {
            local: LocalIdx::from_raw(local),
            projection: projections.iter().cloned().collect(),
        }
    }

    #[test]
    fn test_constant_index_disjoint() {
        let p1 = make_place(
            0,
            &[ProjectionElem::ConstantIndex {
                offset: 0,
                min_length: 0,
                from_end: false,
            }],
        );
        let p2 = make_place(
            0,
            &[ProjectionElem::ConstantIndex {
                offset: 1,
                min_length: 0,
                from_end: false,
            }],
        );
        assert!(!places_conflict(&p1, &p2));
    }

    #[test]
    fn test_constant_index_overlap() {
        let p1 = make_place(
            0,
            &[ProjectionElem::ConstantIndex {
                offset: 0,
                min_length: 0,
                from_end: false,
            }],
        );
        let p2 = make_place(
            0,
            &[ProjectionElem::ConstantIndex {
                offset: 0,
                min_length: 0,
                from_end: false,
            }],
        );
        assert!(places_conflict(&p1, &p2));
    }

    #[test]
    fn test_subslice_disjoint() {
        let p1 = make_place(
            0,
            &[ProjectionElem::Subslice {
                from: 0,
                to: 2,
                from_end: false,
            }],
        );
        let p2 = make_place(
            0,
            &[ProjectionElem::Subslice {
                from: 3,
                to: 5,
                from_end: false,
            }],
        );
        assert!(!places_conflict(&p1, &p2));
    }

    #[test]
    fn test_subslice_overlap() {
        let p1 = make_place(
            0,
            &[ProjectionElem::Subslice {
                from: 0,
                to: 3,
                from_end: false,
            }],
        );
        let p2 = make_place(
            0,
            &[ProjectionElem::Subslice {
                from: 2,
                to: 5,
                from_end: false,
            }],
        );
        assert!(places_conflict(&p1, &p2));
    }

    #[test]
    fn test_subslice_vs_constant_index_disjoint() {
        let p1 = make_place(
            0,
            &[ProjectionElem::Subslice {
                from: 0,
                to: 2,
                from_end: false,
            }],
        );
        let p2 = make_place(
            0,
            &[ProjectionElem::ConstantIndex {
                offset: 3,
                min_length: 0,
                from_end: false,
            }],
        );
        assert!(!places_conflict(&p1, &p2));
    }

    #[test]
    fn test_subslice_vs_constant_index_overlap() {
        let p1 = make_place(
            0,
            &[ProjectionElem::Subslice {
                from: 0,
                to: 3,
                from_end: false,
            }],
        );
        let p2 = make_place(
            0,
            &[ProjectionElem::ConstantIndex {
                offset: 2,
                min_length: 0,
                from_end: false,
            }],
        );
        assert!(places_conflict(&p1, &p2));
    }

    #[test]
    fn test_field_vs_index_disjoint() {
        let p1 = make_place(0, &[ProjectionElem::Field(FieldIdx::from_raw(0))]);
        let p2 = make_place(0, &[ProjectionElem::Index(LocalIdx::from_raw(1))]);
        assert!(!places_conflict(&p1, &p2));
    }
}
