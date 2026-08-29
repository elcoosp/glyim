use glyim_mir::*;
use glyim_type::TyCtx;
use std::collections::HashSet;

/// Dead‑code elimination: removes assignments to locals that are never read.
pub(crate) fn run(_ctx: &TyCtx, body: &mut Body) {
    let used = collect_used_locals(body);
    for bb in 0..body.basic_blocks.len() {
        let block = &mut body.basic_blocks[BasicBlockIdx::from_raw(bb as u32)];
        block.statements.retain(|stmt| {
            if let StatementKind::Assign(place, _) = &stmt.kind
                && place.projection.is_empty()
            {
                return used.contains(&place.local);
            }
            true
        });
    }
}

fn collect_used_locals(body: &Body) -> HashSet<LocalIdx> {
    let mut used = HashSet::new();
    // The return place (_0) and function arguments are always considered used
    // because they communicate values to/from the caller.
    used.insert(LocalIdx::from_raw(0));
    for i in 0..body.arg_count {
        used.insert(LocalIdx::from_raw(u32::try_from(i + 1).unwrap_or(u32::MAX)));
    }
    for bb in 0..body.basic_blocks.len() {
        let block = &body.basic_blocks[BasicBlockIdx::from_raw(bb as u32)];
        for stmt in &block.statements {
            if let StatementKind::Assign(_, rvalue) = &stmt.kind {
                collect_uses_in_rvalue(rvalue, &mut used);
            }
        }
        collect_uses_in_terminator(&block.terminator, &mut used);
    }
    used
}

fn collect_operand_uses(op: &Operand, used: &mut HashSet<LocalIdx>) {
    match op {
        Operand::Copy(place) | Operand::Move(place) => {
            // A projection (e.g. `p.a`) still reads the base local, so the
            // local is used even when the projection list is non-empty. Dropping
            // it would delete the only store that feeds the projection.
            used.insert(place.local);
        }
        Operand::Constant(_) => {}
    }
}

fn collect_uses_in_rvalue(rv: &Rvalue, used: &mut HashSet<LocalIdx>) {
    match rv {
        Rvalue::Use(op) => collect_operand_uses(op, used),
        Rvalue::BinaryOp(_, box_ops) => {
            collect_operand_uses(&box_ops.0, used);
            collect_operand_uses(&box_ops.1, used);
        }
        Rvalue::Ref(place, _) => {
            used.insert(place.local);
        }
        Rvalue::Aggregate(_, operands) => {
            for op in operands {
                collect_operand_uses(op, used);
            }
        }
        Rvalue::Discriminant(place) | Rvalue::Len(place) => {
            used.insert(place.local);
        }
        Rvalue::Cast(_, op, _) => collect_operand_uses(op, used),
        Rvalue::Repeat(op, _) => collect_operand_uses(op, used),
        _ => {}
    }
}

fn collect_uses_in_terminator(term: &Terminator, used: &mut HashSet<LocalIdx>) {
    match &term.kind {
        TerminatorKind::Call {
            func,
            args,
            destination: _,
            target: _,
            cleanup: _,
        } => {
            collect_operand_uses(func, used);
            for arg in args {
                collect_operand_uses(arg, used);
            }
        }
        TerminatorKind::SwitchInt { discr, .. } => {
            collect_operand_uses(discr, used);
        }
        TerminatorKind::Assert { cond, .. } => {
            collect_operand_uses(cond, used);
        }
        _ => {}
    }
}
