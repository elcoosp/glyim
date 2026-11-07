use glyim_core::IndexVec;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::TyCtx;

pub(crate) fn run(_ctx: &TyCtx, body: &mut Body) {
    let mut blocks = std::mem::take(&mut body.basic_blocks).into_raw();
    let mut changed = true;
    while changed {
        changed = false;
        let mut preds = vec![Vec::new(); blocks.len()];
        for (i, block) in blocks.iter().enumerate() {
            for succ in terminator_successors(&block.terminator) {
                let si = succ.to_raw() as usize;
                if si < blocks.len() {
                    preds[si].push(i);
                }
            }
        }
        let mut delete = vec![false; blocks.len()];
        for i in 1..blocks.len() {
            if delete[i] {
                continue;
            }
            if preds[i].len() == 1 {
                let p = preds[i][0];
                if !delete[p]
                    && matches!(&blocks[p].terminator.kind, TerminatorKind::Goto { target } if target.to_raw() as usize == i)
                {
                    let mut stmts = std::mem::take(&mut blocks[i].statements);
                    blocks[p].statements.append(&mut stmts);
                    let term = std::mem::replace(
                        &mut blocks[i].terminator,
                        Terminator {
                            kind: TerminatorKind::Return,
                            source_info: SourceInfo::new(Span::DUMMY),
                        },
                    );
                    blocks[p].terminator = term;
                    delete[i] = true;
                    changed = true;
                }
            }
        }
        if changed {
            let mut new_blocks = Vec::new();
            let mut remap = vec![None; blocks.len()];
            for (i, b) in blocks.into_iter().enumerate() {
                if !delete[i] {
                    remap[i] = Some(new_blocks.len());
                    new_blocks.push(b);
                }
            }
            for b in &mut new_blocks {
                remap_terminator(b, &remap);
            }
            blocks = new_blocks;
        }
    }
    body.basic_blocks = IndexVec::from_raw(blocks);
}

pub(crate) fn terminator_successors(term: &Terminator) -> Vec<BasicBlockIdx> {
    match &term.kind {
        TerminatorKind::Goto { target } => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => {
            let mut s: Vec<_> = targets.iter().map(|(_, t)| t).collect();
            s.push(targets.otherwise());
            s
        }
        TerminatorKind::Call {
            target, cleanup, ..
        } => {
            let mut s = Vec::new();
            if let Some(t) = target {
                s.push(*t);
            }
            if let Some(c) = cleanup {
                s.push(*c);
            }
            s
        }
        TerminatorKind::Assert {
            target, cleanup, ..
        } => {
            let mut s = vec![*target];
            if let Some(c) = cleanup {
                s.push(*c);
            }
            s
        }
        TerminatorKind::Drop {
            target, cleanup, ..
        } => {
            let mut s = vec![*target];
            if let Some(c) = cleanup {
                s.push(*c);
            }
            s
        }
        _ => vec![],
    }
}

pub(crate) fn remap_terminator(block: &mut BasicBlockData, remap: &[Option<usize>]) {
    let map = |idx: &BasicBlockIdx| -> BasicBlockIdx {
        let old = idx.to_raw() as usize;
        if let Some(&Some(new)) = remap.get(old) {
            BasicBlockIdx::from_raw(new as u32)
        } else {
            BasicBlockIdx::from_raw(0)
        }
    };
    let map_opt = |opt: &Option<BasicBlockIdx>| -> Option<BasicBlockIdx> { opt.as_ref().map(map) };
    match &mut block.terminator.kind {
        TerminatorKind::Goto { target } => *target = map(target),
        TerminatorKind::SwitchInt { targets, .. } => {
            let otherwise = map(&targets.otherwise());
            let branches: Vec<(u128, BasicBlockIdx)> =
                targets.iter().map(|(v, t)| (v, map(&t))).collect();
            *targets = SwitchTargets::new(branches.into_boxed_slice(), otherwise);
        }
        TerminatorKind::Call {
            target, cleanup, ..
        } => {
            *target = map_opt(target);
            *cleanup = map_opt(cleanup);
        }
        TerminatorKind::Assert {
            target, cleanup, ..
        } => {
            *target = map(target);
            *cleanup = map_opt(cleanup);
        }
        TerminatorKind::Drop {
            target, cleanup, ..
        } => {
            *target = map(target);
            *cleanup = map_opt(cleanup);
        }
        _ => {}
    }
}
