//! Constant propagation with dataflow analysis and cross-block propagation.
//! Uses a worklist algorithm to propagate constant values across basic blocks.
//! Supports Int, Uint, Bool, Char, and Float constants.

use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Ty, TyCtx};
use std::collections::HashMap;

type BlockMap = HashMap<LocalIdx, Option<MirConst>>;

fn const_eq(a: &MirConst, b: &MirConst) -> bool {
    if std::mem::discriminant(&a.kind) != std::mem::discriminant(&b.kind) {
        return false;
    }
    if a.ty != b.ty {
        return false;
    }
    match (&a.kind, &b.kind) {
        (MirConstKind::Int(a_val), MirConstKind::Int(b_val)) => a_val == b_val,
        (MirConstKind::Uint(a_val), MirConstKind::Uint(b_val)) => a_val == b_val,
        (MirConstKind::Bool(a_val), MirConstKind::Bool(b_val)) => a_val == b_val,
        (MirConstKind::Char(a_val), MirConstKind::Char(b_val)) => a_val == b_val,
        (MirConstKind::FloatBits(a_val), MirConstKind::FloatBits(b_val)) => a_val == b_val,
        (MirConstKind::Unit, MirConstKind::Unit) => true,
        (MirConstKind::Aggregate(a), MirConstKind::Aggregate(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| const_eq(x, y))
        }
        _ => false,
    }
}

fn maps_equal(a: &BlockMap, b: &BlockMap) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (k, v) in a {
        match b.get(k) {
            None => return false,
            Some(bv) => match (v, bv) {
                (None, None) => continue,
                (Some(c1), Some(c2)) => {
                    if !const_eq(c1, c2) {
                        return false;
                    }
                }
                _ => return false,
            },
        }
    }
    // Also check keys in b that are not in a (though len check already covers it, but do it for safety)
    for k in b.keys() {
        if !a.contains_key(k) {
            return false;
        }
    }
    true
}

fn merge_maps(mut into: BlockMap, other: &BlockMap) -> BlockMap {
    for (local, other_val) in other {
        match into.get(local) {
            None => {
                into.insert(*local, other_val.clone());
            }
            Some(existing) => {
                let equal = match (existing, other_val) {
                    (None, None) => true,
                    (Some(e), Some(o)) => const_eq(e, o),
                    _ => false,
                };
                if !equal {
                    into.insert(*local, None);
                }
            }
        }
    }
    into
}

fn operand_to_const(op: &Operand, locals: &BlockMap) -> Option<MirConst> {
    match op {
        Operand::Constant(c) => Some(c.clone()),
        Operand::Copy(place) | Operand::Move(place) => {
            if place.projection.is_empty() {
                locals.get(&place.local).and_then(|opt| opt.clone())
            } else {
                None
            }
        }
    }
}

