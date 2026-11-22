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
    BasicBlockData, Body, LocalDecl, LocalIdx, Operand, Place, Rvalue, SourceInfo, Statement,
    StatementKind, Terminator, TerminatorKind,
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
