//! Move analysis and partial-move tracking.
//!
//! Implements a forward dataflow analysis that tracks which places have been
//! moved out of at each program point. It supports:
//! - Whole-struct moves: moving a struct moves all its fields.
//! - Partial moves: moving one field of a struct leaves other fields usable.
//! - Copy types: moving a copy type does not produce a move.
//! - Reassignment: reassigning a moved local makes it usable again.
//!
//! The analysis works at the level of [`MovePath`]s, which represent prefixes
//! of places that can be independently moved. For a struct with fields 0 and 1,
//! the root local has move paths for each field.

use fixedbitset::FixedBitSet as BitSet;
use glyim_core::arena::IndexVec;
use glyim_diag::GlyimDiagnostic;
use glyim_mir::{
    BasicBlockIdx, Body, LocalDecl, LocalIdx, Operand, Place, ProjectionElem, Rvalue, StatementKind,
};
use glyim_type::TyCtx;
use smallvec::SmallVec;
use tracing::debug;

use crate::BorrowckCtx;
use crate::visitor::successor_blocks;

// ---------------------------------------------------------------------------
// Move path index
// ---------------------------------------------------------------------------

/// Index into the move path arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct MovePathIdx(u32);

impl MovePathIdx {
    #[inline]
    fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    #[inline]
    fn to_raw(self) -> u32 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Move path representation
// ---------------------------------------------------------------------------

/// A move path represents a place (or prefix of a place) that can be
/// independently moved.
#[derive(Clone, Debug)]
struct MovePath {
    /// The place this move path corresponds to.
    place: Place,
    /// Parent move path (e.g. the struct local for a field).
    #[allow(dead_code)]
    parent: Option<MovePathIdx>,
    /// Child move paths (e.g. fields of a struct).
    children: Vec<MovePathIdx>,
}

/// Arena of move paths for a single MIR body.
///
/// Maintains a root index for O(1) lookup of a local's root move path.
/// Child lookups traverse the children list of a parent, which is O(field_count)
/// and fast in practice (structs rarely have more than ~16 fields).
struct MovePathArena {
    paths: Vec<MovePath>,
    /// Maps `local.to_raw() as usize` → root `MovePathIdx` for that local.
    root_index: Vec<Option<MovePathIdx>>,
}

impl MovePathArena {
    fn new(num_locals: usize) -> Self {
        Self {
            paths: Vec::new(),
            root_index: vec![None; num_locals],
        }
    }

    fn push(&mut self, path: MovePath) -> MovePathIdx {
        let idx = MovePathIdx::from_raw(self.paths.len() as u32);
        self.paths.push(path);
        idx
    }

    fn get(&self, idx: MovePathIdx) -> &MovePath {
        &self.paths[idx.to_raw() as usize]
    }

    fn get_mut(&mut self, idx: MovePathIdx) -> &mut MovePath {
        &mut self.paths[idx.to_raw() as usize]
    }

    fn len(&self) -> usize {
        self.paths.len()
    }

