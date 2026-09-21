#![allow(clippy::needless_range_loop)]
#![allow(clippy::unused_enumerate_index)]
//! Drop elaboration: inserts drop flags and conditional branches around `Drop` terminators.
//!
//! Plan §15.2: array drops are replaced with a loop that drops each element.
//! When an array is *incrementally* initialized (a loop that assigns elements
//! one at a time and may early-exit/panic partway), per-element drop flags
//! gate the loop so only initialized elements are dropped — otherwise a partial
//! init + unwind would double-drop / read uninitialized memory. Arrays that are
//! always initialized atomically (the common case — array literals lower to a
//! single `Aggregate` rvalue) stay on the unconditional fast path (no per-element
//! flags, no perf regression).

use std::collections::{HashMap, HashSet};

use glyim_core::BinOp;
use glyim_core::IndexVec;
use glyim_core::Mutability;
use glyim_core::primitives::UintTy;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Const, ConstKind, Ty, TyCtx, TyCtxMut, TyKind};

// -----------------------------------------------------------------------------
// Dataflow: which locals are definitely initialized at each program point.
// -----------------------------------------------------------------------------

struct MaybeInitialized {
    entry: Vec<Vec<bool>>,
}

impl MaybeInitialized {
    fn compute(body: &Body) -> Self {
        let num_locals = body.locals.len();
        let num_blocks = body.basic_blocks.len();
        let mut entry = vec![vec![false; num_locals]; num_blocks];
        for i in 0..=body.arg_count {
            entry[0][i] = true;
        }
        let mut queue = std::collections::VecDeque::new();
        let mut changed = vec![true; num_blocks];
        queue.push_back(0);
        while let Some(bb_idx) = queue.pop_front() {
            let mut cur = entry[bb_idx].clone();
            let block = &body.basic_blocks[BasicBlockIdx::from_raw(bb_idx as u32)];
            for stmt in &block.statements {
                match &stmt.kind {
                    StatementKind::Assign(place, _) if place.projection.is_empty() => {
                        cur[place.local.to_raw() as usize] = true;
                    }
                    StatementKind::StorageLive(local) => {
                        cur[local.to_raw() as usize] = true;
                    }
                    StatementKind::StorageDead(local) => {
                        cur[local.to_raw() as usize] = false;
                    }
                    _ => {}
                }
            }
            for succ in super::cfg_simplify::terminator_successors(&block.terminator) {
                let succ_idx = succ.to_raw() as usize;
                let succ_entry = &mut entry[succ_idx];
                let mut changed_succ = false;
                for i in 0..num_locals {
                    if cur[i] && !succ_entry[i] {
                        succ_entry[i] = true;
                        changed_succ = true;
                    }
                }
                if changed_succ && changed[succ_idx] {
                    changed[succ_idx] = true;
                    queue.push_back(succ_idx);
                }
            }
        }
        MaybeInitialized { entry }
    }

    fn is_definitely_initialized(&self, block: BasicBlockIdx, local: LocalIdx) -> bool {
        self.entry[block.to_raw() as usize][local.to_raw() as usize]
    }
}

// -----------------------------------------------------------------------------
// Drop flags
// -----------------------------------------------------------------------------

struct DropFlags {
    flag_for_local: Vec<Option<LocalIdx>>,
    /// For arrays that are incrementally initialized via a loop (plan §15.2):
    /// map `arr_local -> (flag_array_local, len)`. The flag array is a
    /// `bool[len]` local; element `i` is set when `arr[i]` is assigned and
    /// consulted when dropping element `i`.
    per_element: HashMap<LocalIdx, (LocalIdx, u64)>,
}

