use glyim_core::IndexVec;
use glyim_mir::*;
use glyim_type::TyCtx;
use std::collections::HashSet;

/// Eliminates basic blocks that are not reachable from the start block.
pub(crate) fn run(_ctx: &TyCtx, body: &mut Body) {
    let blocks: Vec<BasicBlockData> = std::mem::take(&mut body.basic_blocks).into_raw();
    let reachable = reachable_set(&blocks);
    if reachable.len() == blocks.len() {
        // Put the blocks back
        body.basic_blocks = IndexVec::from_vec(blocks);
        return;
    }

    let mut new_blocks = Vec::with_capacity(reachable.len());
    let mut old_to_new: Vec<Option<usize>> = vec![None; blocks.len()];
    for (i, block) in blocks.into_iter().enumerate() {
        if reachable.contains(&i) {
            old_to_new[i] = Some(new_blocks.len());
            new_blocks.push(block);
        }
    }

    for block in &mut new_blocks {
        super::cfg_simplify::remap_terminator(block, &old_to_new);
    }

    body.basic_blocks = IndexVec::from_vec(new_blocks);
}

fn reachable_set(blocks: &[BasicBlockData]) -> HashSet<usize> {
    let mut visited = HashSet::new();
    let mut stack = vec![0usize];
    while let Some(i) = stack.pop() {
        if visited.insert(i) {
            for succ in super::cfg_simplify::terminator_successors(&blocks[i].terminator) {
                stack.push(succ.to_raw() as usize);
            }
        }
    }
    visited
}
