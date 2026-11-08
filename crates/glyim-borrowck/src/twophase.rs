//! Two‑phase borrow activation analysis – full CFG dataflow.

use crate::visitor::{LocalReadChecker, successor_blocks, walk_rvalue_reads};
use fixedbitset::FixedBitSet as BitSet;
use glyim_mir::{BasicBlockIdx, Body, LocalIdx, StatementKind};
use std::collections::{HashSet, VecDeque};

pub struct ReservationAnalysis {
    per_block: Vec<BitSet>,
}

impl ReservationAnalysis {
    pub fn compute(
        body: &Body,
        loan_block: BasicBlockIdx,
        loan_stmt: usize,
        dest_local: LocalIdx,
    ) -> Self {
        let num_blocks = body.basic_blocks.len();
        let stmt_counts: Vec<usize> = body
            .basic_blocks
            .iter()
            .map(|b| b.statements.len())
            .collect();

        let mut per_block: Vec<BitSet> = stmt_counts
            .iter()
            .map(|&len| BitSet::with_capacity(len + 1))
            .collect();

        let mut entry_reserved: Vec<bool> = vec![false; num_blocks];
        let mut exit_reserved: Vec<bool> = vec![false; num_blocks];

        let mut worklist: VecDeque<BasicBlockIdx> = VecDeque::new();
        let mut in_worklist: HashSet<BasicBlockIdx> = HashSet::new();

        let mut enqueue = |block: BasicBlockIdx| {
            if !in_worklist.contains(&block) {
                in_worklist.insert(block);
                worklist.push_back(block);
            }
        };

        let transfer =
            |block: BasicBlockIdx, start_idx: usize, start_current: bool| -> (BitSet, bool) {
                let num_stmts = stmt_counts[block.to_raw() as usize];
                let mut bits = BitSet::with_capacity(num_stmts + 1);
                let mut current = start_current;
                if start_idx <= num_stmts {
                    for i in start_idx..num_stmts {
                        if current {
                            bits.insert(i);
                        }
                        let stmt = &body.basic_blocks[block].statements[i];
                        let mut checker = LocalReadChecker::new(dest_local);
                        if let StatementKind::Assign(_, rvalue) = &stmt.kind {
                            walk_rvalue_reads(rvalue, &mut checker);
                        }
                        if checker.found() {
                            current = false;
                        }
                    }
                    if current {
                        bits.insert(num_stmts);
                    }
                }
                let exit_state = current;
                (bits, exit_state)
            };

        let block_data = &body.basic_blocks[loan_block];
        let start_point = loan_stmt + 1;

        let (bits, exit) = transfer(loan_block, start_point, true);
        per_block[loan_block.to_raw() as usize] = bits;
        exit_reserved[loan_block.to_raw() as usize] = exit;

        if exit {
            for succ in successor_blocks(&block_data.terminator.kind) {
                if !entry_reserved[succ.to_raw() as usize] {
                    entry_reserved[succ.to_raw() as usize] = true;
                    enqueue(succ);
                }
            }
        }

        let mut preds: Vec<Vec<BasicBlockIdx>> = vec![Vec::new(); num_blocks];
        for (i, block_data) in body.basic_blocks.iter_enumerated() {
            for succ in successor_blocks(&block_data.terminator.kind) {
                preds[succ.to_raw() as usize].push(i);
            }
        }

        while let Some(block) = worklist.pop_front() {
            in_worklist.remove(&block);
            let entry = entry_reserved[block.to_raw() as usize];
            let (bits, exit) = transfer(block, 0, entry);
            let old_bits = &per_block[block.to_raw() as usize];
            let old_exit = exit_reserved[block.to_raw() as usize];
            if &bits != old_bits || exit != old_exit {
                per_block[block.to_raw() as usize] = bits;
                exit_reserved[block.to_raw() as usize] = exit;
                for succ in successor_blocks(&body.basic_blocks[block].terminator.kind) {
                    let new_entry = preds[succ.to_raw() as usize]
                        .iter()
                        .any(|&pred| exit_reserved[pred.to_raw() as usize]);
                    if new_entry != entry_reserved[succ.to_raw() as usize] {
                        entry_reserved[succ.to_raw() as usize] = new_entry;
                        if !in_worklist.contains(&succ) {
                            in_worklist.insert(succ);
                            worklist.push_back(succ);
                        }
                    }
                }
            }
        }

        ReservationAnalysis { per_block }
    }