impl DropFlags {
    fn new(ctx: &TyCtx, body: &Body, _analysis: &MaybeInitialized) -> Self {
        let mut flags = vec![None; body.locals.len()];
        let mut per_element: HashMap<LocalIdx, (LocalIdx, u64)> = HashMap::new();

        // Detect arrays built element-by-element via a loop: any assignment
        // `arr[i] = ...` (a `ProjectionElem::Index` onto the array local).
        // Atomic array literals lower to a single `Aggregate` rvalue and never
        // produce such assignments, so they stay on the fast path.
        //
        // A leading `Deref` (e.g. `*p[i] = x` where `p: &mut [T; N]`) is still
        // an element-wise assignment to the array behind the reference, so we
        // key on the place's base local (the local the projection is rooted at,
        // after stripping a leading `Deref`) — that is `place.local` itself.
        let mut loop_built: HashSet<LocalIdx> = HashSet::new();
        for block in body.basic_blocks.iter() {
            for stmt in &block.statements {
                if let StatementKind::Assign(place, _) = &stmt.kind {
                    let has_index = place
                        .projection
                        .iter()
                        .any(|p| matches!(p, ProjectionElem::Index(_)));
                    if has_index && ctx.needs_drop(place.ty(ctx, &body.locals)) {
                        loop_built.insert(place.local);
                    }
                }
            }
        }

        for (local, decl) in body.locals.iter_enumerated() {
            if !ctx.needs_drop(decl.ty) {
                continue;
            }
            if let TyKind::Array(_, count) = ctx.ty_kind(decl.ty) {
                let len = match &count.kind {
                    ConstKind::Uint(n) => *n as u64,
                    ConstKind::Int(n) if *n >= 0 => *n as u64,
                    _ => 0,
                };
                if len > 0 && loop_built.contains(&local) {
                    per_element.insert(local, (LocalIdx::from_raw(0), len));
                    continue;
                }
            }
            flags[local.to_raw() as usize] = Some(LocalIdx::from_raw(0));
        }
        DropFlags {
            flag_for_local: flags,
            per_element,
        }
    }

    fn create_flags(&mut self, ctx: &mut TyCtxMut, body: &mut Body) {
        for flag_opt in self.flag_for_local.iter_mut() {
            if flag_opt.is_some() {
                let flag_local = body.locals.push(LocalDecl {
                    ty: ctx.bool_ty(),
                    mutability: Mutability::Mut,
                    source_info: SourceInfo::new(Span::DUMMY),
                });
                *flag_opt = Some(flag_local);
                let entry_block = &mut body.basic_blocks[BasicBlockIdx::from_raw(0)];
                entry_block.statements.insert(
                    0,
                    Statement {
                        kind: StatementKind::StorageLive(flag_local),
                        source_info: SourceInfo::new(Span::DUMMY),
                    },
                );
                let init = Statement {
                    kind: StatementKind::Assign(
                        Place::new(flag_local),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(false),
                            ty: ctx.bool_ty(),
                            span: Span::DUMMY,
                        })),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                };
                entry_block.statements.insert(1, init);
            }
        }

        // Per-element flag arrays for incrementally-initialized arrays (§15.2).
        for (flag_arr, len) in self.per_element.values_mut() {
            let len = *len;
            if len == 0 {
                continue;
            }
            let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
            let flag_arr_ty = ctx.mk_ty(TyKind::Array(
                ctx.bool_ty(),
                Const {
                    kind: ConstKind::Uint(len.into()),
                    ty: usize_ty,
                },
            ));
            let flag_arr_local = body.locals.push(LocalDecl {
                ty: flag_arr_ty,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            });
            *flag_arr = flag_arr_local;
            let entry_block = &mut body.basic_blocks[BasicBlockIdx::from_raw(0)];
            entry_block.statements.insert(
                0,
                Statement {
                    kind: StatementKind::StorageLive(flag_arr_local),
                    source_info: SourceInfo::new(Span::DUMMY),
                },
            );
            let bool_ty = ctx.bool_ty();
            let false_elems: Vec<Operand> = (0..len)
                .map(|_| {
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(false),
                        ty: bool_ty,
                        span: Span::DUMMY,
                    })
                })
                .collect();
            let init = Statement {
                kind: StatementKind::Assign(
                    Place::new(flag_arr_local),
                    Rvalue::Aggregate(AggregateKind::Array(bool_ty), false_elems),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            };
            entry_block.statements.insert(1, init);
        }
    }

    fn get_flag(&self, local: LocalIdx) -> Option<LocalIdx> {
        let idx = local.to_raw() as usize;
        if idx < self.flag_for_local.len() {
            self.flag_for_local[idx]
        } else {
            None
        }
    }

    fn set_flag_stmt(flag: LocalIdx, value: bool, span: Span, ctx: &TyCtx) -> Statement {
        Statement {
            kind: StatementKind::Assign(
                Place::new(flag),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(value),
                    ty: ctx.bool_ty(),
                    span,
                })),
            ),
            source_info: SourceInfo::new(span),
        }
    }

    /// Set a per-element flag: `flag_arr[idx_local] = value`.
    fn set_flag_stmt_indexed(
        flag_arr: LocalIdx,
        idx_local: LocalIdx,
        value: bool,
        span: Span,
        ctx: &TyCtx,
    ) -> Statement {
        Statement {
            kind: StatementKind::Assign(
                Place {
                    local: flag_arr,
                    projection: vec![ProjectionElem::Index(idx_local)].into_boxed_slice(),
                },
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(value),
                    ty: ctx.bool_ty(),
                    span,
                })),
            ),
            source_info: SourceInfo::new(span),
        }
    }
}