    /// Find the move path for a given place.
    fn find(&mut self, place: &Place) -> Option<MovePathIdx> {
        let root_idx = *self.root_index.get(place.local.to_raw() as usize)?;

        if place.projection.is_empty() {
            return root_idx;
        }

        let mut current_idx = root_idx?;
        for proj_elem in place.projection.iter() {
            match proj_elem {
                ProjectionElem::Index(_) => {
                    // For Index projections, we treat them as the same path (all indices share a path).
                    // We'll just continue with the current path.
                    // However, we might want to create a path for each index? Usually all indices are treated as the same path.
                    // So we keep current_idx unchanged.
                }
                ProjectionElem::Downcast(variant_idx) => {
                    let current = self.get(current_idx);
                    let mut found = None;
                    for &child_idx in &current.children {
                        let child = self.get(child_idx);
                        if let Some(ProjectionElem::Downcast(v)) = child.place.projection.last()
                            && *v == *variant_idx
                        {
                            found = Some(child_idx);
                            break;
                        }
                    }
                    match found {
                        Some(idx) => current_idx = idx,
                        None => {
                            let mut new_proj = self.get(current_idx).place.projection.to_vec();
                            new_proj.push(ProjectionElem::Downcast(*variant_idx));
                            let new_path = MovePath {
                                place: Place {
                                    local: place.local,
                                    projection: new_proj.into_boxed_slice(),
                                },
                                parent: Some(current_idx),
                                children: Vec::new(),
                            };
                            let new_idx = self.push(new_path);
                            self.get_mut(current_idx).children.push(new_idx);
                            current_idx = new_idx;
                        }
                    }
                }
                ProjectionElem::Field(_) => {
                    let current = self.get(current_idx);
                    let mut found = None;
                    for &child_idx in &current.children {
                        let child = self.get(child_idx);
                        if let Some(ProjectionElem::Field(f)) = child.place.projection.last()
                            && let ProjectionElem::Field(g) = proj_elem
                            && f == g
                        {
                            found = Some(child_idx);
                            break;
                        }
                    }
                    match found {
                        Some(idx) => current_idx = idx,
                        None => return Some(current_idx),
                    }
                }
                ProjectionElem::ConstantIndex {
                    offset,
                    from_end: false,
                    ..
                } => {
                    let current = self.get(current_idx);
                    let mut found = None;
                    for &child_idx in &current.children {
                        let child = self.get(child_idx);
                        if let Some(ProjectionElem::ConstantIndex {
                            offset: o,
                            from_end: false,
                            ..
                        }) = child.place.projection.last()
                            && *o == *offset
                        {
                            found = Some(child_idx);
                            break;
                        }
                    }
                    match found {
                        Some(idx) => current_idx = idx,
                        None => {
                            let mut new_proj = self.get(current_idx).place.projection.to_vec();
                            new_proj.push(ProjectionElem::ConstantIndex {
                                offset: *offset,
                                min_length: 0,
                                from_end: false,
                            });
                            let new_path = MovePath {
                                place: Place {
                                    local: place.local,
                                    projection: new_proj.into_boxed_slice(),
                                },
                                parent: Some(current_idx),
                                children: Vec::new(),
                            };
                            let new_idx = self.push(new_path);
                            self.get_mut(current_idx).children.push(new_idx);
                            current_idx = new_idx;
                        }
                    }
                }
                ProjectionElem::ConstantIndex { from_end: true, .. }
                | ProjectionElem::Subslice { .. } => {
                    // For from_end constant indices and subslices, we track at the ancestor.
                    return Some(current_idx);
                }
                ProjectionElem::Deref => {
                    // Deref is tracked at the ancestor.
                    return Some(current_idx);
                }
            }
        }
        Some(current_idx)
    }