fn evaluate_rvalue_to_const(rv: &Rvalue, locals: &BlockMap, ctx: &TyCtx, ty: Ty) -> Option<MirConst> {
    match rv {
        Rvalue::Use(op) => operand_to_const(op, locals),
        Rvalue::BinaryOp(op, box_ops) => {
            let left = operand_to_const(&box_ops.0, locals)?;
            let right = operand_to_const(&box_ops.1, locals)?;
            match (left.kind, right.kind) {
                (MirConstKind::Int(l), MirConstKind::Int(r)) => {
                    let result = match op {
                        glyim_core::primitives::BinOp::Add => l + r,
                        glyim_core::primitives::BinOp::Sub => l - r,
                        glyim_core::primitives::BinOp::Mul => l * r,
                        glyim_core::primitives::BinOp::Div => {
                            if r != 0 {
                                l / r
                            } else {
                                0
                            }
                        }
                        glyim_core::primitives::BinOp::Rem => {
                            if r != 0 {
                                l % r
                            } else {
                                0
                            }
                        }
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::Int(result),
                        ty: left.ty,
                        span: left.span,
                    })
                }
                (MirConstKind::Uint(l), MirConstKind::Uint(r)) => {
                    let result = match op {
                        glyim_core::primitives::BinOp::Add => l + r,
                        glyim_core::primitives::BinOp::Sub => l - r,
                        glyim_core::primitives::BinOp::Mul => l * r,
                        glyim_core::primitives::BinOp::Div => {
                            if r != 0 {
                                l / r
                            } else {
                                0
                            }
                        }
                        glyim_core::primitives::BinOp::Rem => {
                            if r != 0 {
                                l % r
                            } else {
                                0
                            }
                        }
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::Uint(result),
                        ty: left.ty,
                        span: left.span,
                    })
                }
                (MirConstKind::Bool(l), MirConstKind::Bool(r)) => {
                    let result = match op {
                        glyim_core::primitives::BinOp::And => l && r,
                        glyim_core::primitives::BinOp::Or => l || r,
                        glyim_core::primitives::BinOp::Eq => l == r,
                        glyim_core::primitives::BinOp::Ne => l != r,
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::Bool(result),
                        ty: left.ty,
                        span: left.span,
                    })
                }
                (MirConstKind::FloatBits(l), MirConstKind::FloatBits(r)) => {
                    let lf = f64::from_bits(l);
                    let rf = f64::from_bits(r);
                    let result = match op {
                        glyim_core::primitives::BinOp::Add => lf + rf,
                        glyim_core::primitives::BinOp::Sub => lf - rf,
                        glyim_core::primitives::BinOp::Mul => lf * rf,
                        glyim_core::primitives::BinOp::Div => {
                            if rf != 0.0 {
                                lf / rf
                            } else {
                                0.0
                            }
                        }
                        glyim_core::primitives::BinOp::Eq => {
                            if lf == rf {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        glyim_core::primitives::BinOp::Ne => {
                            if lf != rf {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        glyim_core::primitives::BinOp::Lt => {
                            if lf < rf {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        glyim_core::primitives::BinOp::Gt => {
                            if lf > rf {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        glyim_core::primitives::BinOp::LtEq => {
                            if lf <= rf {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        glyim_core::primitives::BinOp::GtEq => {
                            if lf >= rf {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::FloatBits(result.to_bits()),
                        ty: left.ty,
                        span: left.span,
                    })
                }
                _ => None,
            }
        }
        Rvalue::UnaryOp(op, operand) => {
            let c = operand_to_const(operand, locals)?;
            match c.kind {
                MirConstKind::Int(v) => {
                    let result = match op {
                        glyim_core::primitives::UnOp::Neg => -v,
                        glyim_core::primitives::UnOp::Not => !v,
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::Int(result),
                        ty: c.ty,
                        span: c.span,
                    })
                }
                MirConstKind::Uint(v) => {
                    let result = match op {
                        glyim_core::primitives::UnOp::Not => !v,
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::Uint(result),
                        ty: c.ty,
                        span: c.span,
                    })
                }
                MirConstKind::Bool(v) => {
                    let result = match op {
                        glyim_core::primitives::UnOp::Not => !v,
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::Bool(result),
                        ty: c.ty,
                        span: c.span,
                    })
                }
                MirConstKind::FloatBits(v) => {
                    let f = f64::from_bits(v);
                    let result = match op {
                        glyim_core::primitives::UnOp::Neg => -f,
                        _ => return None,
                    };
                    Some(MirConst {
                        kind: MirConstKind::FloatBits(result.to_bits()),
                        ty: c.ty,
                        span: c.span,
                    })
                }
                _ => None,
            }
        }
        Rvalue::Aggregate(kind, operands) => {
            // Handle aggregates of constants (tuples, arrays, structs) — plan
            // §15.3: generalize beyond the empty-tuple (Unit) case. If every
            // operand is itself a constant, fold the whole aggregate into a
            // single `MirConstKind::Aggregate`.
            let mut field_consts = Vec::with_capacity(operands.len());
            for op in operands {
                if let Some(c) = operand_to_const(op, locals) {
                    field_consts.push(c);
                } else {
                    return None;
                }
            }
            match kind {
                AggregateKind::Tuple | AggregateKind::Array(_) | AggregateKind::Adt(_, _, _) => {
                    if field_consts.is_empty() {
                        // Empty tuple is the unit value.
                        Some(MirConst {
                            kind: MirConstKind::Unit,
                            ty: ctx.unit_ty(),
                            span: Span::DUMMY,
                        })
                    } else {
                        Some(MirConst {
                            kind: MirConstKind::Aggregate(field_consts),
                            ty,
                            span: Span::DUMMY,
                        })
                    }
                }
                _ => None,
            }
        }
        Rvalue::Ref(_place, _borrow_kind) => {
            // References to constants are not representable; return None.
            None
        }
        Rvalue::Cast(_, op, _) => operand_to_const(op, locals),
        Rvalue::Repeat(op, _) => operand_to_const(op, locals),
        Rvalue::Discriminant(_) | Rvalue::Len(_) => None,
    }
}

fn replace_operand(op: &mut Operand, locals: &BlockMap) -> bool {
    match op {
        Operand::Copy(place) | Operand::Move(place) => {
            if place.projection.is_empty()
                && let Some(Some(c)) = locals.get(&place.local)
            {
                *op = Operand::Constant(c.clone());
                return true;
            }
            false
        }
        Operand::Constant(_) => false,
    }
}

fn replace_in_rvalue(rv: &mut Rvalue, locals: &BlockMap) -> bool {
    match rv {
        Rvalue::Use(op) => replace_operand(op, locals),
        Rvalue::BinaryOp(_, box_ops) => {
            let a = replace_operand(&mut box_ops.0, locals);
            let b = replace_operand(&mut box_ops.1, locals);
            a || b
        }
        Rvalue::UnaryOp(_, op) => replace_operand(op, locals),
        Rvalue::Ref(_, _) => false,
        Rvalue::Aggregate(_, operands) => {
            let mut changed = false;
            for op in operands {
                changed = replace_operand(op, locals) || changed;
            }
            changed
        }
        Rvalue::Discriminant(_) | Rvalue::Len(_) => false,
        Rvalue::Cast(_, op, _) => replace_operand(op, locals),
        Rvalue::Repeat(op, _) => replace_operand(op, locals),
    }
}

pub(crate) fn run(ctx: &TyCtx, body: &mut Body) {
    let num_blocks = body.basic_blocks.len();
    if num_blocks == 0 {
        return;
    }

    let mut preds = vec![Vec::new(); num_blocks];
    for i in 0..num_blocks {
        let bb = BasicBlockIdx::from_raw(i as u32);
        let block = &body.basic_blocks[bb];
        for succ in super::cfg_simplify::terminator_successors(&block.terminator) {
            let succ_idx = succ.to_raw() as usize;
            if succ_idx < num_blocks {
                preds[succ_idx].push(i);
            }
        }
    }

    let mut in_maps: Vec<Option<BlockMap>> = vec![None; num_blocks];
    let mut changed = true;
    let mut iteration = 0;
    const MAX_ITERATIONS: usize = 1000;

    while changed && iteration < MAX_ITERATIONS {
        changed = false;
        iteration += 1;
        for bb_idx in 0..num_blocks {
            let mut incoming = BlockMap::new();
            for &pred_idx in &preds[bb_idx] {
                if let Some(ref pred_out) = in_maps[pred_idx] {
                    incoming = merge_maps(incoming, pred_out);
                }
            }
            if preds[bb_idx].is_empty() && bb_idx != 0 {
                continue;
            }

            let mut out = incoming.clone();
            let block = &body.basic_blocks[BasicBlockIdx::from_raw(bb_idx as u32)];
            // Locals *defined* (assigned) in this block. A local defined with a
            // non-constant rvalue must be recorded as `None` (wild) in `out`,
            // NOT left absent. "Absent" means "this block does not define the
            // local, so its value flows in from predecessors"; "present as None"
            // means "this block defines it but the value is not a compile-time
            // constant". Treating a defined-non-constant local as absent lets
            // `merge_maps` adopt a sibling predecessor's constant and fold a
            // phi (e.g. `match { A(v) => v, B => 0 }` → `move(local1)`) into a
            // wrong constant -- a silent miscompile (constant-propagation over
            // a divergent merge).
            let mut defined: std::collections::HashSet<LocalIdx> = std::collections::HashSet::new();
            for stmt in &block.statements {
                if let StatementKind::Assign(place, rvalue) = &stmt.kind {
                    out.remove(&place.local);
                    defined.insert(place.local);
                    if place.projection.is_empty()
                        && let Some(c) = evaluate_rvalue_to_const(rvalue, &out, ctx, body.locals[place.local].ty)
                    {
                        out.insert(place.local, Some(c));
                    }
                }
            }
            // A defined local that wasn't re-inserted above is non-constant:
            // pin it to `None` so the merge sees it as wild.
            for d in &defined {
                out.entry(*d).or_insert(None);
            }

            let changed_this = match &in_maps[bb_idx] {
                None => true,
                Some(old) => !maps_equal(old, &out),
            };
            if changed_this {
                in_maps[bb_idx] = Some(out);
                changed = true;
            }
        }
    }

    for bb_idx in 0..num_blocks {
        if let Some(map) = &in_maps[bb_idx] {
            let block = &mut body.basic_blocks[BasicBlockIdx::from_raw(bb_idx as u32)];
            for stmt in &mut block.statements {
                if let StatementKind::Assign(_place, rvalue) = &mut stmt.kind {
                    replace_in_rvalue(rvalue, map);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::primitives::IntTy;
    use glyim_test::with_fresh_ty_ctx;
    use std::collections::HashMap;

    #[test]
    fn folds_all_constant_tuple_to_aggregate_const() {
        // Plan §15.3: an `Aggregate` rvalue whose every operand is a constant
        // must fold to a single `MirConstKind::Aggregate`, carrying the tuple
        // type and the element constants in order.
        let (ctx, int_ty) = with_fresh_ty_ctx(|c| c.mk_ty(glyim_type::TyKind::Int(IntTy::I32)));
        let rv = Rvalue::Aggregate(
            AggregateKind::Tuple,
            vec![
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: int_ty,
                    span: Span::DUMMY,
                }),
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(2),
                    ty: int_ty,
                    span: Span::DUMMY,
                }),
            ],
        );
        let result = evaluate_rvalue_to_const(&rv, &HashMap::new(), &ctx, int_ty);
        let c = result.expect("tuple of constants must fold to an aggregate const");
        assert_eq!(
            ctx.ty_kind(c.ty),
            ctx.ty_kind(int_ty),
            "folded const must carry the passed type"
        );
        match &c.kind {
            MirConstKind::Aggregate(elems) => {
                assert_eq!(elems.len(), 2, "must preserve element count");
                assert!(matches!(elems[0].kind, MirConstKind::Int(1)));
                assert!(matches!(elems[1].kind, MirConstKind::Int(2)));
            }
            other => panic!("expected Aggregate, got {:?}", other),
        }
    }

    #[test]
    fn non_constant_operand_blocks_fold() {
        // A tuple with a non-constant operand must NOT fold to an aggregate
        // const (the inner constant-prop map has no entry for the operand).
        let (ctx, int_ty) = with_fresh_ty_ctx(|c| c.mk_ty(glyim_type::TyKind::Int(IntTy::I32)));
        let rv = Rvalue::Aggregate(
            AggregateKind::Tuple,
            vec![
                Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: int_ty,
                    span: Span::DUMMY,
                }),
                // A place that is not in the constant map → blocks folding.
                Operand::Copy(Place::new(LocalIdx::from_raw(0))),
            ],
        );
        let result = evaluate_rvalue_to_const(&rv, &HashMap::new(), &ctx, int_ty);
        assert!(
            result.is_none(),
            "aggregate with a non-constant operand must not fold"
        );
    }
}