// -----------------------------------------------------------------------------
// Main transformation
// -----------------------------------------------------------------------------

pub(crate) fn run(ctx: &mut TyCtxMut, body: &mut Body) {
    // `ctx` is mutable (needed for `mk_ty` allocation of flag-array types,
    // §15.2); `tc` is a frozen snapshot used for all read-only type queries.
    let tc = ctx.freeze();
    let analysis = MaybeInitialized::compute(body);
    let mut flags = DropFlags::new(&tc, body, &analysis);
    flags.create_flags(ctx, body);

    // Insert flag-setting after assignments.
    for block_idx in 0..body.basic_blocks.len() {
        let block = &mut body.basic_blocks[BasicBlockIdx::from_raw(block_idx as u32)];
        let mut new_stmts = Vec::new();
        for stmt in block.statements.drain(..) {
            let span = stmt.source_info.span;
            let mut is_assign_to_local = false;
            if let StatementKind::Assign(place, _) = &stmt.kind {
                is_assign_to_local = place.projection.is_empty();
            }
            let mut local = None;
            if is_assign_to_local {
                if let StatementKind::Assign(place, _) = &stmt.kind {
                    local = Some(place.local);
                }
            }
            new_stmts.push(stmt);
            if let Some(local) = local {
                if let Some(flag) = flags.get_flag(local) {
                    new_stmts.push(DropFlags::set_flag_stmt(flag, true, span, &tc));
                }
            }
            // §15.2: mark the per-element flag when an array element is assigned.
            if let Statement {
                kind: StatementKind::Assign(place, _),
                ..
            } = new_stmts.last().unwrap()
            {
                if let Some((flag_arr, _)) = flags.per_element.get(&place.local) {
                    let idx_local = place
                        .projection
                        .iter()
                        .find(|p| matches!(p, ProjectionElem::Index(_)));
                    if let Some(&ProjectionElem::Index(idx_local)) = idx_local {
                        new_stmts.push(DropFlags::set_flag_stmt_indexed(
                            *flag_arr, idx_local, true, span, &tc,
                        ));
                    }
                }
            }
        }
        block.statements = new_stmts;
    }

    // Transform Drop terminators.
    let mut new_blocks = Vec::new();
    let mut block_map: Vec<Option<usize>> = vec![None; body.basic_blocks.len()];

    for (old_idx, old_block) in body.basic_blocks.iter().enumerate() {
        let old_bb = BasicBlockIdx::from_raw(old_idx as u32);
        let terminator = &old_block.terminator;

        let new_term = match &terminator.kind {
            TerminatorKind::Drop {
                place,
                target,
                cleanup,
            } => {
                let ty = place.ty(&tc, &body.locals);
                if !tc.needs_drop(ty) {
                    TerminatorKind::Goto { target: *target }
                } else if let TyKind::Array(_elem_ty, count) = tc.ty_kind(ty) {
                    let len = match &count.kind {
                        ConstKind::Uint(n) => *n as u64,
                        ConstKind::Int(n) if *n >= 0 => *n as u64,
                        _ => 0,
                    };
                    if len == 0 {
                        TerminatorKind::Goto { target: *target }
                    } else {
                        let per_element = flags.per_element.get(&place.local).cloned();
                        let idx_local = body.locals.push(LocalDecl {
                            ty: count.ty,
                            mutability: Mutability::Mut,
                            source_info: SourceInfo::new(terminator.source_info.span),
                        });
                        let idx_place = Place::new(idx_local);
                        emit_array_drop_loop(
                            &tc,
                            place,
                            *target,
                            *cleanup,
                            old_block.is_cleanup,
                            terminator.source_info.clone(),
                            &mut new_blocks,
                            per_element,
                            count.ty,
                            idx_local,
                            idx_place,
                            len,
                        )
                    }
                } else if place.projection.is_empty() {
                    let local = place.local;
                    let definitely_init = analysis.is_definitely_initialized(old_bb, local);
                    if !definitely_init {
                        if let Some(flag_local) = flags.get_flag(local) {
                            let drop_block_idx = new_blocks.len();
                            let drop_block = BasicBlockIdx::from_raw(drop_block_idx as u32);
                            let clear_flag = DropFlags::set_flag_stmt(
                                flag_local,
                                false,
                                terminator.source_info.span,
                                &tc,
                            );
                            let drop_block_data = BasicBlockData {
                                statements: vec![clear_flag],
                                terminator: Terminator {
                                    kind: TerminatorKind::Drop {
                                        place: place.clone(),
                                        target: *target,
                                        cleanup: *cleanup,
                                    },
                                    source_info: terminator.source_info.clone(),
                                },
                                is_cleanup: old_block.is_cleanup,
                            };
                            new_blocks.push(drop_block_data);
                            TerminatorKind::SwitchInt {
                                discr: Operand::Copy(Place::new(flag_local)),
                                switch_ty: tc.bool_ty(),
                                targets: SwitchTargets::if_switch(drop_block, *target),
                            }
                        } else {
                            TerminatorKind::Drop {
                                place: place.clone(),
                                target: *target,
                                cleanup: *cleanup,
                            }
                        }
                    } else {
                        TerminatorKind::Drop {
                            place: place.clone(),
                            target: *target,
                            cleanup: *cleanup,
                        }
                    }
                } else {
                    TerminatorKind::Drop {
                        place: place.clone(),
                        target: *target,
                        cleanup: *cleanup,
                    }
                }
            }
            _ => terminator.kind.clone(),
        };

        let new_idx = new_blocks.len();
        block_map[old_idx] = Some(new_idx);
        new_blocks.push(BasicBlockData {
            statements: old_block.statements.clone(),
            terminator: Terminator {
                kind: new_term,
                source_info: terminator.source_info.clone(),
            },
            is_cleanup: old_block.is_cleanup,
        });
    }

    for block in &mut new_blocks {
        super::cfg_simplify::remap_terminator(block, &block_map);
    }

    body.basic_blocks = IndexVec::from_raw(new_blocks);
}

