//! Random well-formed MIR `Body` generator (M4 property-oracle support).
//!
//! Pairs with [`glyim_mir::validate::check_local_declaredness`]: every body
//! this generator produces references only declared locals, so the
//! declaredness oracle must accept it. The property test asserts that
//! invariant over many seeds/iterations; a negative partner injects an
//! undeclared local to prove the oracle catches the violation rather than
//! rubber-stamping generated bodies.

use glyim_core::def_id::DefId;
use glyim_core::primitives::Mutability;
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, LocalDecl, LocalIdx, Operand, Place, Rvalue, SourceInfo,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_type::Ty;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

/// Generates random MIR bodies that are well-formed by construction: every
/// local a statement references is one of the body's declared locals.
pub struct MirBodyGenerator {
    rng: StdRng,
}

impl MirBodyGenerator {
    /// new.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Generate a body with a random count of declared locals (1..=8) and a
    /// single basic block of `StorageLive`/`StorageDead`/`Assign(Use(Copy))`/
    /// `Nop` statements that reference only declared locals, terminated by
    /// `Return`. The body is well-formed by construction.
    pub fn generate_well_formed_body(&mut self, owner: DefId) -> Body {
        let mut body = Body::dummy(owner);
        let extra = self.rng.random_range(0..8);
        for _ in 0..extra {
            body.locals.push(LocalDecl {
                ty: Ty::UNIT,
                mutability: Mutability::Not,
                source_info: SourceInfo::new(Span::DUMMY),
            });
        }
        let declared: Vec<LocalIdx> = body.locals.iter_enumerated().map(|(idx, _)| idx).collect();
        let mut block = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let n_stmts = self.rng.random_range(0..8);
        for _ in 0..n_stmts {
            let local = declared[self.rng.random_range(0..declared.len())];
            block.statements.push(Statement {
                kind: self.random_statement_kind(local),
                source_info: SourceInfo::new(Span::DUMMY),
            });
        }
        body.basic_blocks.push(block);
        body
    }

    /// Generate a body whose first statement references a local index beyond
    /// the declared set, so the declaredness oracle must reject it.
    pub fn generate_undeclared_local_body(&mut self, owner: DefId) -> Body {
        let mut body = Body::dummy(owner);
        let mut block = BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        // `body.locals.len()` is 1 (the dummy local); index 5 is undeclared.
        let undeclared = LocalIdx::from_raw(5);
        block.statements.push(Statement {
            kind: StatementKind::StorageLive(undeclared),
            source_info: SourceInfo::new(Span::DUMMY),
        });
        body.basic_blocks.push(block);
        body
    }

    /// Generate a malformed body by starting from a well-formed one and
    /// injecting exactly one historical bug class (audit MIR-* rows), picked
    /// at random. Every body this returns violates exactly one validator
    /// invariant family, so the unified [`glyim_mir::validate::validate_body`]
    /// must reject it with a typed [`glyim_mir::validate::Violation`] — and
    /// the construction itself never panics (no out-of-bounds indexing), so
    /// the fuzz harness can throw 10 000 of these at the validator and assert
    /// zero panics.
    ///
    /// The injected classes are all ctx-free (no type lookups), so the
    /// generator stays independent of `TyCtx`:
    ///  * undeclared local (MIR-28),
    ///  * missing block target (MIR-7 / MIR-10),
    ///  * double `StorageLive` (storage pairing),
    ///  * `arg_count` overflow (MIR-30),
    ///  * `Subslice` inside an `Aggregate` operand (MIR-31),
    ///  * cleanup edge on a cleanup block (cleanup-edge validity).
    pub fn generate_malformed_body(&mut self, owner: DefId) -> Body {
        let mut body = self.generate_well_formed_body(owner);
        let bb0 = BasicBlockIdx::from_raw(0);
        // The well-formed body has at least block 0 (dummy) + one pushed block;
        // operate on block 0, which always exists.
        match self.rng.random_range(0..6) {
            0 => {
                // Undeclared local: StorageLive an index beyond locals.len().
                let undeclared = LocalIdx::from_raw((body.locals.len() + 5) as u32);
                body.basic_blocks[bb0].statements.push(Statement {
                    kind: StatementKind::StorageLive(undeclared),
                    source_info: SourceInfo::new(Span::DUMMY),
                });
            }
            1 => {
                // Missing block target: retarget block 0's terminator to 99.
                body.basic_blocks[bb0].terminator = Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(99),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                };
            }
            2 => {
                // Double StorageLive of a non-arg local (index 1 if present,
                // else the return place 0 — both are valid locals, so this is
                // purely the double-live violation).
                let local = if body.locals.len() > 1 {
                    LocalIdx::from_raw(1)
                } else {
                    LocalIdx::from_raw(0)
                };
                body.basic_blocks[bb0].statements.push(Statement {
                    kind: StatementKind::StorageLive(local),
                    source_info: SourceInfo::new(Span::DUMMY),
                });
                body.basic_blocks[bb0].statements.push(Statement {
                    kind: StatementKind::StorageLive(local),
                    source_info: SourceInfo::new(Span::DUMMY),
                });
            }
            3 => {
                // arg_count overflow: claim more args than locals can hold.
                body.arg_count = body.locals.len() + 5;
            }
            4 => {
                // Subslice inside an Aggregate operand (MIR-31): the validator
                // must scan every rvalue operand, not just the first place.
                let subslice_place = Place {
                    local: LocalIdx::from_raw(0),
                    projection: Box::new([glyim_mir::ProjectionElem::Subslice {
                        from: 1,
                        to: 2,
                        from_end: false,
                    }]),
                };
                body.basic_blocks[bb0].statements.push(Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(0)),
                        Rvalue::Aggregate(
                            glyim_mir::AggregateKind::Tuple,
                            vec![Operand::Copy(subslice_place)],
                        ),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                });
            }
            _ => {
                // Cleanup edge on a cleanup block: mark block 0 cleanup and
                // give it a Drop terminator with a cleanup edge.
                body.basic_blocks[bb0].is_cleanup = true;
                body.basic_blocks[bb0].terminator = Terminator {
                    kind: TerminatorKind::Drop {
                        place: Place::new(LocalIdx::from_raw(0)),
                        target: bb0,
                        cleanup: Some(bb0),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                };
            }
        }
        body
    }

    /// Pick a statement kind that references `local` (or `Nop`).
    fn random_statement_kind(&mut self, local: LocalIdx) -> StatementKind {
        match self.rng.random_range(0..4) {
            0 => StatementKind::StorageLive(local),
            1 => StatementKind::StorageDead(local),
            2 => StatementKind::Assign(
                Place::new(local),
                Rvalue::Use(Operand::Copy(Place::new(local))),
            ),
            _ => StatementKind::Nop,
        }
    }
}