    pub fn is_reservation(&self, block: BasicBlockIdx, stmt_idx: usize) -> bool {
        self.per_block
            .get(block.to_raw() as usize)
            .is_some_and(|bits| bits.contains(stmt_idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::def_id::{CrateId, DefId, LocalDefId};
    use glyim_core::primitives::Mutability;
    use glyim_mir::{
        BasicBlockData, Body, BorrowKind, LocalDecl, Place, Rvalue, SourceInfo, Statement,
        StatementKind, Terminator, TerminatorKind,
    };
    use glyim_span::Span;
    use glyim_type::Ty;

    #[test]
    fn test_same_block_no_activation() {
        let mut body = Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: glyim_core::arena::IndexVec::new(),
            locals: glyim_core::arena::IndexVec::new(),
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: Span::DUMMY,
            var_debug_info: Vec::new(),
        };

        let local_1 = body.locals.push(LocalDecl {
            ty: Ty::BOOL,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let local_2 = body.locals.push(LocalDecl {
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
                        allow_two_phase_borrow: true,
                    },
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let block = BasicBlockData {
            statements: vec![borrow_stmt],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };
        body.basic_blocks.push(block);

        let analysis = ReservationAnalysis::compute(&body, BasicBlockIdx::from_raw(0), 0, local_2);
        assert!(analysis.is_reservation(BasicBlockIdx::from_raw(0), 1));
        assert!(!analysis.is_reservation(BasicBlockIdx::from_raw(0), 0));
    }

    #[test]
    fn test_cross_block_extends_if_not_activated() {
        let mut body = Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: glyim_core::arena::IndexVec::new(),
            locals: glyim_core::arena::IndexVec::new(),
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: Span::DUMMY,
            var_debug_info: Vec::new(),
        };
        let local_1 = body.locals.push(LocalDecl {
            ty: Ty::BOOL,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let local_2 = body.locals.push(LocalDecl {
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
                        allow_two_phase_borrow: true,
                    },
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let block0 = BasicBlockData {
            statements: vec![borrow_stmt],
            terminator: Terminator {
                kind: TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(1),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };
        let block1 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };
        body.basic_blocks.push(block0);
        body.basic_blocks.push(block1);
        let analysis = ReservationAnalysis::compute(&body, BasicBlockIdx::from_raw(0), 0, local_2);
        assert!(analysis.is_reservation(BasicBlockIdx::from_raw(1), 0));
    }

    #[test]
    fn test_multiple_loans_index_alignment() {
        let mut body = Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
            basic_blocks: glyim_core::arena::IndexVec::new(),
            locals: glyim_core::arena::IndexVec::new(),
            arg_count: 0,
            return_ty: Ty::UNIT,
            span: Span::DUMMY,
            var_debug_info: Vec::new(),
        };

        let local_1 = body.locals.push(LocalDecl {
            ty: Ty::BOOL,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let local_2 = body.locals.push(LocalDecl {
            ty: Ty::ERROR,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let local_3 = body.locals.push(LocalDecl {
            ty: Ty::ERROR,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        let local_4 = body.locals.push(LocalDecl {
            ty: Ty::ERROR,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });

        let borrow_stmt_1 = Statement {
            kind: StatementKind::Assign(
                Place::new(local_2),
                Rvalue::Ref(
                    Place::new(local_1),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: true,
                    },
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let borrow_stmt_2 = Statement {
            kind: StatementKind::Assign(
                Place::new(local_3),
                Rvalue::Ref(
                    Place::new(local_1),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: true,
                    },
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };
        let borrow_stmt_3 = Statement {
            kind: StatementKind::Assign(
                Place::new(local_4),
                Rvalue::Ref(
                    Place::new(local_1),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: true,
                    },
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        };

        let block = BasicBlockData {
            statements: vec![borrow_stmt_1, borrow_stmt_2, borrow_stmt_3],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        };
        body.basic_blocks.push(block);

        let loans = crate::collect_loans(&body);
        assert_eq!(loans.len(), 3);
        assert_eq!(loans[0].index, 0);
        assert_eq!(loans[1].index, 1);
        assert_eq!(loans[2].index, 2);

        // Test that the activation cache is keyed by global index
        let analysis = ReservationAnalysis::compute(
            &body,
            BasicBlockIdx::from_raw(0),
            1, // loan 1
            local_3,
        );
        // Loan 1 is in reservation at stmt 2
        assert!(analysis.is_reservation(BasicBlockIdx::from_raw(0), 2));
    }
}