    /// Find the root move path for a local (no projections).
    fn find_root(&self, local: LocalIdx) -> Option<MovePathIdx> {
        self.root_index
            .get(local.to_raw() as usize)
            .copied()
            .flatten()
    }
}

// ---------------------------------------------------------------------------
// Building move paths
// ---------------------------------------------------------------------------

fn build_move_paths(body: &Body, ctx: &dyn BorrowckCtx) -> MovePathArena {
    let mut arena = MovePathArena::new(body.locals.len());

    for (local_idx, local_decl) in body.locals.iter_enumerated() {
        let ty = local_decl.ty;

        let root_idx = arena.push(MovePath {
            place: Place::new(local_idx),
            parent: None,
            children: Vec::new(),
        });
        arena.root_index[local_idx.to_raw() as usize] = Some(root_idx);

        let ty_ctx = ctx.ty_ctx();
        if let Some(field_count) = count_fields(ty, ty_ctx) {
            for field_idx in 0..field_count {
                let field_proj = ProjectionElem::Field(glyim_type::FieldIdx::from_raw(field_idx));
                let child_place = Place {
                    local: local_idx,
                    projection: Box::new([field_proj]),
                };
                let child_idx = arena.push(MovePath {
                    place: child_place,
                    parent: Some(root_idx),
                    children: Vec::new(),
                });
                arena.get_mut(root_idx).children.push(child_idx);
            }
        }
    }

    debug!(num_paths = arena.len(), "built move paths");
    arena
}

fn count_fields(ty: glyim_type::Ty, ctx: &TyCtx) -> Option<u32> {
    match ctx.ty_kind(ty) {
        glyim_type::TyKind::Tuple(substs) => Some(u32::from(substs.len())),
        glyim_type::TyKind::Adt(adt_id, _) => {
            ctx.adt_repr(*adt_id).map(|r| r.field_tys.len() as u32)
        }
        glyim_type::TyKind::Array(_, len) => {
            // Try to resolve the const length.
            match &len.kind {
                glyim_type::ConstKind::Uint(n) => Some(*n as u32),
                glyim_type::ConstKind::Int(n) => {
                    if *n >= 0 {
                        Some(*n as u32)
                    } else {
                        None
                    }
                }
                // For generic/associated consts, we could try to resolve via substitution,
                // but we don't have the substs here. Return None for now.
                _ => None,
            }
        }
        glyim_type::TyKind::Slice(_) => None,
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Move dataflow analysis
// ---------------------------------------------------------------------------
/// Result of the move dataflow analysis.
struct MoveAnalysisResult {
    moved_in: Vec<BitSet>,
    #[allow(dead_code)]
    moved_out: Vec<BitSet>,
    /// For each basic block, the set of move path indices that have been
    /// StorageDead'd (storage deallocated). Unlike regular moves, using a
    /// dead local is always an error, even for Copy types.
    dead_in: Vec<BitSet>,
    #[allow(dead_code)]
    dead_out: Vec<BitSet>,
    move_paths: MovePathArena,
}

fn record_move(move_paths: &mut MovePathArena, mp_idx: MovePathIdx, moved: &mut BitSet) {
    moved.insert(mp_idx.to_raw() as usize);
    for &child in &move_paths.get(mp_idx).children {
        moved.insert(child.to_raw() as usize);
    }
}

fn record_init(move_paths: &mut MovePathArena, mp_idx: MovePathIdx, inits: &mut BitSet) {
    inits.insert(mp_idx.to_raw() as usize);
    for &child in &move_paths.get(mp_idx).children {
        inits.insert(child.to_raw() as usize);
    }
}

fn compute_move_dataflow(
    body: &Body,
    mut move_paths: MovePathArena,
    ctx: &dyn BorrowckCtx,
) -> MoveAnalysisResult {
    let num_blocks = body.basic_blocks.len();
    let num_paths = move_paths.len();

    let mut block_moves: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();
    let mut block_inits: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();
    // Track StorageDead separately — applies even to Copy types
    let mut block_deads: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();
    let mut block_dead_inits: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();

    for (block_idx, block_data) in body.basic_blocks.iter_enumerated() {
        let bi = block_idx.to_raw() as usize;
        for stmt in &block_data.statements {
            collect_stmt_move_effects(
                stmt,
                &mut move_paths,
                &mut block_moves[bi],
                &mut block_inits[bi],
                &mut block_deads[bi],
                &mut block_dead_inits[bi],
                ctx,
                &body.locals,
            );
        }
        // A `Call` terminator writes its `destination` local, which behaves
        // like an assignment (it (re)initializes that move path). The
        // dataflow below must treat it as an init so that a value produced by
        // a `Call` and then moved in a successor block is not falsely reported
        // as "use of moved value" when the producing block is reached through a
        // loop back-edge (its `moved_in` carries the value as moved from the
        // previous iteration, because the dataflow only saw statements, never
        // the terminator write). This previously broke the async state-machine
        // desugar's poll loop, where every `.await`'s future is a `Call`
        // destination that is immediately moved into the suspended state.
        if let glyim_mir::TerminatorKind::Call { destination, .. } = &block_data.terminator.kind
            && let Some(mp_idx) = move_paths.find(destination)
        {
            record_init(&mut move_paths, mp_idx, &mut block_inits[bi]);
            record_init(&mut move_paths, mp_idx, &mut block_dead_inits[bi]);
        }
    }

    let mut predecessors: Vec<Vec<BasicBlockIdx>> = vec![Vec::new(); num_blocks];
    for (block_idx, block_data) in body.basic_blocks.iter_enumerated() {
        for succ in successor_blocks(&block_data.terminator.kind) {
            predecessors[succ.to_raw() as usize].push(block_idx);
        }
    }

    let mut moved_in: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();
    let mut moved_out: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();
    let mut dead_in: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();
    let mut dead_out: Vec<BitSet> = (0..num_blocks)
        .map(|_| BitSet::with_capacity(num_paths))
        .collect();

    let mut worklist: Vec<usize> = (0..num_blocks).collect();
    let mut in_worklist = BitSet::with_capacity(num_blocks);
    for i in 0..num_blocks {
        in_worklist.insert(i);
    }

    while let Some(bi) = worklist.pop() {
        in_worklist.remove(bi);

        // --- Moved dataflow ---
        let mut new_moved_in = BitSet::with_capacity(num_paths);
        if bi != 0 {
            for &pred in &predecessors[bi] {
                new_moved_in.union_with(&moved_out[pred.to_raw() as usize]);
            }
        }
        let mut new_moved_out = new_moved_in.clone();
        new_moved_out.union_with(&block_moves[bi]);
        new_moved_out.difference_with(&block_inits[bi]);

        // --- Dead dataflow ---
        let mut new_dead_in = BitSet::with_capacity(num_paths);
        if bi != 0 {
            for &pred in &predecessors[bi] {
                new_dead_in.union_with(&dead_out[pred.to_raw() as usize]);
            }
        }
        let mut new_dead_out = new_dead_in.clone();
        new_dead_out.union_with(&block_deads[bi]);
        new_dead_out.difference_with(&block_dead_inits[bi]);

        if new_moved_in != moved_in[bi]
            || new_moved_out != moved_out[bi]
            || new_dead_in != dead_in[bi]
            || new_dead_out != dead_out[bi]
        {
            moved_in[bi] = new_moved_in;
            moved_out[bi] = new_moved_out;
            dead_in[bi] = new_dead_in;
            dead_out[bi] = new_dead_out;

            let block_idx = BasicBlockIdx::from_raw(bi as u32);
            let block_data = &body.basic_blocks[block_idx];
            for succ in successor_blocks(&block_data.terminator.kind) {
                let si = succ.to_raw() as usize;
                if !in_worklist.contains(si) {
                    in_worklist.insert(si);
                    worklist.push(si);
                }
            }
        }
    }

    MoveAnalysisResult {
        moved_in,
        moved_out,
        dead_in,
        dead_out,
        move_paths,
    }
}
#[allow(clippy::too_many_arguments)]
fn collect_stmt_move_effects(
    stmt: &glyim_mir::Statement,
    move_paths: &mut MovePathArena,
    moves: &mut BitSet,
    inits: &mut BitSet,
    deads: &mut BitSet,
    dead_inits: &mut BitSet,
    ctx: &dyn BorrowckCtx,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
) {
    match &stmt.kind {
        StatementKind::Assign(dest, rvalue) => {
            if let Some(mp_idx) = move_paths.find(dest) {
                record_init(move_paths, mp_idx, inits);
                record_init(move_paths, mp_idx, dead_inits);
            }
            collect_rvalue_move_operands(rvalue, move_paths, moves, ctx, local_decls);
        }
        StatementKind::StorageLive(local) => {
            if let Some(mp_idx) = move_paths.find_root(*local) {
                record_init(move_paths, mp_idx, inits);
                record_init(move_paths, mp_idx, dead_inits);
            }
        }
        StatementKind::StorageDead(local) => {
            if let Some(mp_idx) = move_paths.find_root(*local) {
                // StorageDead is both a move (value is gone) and a dead (storage is gone).
                // Dead applies even to Copy types.
                record_move(move_paths, mp_idx, moves);
                record_move(move_paths, mp_idx, deads);
            }
        }
        StatementKind::Nop => {}
    }
}

fn collect_rvalue_move_operands(
    rvalue: &Rvalue,
    move_paths: &mut MovePathArena,
    moves: &mut BitSet,
    ctx: &dyn BorrowckCtx,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
) {
    match rvalue {
        Rvalue::Use(operand) => {
            collect_operand_move(operand, move_paths, moves, ctx, local_decls);
        }
        Rvalue::Ref(_, _) => {}
        Rvalue::BinaryOp(_, pair) => {
            let (left, right) = pair.as_ref();
            collect_operand_move(left, move_paths, moves, ctx, local_decls);
            collect_operand_move(right, move_paths, moves, ctx, local_decls);
        }
        Rvalue::UnaryOp(_, operand) => {
            collect_operand_move(operand, move_paths, moves, ctx, local_decls);
        }
        Rvalue::Aggregate(_, operands) => {
            for op in operands {
                collect_operand_move(op, move_paths, moves, ctx, local_decls);
            }
        }
        Rvalue::Discriminant(_) | Rvalue::Len(_) => {}
        Rvalue::Cast(_, operand, _) => {
            collect_operand_move(operand, move_paths, moves, ctx, local_decls);
        }
        Rvalue::Repeat(operand, _) => {
            collect_operand_move(operand, move_paths, moves, ctx, local_decls);
        }
    }
}

fn collect_operand_move(
    operand: &Operand,
    move_paths: &mut MovePathArena,
    moves: &mut BitSet,
    ctx: &dyn BorrowckCtx,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
) {
    match operand {
        Operand::Move(place) => {
            let place_ty = place.ty(ctx.ty_ctx(), local_decls);
            if !ctx.is_copy(place_ty)
                && let Some(mp_idx) = move_paths.find(place)
            {
                record_move(move_paths, mp_idx, moves);
            }
        }
        Operand::Copy(_) | Operand::Constant(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Per-statement move state
// ---------------------------------------------------------------------------
fn compute_stmt_moved(
    body: &Body,
    block: BasicBlockIdx,
    moved_in: &BitSet,
    dead_in: &BitSet,
    move_paths: &mut MovePathArena,
    ctx: &dyn BorrowckCtx,
) -> (Vec<BitSet>, Vec<BitSet>) {
    let block_data = &body.basic_blocks[block];
    let num_stmts = block_data.statements.len();
    let num_paths = move_paths.len();

    let mut moved = vec![BitSet::with_capacity(num_paths); num_stmts + 1];
    let mut dead = vec![BitSet::with_capacity(num_paths); num_stmts + 1];
    moved[0] = moved_in.clone();
    dead[0] = dead_in.clone();

    for i in 0..num_stmts {
        let mut current_moved = moved[i].clone();
        let mut current_dead = dead[i].clone();
        apply_stmt_move_effects(
            &block_data.statements[i],
            move_paths,
            &mut current_moved,
            &mut current_dead,
            ctx,
            &body.locals,
        );
        moved[i + 1] = current_moved;
        dead[i + 1] = current_dead;
    }

    (moved, dead)
}

fn apply_stmt_move_effects(
    stmt: &glyim_mir::Statement,
    move_paths: &mut MovePathArena,
    moved: &mut BitSet,
    dead: &mut BitSet,
    ctx: &dyn BorrowckCtx,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
) {
    match &stmt.kind {
        StatementKind::Assign(dest, rvalue) => {
            if let Some(mp_idx) = move_paths.find(dest) {
                moved.remove(mp_idx.to_raw() as usize);
                dead.remove(mp_idx.to_raw() as usize);
                for &child in &move_paths.get(mp_idx).children {
                    moved.remove(child.to_raw() as usize);
                    dead.remove(child.to_raw() as usize);
                }
            }
            collect_rvalue_move_operands(rvalue, move_paths, moved, ctx, local_decls);
        }
        StatementKind::StorageLive(local) => {
            if let Some(mp_idx) = move_paths.find_root(*local) {
                moved.remove(mp_idx.to_raw() as usize);
                dead.remove(mp_idx.to_raw() as usize);
                for &child in &move_paths.get(mp_idx).children {
                    moved.remove(child.to_raw() as usize);
                    dead.remove(child.to_raw() as usize);
                }
            }
        }
        StatementKind::StorageDead(local) => {
            if let Some(mp_idx) = move_paths.find_root(*local) {
                record_move(move_paths, mp_idx, moved);
                record_move(move_paths, mp_idx, dead);
            }
        }
        StatementKind::Nop => {}
    }
}

// ---------------------------------------------------------------------------
// Use-after-move checking
// ---------------------------------------------------------------------------

pub(crate) fn check_moves(ctx: &dyn BorrowckCtx, body: &Body) -> Vec<GlyimDiagnostic> {
    let mut errors = Vec::new();

    let move_paths = build_move_paths(body, ctx);
    if move_paths.len() == 0 {
        return errors;
    }

    let mut move_result = compute_move_dataflow(body, move_paths, ctx);

    for (block_idx, block_data) in body.basic_blocks.iter_enumerated() {
        let block_usize = block_idx.to_raw() as usize;
        let moved_in = &move_result.moved_in[block_usize];
        let dead_in = &move_result.dead_in[block_usize];

        let (stmt_moved, stmt_dead) = compute_stmt_moved(
            body,
            block_idx,
            moved_in,
            dead_in,
            &mut move_result.move_paths,
            ctx,
        );

        for (stmt_idx, stmt) in block_data.statements.iter().enumerate() {
            let current_moved = &stmt_moved[stmt_idx];
            let current_dead = &stmt_dead[stmt_idx];
            check_stmt_use_after_move(
                stmt,
                block_idx,
                current_moved,
                current_dead,
                &mut move_result.move_paths,
                ctx,
                &body.locals,
                &mut errors,
            );
        }
    }

    errors
}
enum UsedPlace<'a> {
    Operand(&'a Place),
    Borrow(&'a Place),
    Inspect(&'a Place),
}

#[allow(clippy::too_many_arguments)]
fn check_stmt_use_after_move(
    stmt: &glyim_mir::Statement,
    block_idx: glyim_mir::BasicBlockIdx,
    moved: &BitSet,
    dead: &BitSet,
    move_paths: &mut MovePathArena,
    ctx: &dyn BorrowckCtx,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
    errors: &mut Vec<GlyimDiagnostic>,
) {
    let used_places = collect_used_places(stmt);
    for used_place in used_places {
        check_place_use_after_move(
            &used_place,
            stmt,
            block_idx,
            moved,
            dead,
            move_paths,
            ctx,
            local_decls,
            errors,
        );
    }
}

fn collect_used_places(stmt: &glyim_mir::Statement) -> SmallVec<[UsedPlace<'_>; 4]> {
    let mut places = SmallVec::new();
    if let StatementKind::Assign(_, rvalue) = &stmt.kind {
        collect_rvalue_used_places(rvalue, &mut places);
    }
    places
}

fn collect_rvalue_used_places<'a>(rvalue: &'a Rvalue, places: &mut SmallVec<[UsedPlace<'a>; 4]>) {
    match rvalue {
        Rvalue::Use(operand) => {
            if let Operand::Copy(place) | Operand::Move(place) = operand {
                places.push(UsedPlace::Operand(place));
            }
        }
        Rvalue::Ref(place, _) => {
            places.push(UsedPlace::Borrow(place));
        }
        Rvalue::BinaryOp(_, pair) => {
            let (left, right) = pair.as_ref();
            if let Operand::Copy(p) | Operand::Move(p) = left {
                places.push(UsedPlace::Operand(p));
            }
            if let Operand::Copy(p) | Operand::Move(p) = right {
                places.push(UsedPlace::Operand(p));
            }
        }
        Rvalue::UnaryOp(_, operand) => {
            if let Operand::Copy(place) | Operand::Move(place) = operand {
                places.push(UsedPlace::Operand(place));
            }
        }
        Rvalue::Aggregate(_, operands) => {
            for op in operands {
                if let Operand::Copy(place) | Operand::Move(place) = op {
                    places.push(UsedPlace::Operand(place));
                }
            }
        }
        Rvalue::Discriminant(place) | Rvalue::Len(place) => {
            places.push(UsedPlace::Inspect(place));
        }
        Rvalue::Cast(_, operand, _) => {
            if let Operand::Copy(place) | Operand::Move(place) = operand {
                places.push(UsedPlace::Operand(place));
            }
        }
        Rvalue::Repeat(operand, _) => {
            if let Operand::Copy(place) | Operand::Move(place) = operand {
                places.push(UsedPlace::Operand(place));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_place_use_after_move(
    used_place: &UsedPlace<'_>,
    stmt: &glyim_mir::Statement,
    _block_idx: glyim_mir::BasicBlockIdx,
    moved: &BitSet,
    dead: &BitSet,
    move_paths: &mut MovePathArena,
    ctx: &dyn BorrowckCtx,
    local_decls: &IndexVec<LocalIdx, LocalDecl>,
    errors: &mut Vec<GlyimDiagnostic>,
) {
    let place = match used_place {
        UsedPlace::Operand(p) => *p,
        UsedPlace::Borrow(p) => *p,
        UsedPlace::Inspect(p) => *p,
    };

    // Check use-after-dead FIRST — this applies regardless of Copy status.
    // StorageDead means the local's storage is deallocated; any subsequent
    // use is undefined behaviour, even for Copy types.
    if let Some(mp_idx) = move_paths.find(place)
        && dead.contains(mp_idx.to_raw() as usize)
    {
        let name = ctx.local_name(place.local);
        let msg = if place.projection.is_empty() {
            format!("use of dead local `{name}`")
        } else {
            format!("use of dead local `{name}.*`")
        };
        errors.push(GlyimDiagnostic::borrow_error(stmt.source_info.span, msg));
        return;
    }
    // For a bare local, also check if any field has been dead'd
    if place.projection.is_empty()
        && let Some(root_idx) = move_paths.find_root(place.local)
    {
        let any_child_dead = move_paths
            .get(root_idx)
            .children
            .iter()
            .any(|child| dead.contains(child.to_raw() as usize));
        if any_child_dead {
            let name = ctx.local_name(place.local);
            let msg = format!("use of partially dead local `{name}`");
            errors.push(GlyimDiagnostic::borrow_error(stmt.source_info.span, msg));
            return;
        }
    }

    // Check use-after-move — only for non-Copy types.
    if let UsedPlace::Operand(p) = used_place {
        let place_ty = p.ty(ctx.ty_ctx(), local_decls);
        if ctx.is_copy(place_ty) {
            return;
        }
    }

    if let Some(mp_idx) = move_paths.find(place)
        && moved.contains(mp_idx.to_raw() as usize)
    {
        let name = ctx.local_name(place.local);
        let msg = if place.projection.is_empty() {
            format!("use of moved value: `{name}`")
        } else {
            format!("use of partially moved value: `{name}.*`")
        };
        errors.push(GlyimDiagnostic::borrow_error(stmt.source_info.span, msg));
        return;
    }

    if place.projection.is_empty()
        && let Some(root_idx) = move_paths.find_root(place.local)
    {
        let any_child_moved = move_paths
            .get(root_idx)
            .children
            .iter()
            .any(|child| moved.contains(child.to_raw() as usize));
        if any_child_moved {
            let name = ctx.local_name(place.local);
            let msg = format!("use of partially moved value: `{name}`");
            errors.push(GlyimDiagnostic::borrow_error(stmt.source_info.span, msg));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::primitives::UintTy;
    use glyim_type::{Const, ConstKind, TyCtxMut, TyKind};

    #[test]
    fn test_count_fields_array() {
        let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
        let elem_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
        let count = Const {
            kind: ConstKind::Uint(5),
            ty: ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        let array_ty = ctx_mut.mk_ty(TyKind::Array(elem_ty, count));
        let ctx = ctx_mut.freeze();

        assert_eq!(count_fields(array_ty, &ctx), Some(5));
    }

    #[test]
    fn test_count_fields_slice() {
        let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
        let elem_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
        let slice_ty = ctx_mut.mk_ty(TyKind::Slice(elem_ty));
        let ctx = ctx_mut.freeze();

        assert_eq!(count_fields(slice_ty, &ctx), None);
    }
}