/// Emit the array-drop loop for a `Drop` on an array place.
///
/// `per_element` is `Some((flag_arr, len))` when the array is incrementally
/// initialized (plan §15.2): each element is dropped only if its per-element
/// flag is set. When `None`, the array is always fully initialized and we drop
/// every element unconditionally (the fast path).
fn emit_array_drop_loop(
    ctx: &TyCtx,
    place: &Place,
    target: BasicBlockIdx,
    cleanup: Option<BasicBlockIdx>,
    is_cleanup: bool,
    span: SourceInfo,
    new_blocks: &mut Vec<BasicBlockData>,
    per_element: Option<(LocalIdx, u64)>,
    count_ty: Ty,
    idx_local: LocalIdx,
    idx_place: Place,
    len: u64,
) -> TerminatorKind {
    let init_block_idx = new_blocks.len();
    let init_block = BasicBlockIdx::from_raw(init_block_idx as u32);
    let cond_block_idx = init_block_idx + 1;
    let cond_block = BasicBlockIdx::from_raw(cond_block_idx as u32);
    let body_block_idx = cond_block_idx + 1;
    let body_block = BasicBlockIdx::from_raw(body_block_idx as u32);
    let exit_block_idx = body_block_idx + 1;
    let exit_block = BasicBlockIdx::from_raw(exit_block_idx as u32);

    let init_block_data = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                idx_place.clone(),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(len.into()),
                    ty: count_ty,
                    span: span.span,
                })),
            ),
            source_info: span.clone(),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Goto { target: cond_block },
            source_info: span.clone(),
        },
        is_cleanup,
    };
    new_blocks.push(init_block_data);

    let cond_block_data = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: Operand::Copy(idx_place.clone()),
                switch_ty: count_ty,
                targets: SwitchTargets::new(vec![(0, exit_block)].into_boxed_slice(), body_block),
            },
            source_info: span.clone(),
        },
        is_cleanup,
    };
    new_blocks.push(cond_block_data);

    let dec_stmt = Statement {
        kind: StatementKind::Assign(
            idx_place.clone(),
            Rvalue::BinaryOp(
                BinOp::Sub,
                Box::new((
                    Operand::Copy(idx_place.clone()),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Uint(1),
                        ty: count_ty,
                        span: span.span,
                    }),
                )),
            ),
        ),
        source_info: span.clone(),
    };
    let elem_place = Place {
        local: place.local,
        projection: vec![ProjectionElem::Index(idx_local)].into_boxed_slice(),
    };

    if let Some((flag_arr, _)) = per_element {
        let drop_elem_block_idx = new_blocks.len();
        let drop_elem_block = BasicBlockIdx::from_raw(drop_elem_block_idx as u32);
        let drop_elem_data = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: elem_place,
                    target: cond_block,
                    cleanup,
                },
                source_info: span.clone(),
            },
            is_cleanup,
        };
        new_blocks.push(drop_elem_data);

        let flag_elem_place = Place {
            local: flag_arr,
            projection: vec![ProjectionElem::Index(idx_local)].into_boxed_slice(),
        };
        let body_block_data = BasicBlockData {
            statements: vec![dec_stmt],
            terminator: Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(flag_elem_place),
                    switch_ty: ctx.bool_ty(),
                    targets: SwitchTargets::if_switch(drop_elem_block, cond_block),
                },
                source_info: span.clone(),
            },
            is_cleanup,
        };
        new_blocks.push(body_block_data);
    } else {
        let body_block_data = BasicBlockData {
            statements: vec![dec_stmt],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: elem_place,
                    target: cond_block,
                    cleanup,
                },
                source_info: span.clone(),
            },
            is_cleanup,
        };
        new_blocks.push(body_block_data);
    }

    let exit_block_data = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Goto { target },
            source_info: span.clone(),
        },
        is_cleanup,
    };
    new_blocks.push(exit_block_data);

    TerminatorKind::Goto { target: init_block }
}

// (no standalone `needs_drop` here — it is the shared `TyCtx::needs_drop`)
