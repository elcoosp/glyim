use crate::codegen::ctx::FunctionContext;
use crate::codegen::expr::codegen_expr;
use crate::codegen::stmt::codegen_block;
use crate::Codegen;
use glyim_hir::{HirExpr, HirPattern};
use inkwell::values::{BasicValue, IntValue};
use inkwell::{AddressSpace, IntPredicate};

pub(crate) fn codegen_while<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    if let HirExpr::While {
        condition, body, ..
    } = expr
    {
        let cond_bb = cg.context.append_basic_block(fctx.fn_value, "while.cond");
        let body_bb = cg.context.append_basic_block(fctx.fn_value, "while.body");
        let end_bb = cg.context.append_basic_block(fctx.fn_value, "while.end");

        cg.builder.build_unconditional_branch(cond_bb).ok()?;

        cg.builder.position_at_end(cond_bb);
        let cond_val = codegen_expr(cg, condition, fctx)?;
        let cond_bool = cg
            .builder
            .build_int_compare(
                IntPredicate::NE,
                cond_val,
                cg.i64_type.const_int(0, false),
                "while_cond",
            )
            .ok()?;
        cg.builder
            .build_conditional_branch(cond_bool, body_bb, end_bb)
            .ok()?;

        cg.builder.position_at_end(body_bb);
        codegen_block(cg, body, fctx)?;
        if cg
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_none()
        {
            cg.builder.build_unconditional_branch(cond_bb).ok();
        }

        cg.builder.position_at_end(end_bb);
        Some(cg.i64_type.const_int(0, false))
    } else {
        None
    }
}

pub(crate) fn codegen_if<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    if let HirExpr::If {
        condition,
        then_branch,
        else_branch,
        ..
    } = expr
    {
        let cond_val = codegen_expr(cg, condition, fctx)?;
        let cond_bool = cg
            .builder
            .build_int_compare(
                IntPredicate::NE,
                cond_val,
                cg.i64_type.const_int(0, false),
                "if_cond",
            )
            .ok()?;
        let then_bb = cg.context.append_basic_block(fctx.fn_value, "then");
        let else_bb = cg.context.append_basic_block(fctx.fn_value, "else");
        let merge_bb = cg.context.append_basic_block(fctx.fn_value, "merge");
        cg.builder
            .build_conditional_branch(cond_bool, then_bb, else_bb)
            .ok()?;
        cg.builder.position_at_end(then_bb);
        let then_val = codegen_block(cg, then_branch, fctx)?;
        cg.builder.build_unconditional_branch(merge_bb).ok()?;
        let then_bb_final = cg.builder.get_insert_block().unwrap();
        cg.builder.position_at_end(else_bb);
        let else_val = match else_branch {
            Some(e) => codegen_block(cg, e, fctx)?,
            None => cg.i64_type.const_int(0, false),
        };
        cg.builder.build_unconditional_branch(merge_bb).ok()?;
        let else_bb_final = cg.builder.get_insert_block().unwrap();
        cg.builder.position_at_end(merge_bb);
        let phi = cg.builder.build_phi(cg.i64_type, "if_result").ok()?;
        phi.add_incoming(&[
            (&then_val as &dyn BasicValue, then_bb_final),
            (&else_val as &dyn BasicValue, else_bb_final),
        ]);
        Some(phi.as_basic_value().into_int_value())
    } else {
        None
    }
}

pub(crate) fn codegen_match<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    if let HirExpr::Match {
        scrutinee, arms, ..
    } = expr
    {
        let scrutinee_val = codegen_expr(cg, scrutinee, fctx)?;
        // If we have exactly two arms: OptionSome/ResultOk and OptionNone/ResultErr,
        // perform a proper tagged dispatch.
        if arms.len() == 2 {
            let (pat0, _guard0, body0) = &arms[0];
            let (_pat1, _guard1, body1) = &arms[1];
            let is_some_like = matches!(pat0, HirPattern::OptionSome(_) | HirPattern::ResultOk(_));

            if is_some_like {
                let enum_ptr = cg.builder.build_int_to_ptr(
                    scrutinee_val,
                    cg.context.ptr_type(AddressSpace::from(0u16)),
                    "enum_ptr",
                ).ok()?;
                let st = cg.context.struct_type(
                    &[cg.i32_type.into(), cg.i64_type.into()],
                    false,
                );
                // Load and test tag
                let tag_ptr = cg.builder.build_struct_gep(st, enum_ptr, 0, "tag_ptr").ok()?;
                let tag_val = cg.builder.build_load(cg.i32_type, tag_ptr, "tag_val").ok()?.into_int_value();
                let is_some = cg.builder.build_int_compare(IntPredicate::EQ, tag_val, cg.i32_type.const_int(0, false), "is_some").ok()?;
                let some_bb = cg.context.append_basic_block(fctx.fn_value, "some");
                let none_bb = cg.context.append_basic_block(fctx.fn_value, "none");
                let merge_bb = cg.context.append_basic_block(fctx.fn_value, "match_merge");
                cg.builder.build_conditional_branch(is_some, some_bb, none_bb).ok()?;

                // Some branch
                cg.builder.position_at_end(some_bb);
                if let HirPattern::OptionSome(inner) | HirPattern::ResultOk(inner) = pat0 {
                    if let HirPattern::Var(name) = inner.as_ref() {
                        let payload_ptr = cg.builder.build_struct_gep(st, enum_ptr, 1, "payload_ptr").ok()?;
                        let payload_val = cg.builder.build_load(cg.i64_type, payload_ptr, "payload_val").ok()?.into_int_value();
                        let alloca = cg.builder.build_alloca(cg.i64_type, cg.interner.resolve(*name)).ok()?;
                        cg.builder.build_store(alloca, payload_val).ok()?;
                        fctx.vars.insert(*name, alloca);
                    }
                }
                let some_val = codegen_expr(cg, body0, fctx)?;
                let some_end = cg.builder.get_insert_block().unwrap();
                if cg.builder.get_insert_block().and_then(|b| b.get_terminator()).is_none() {
                    cg.builder.build_unconditional_branch(merge_bb).ok()?;
                }

                // None branch
                cg.builder.position_at_end(none_bb);
                let none_val = codegen_expr(cg, body1, fctx)?;
                let none_end = cg.builder.get_insert_block().unwrap();
                if cg.builder.get_insert_block().and_then(|b| b.get_terminator()).is_none() {
                    cg.builder.build_unconditional_branch(merge_bb).ok()?;
                }

                // Merge
                cg.builder.position_at_end(merge_bb);
                let phi = cg.builder.build_phi(cg.i64_type, "match_result").ok()?;
                phi.add_incoming(&[(&some_val as &dyn BasicValue, some_end), (&none_val as &dyn BasicValue, none_end)]);
                return Some(phi.as_basic_value().into_int_value());
            }
        }
        // Fallback for single-arm or other matches
        if let Some((_pattern, _, body)) = arms.first() {
            codegen_expr(cg, body, fctx)
        } else {
            Some(cg.i64_type.const_int(0, false))
        }
    } else {
        None
    }
}
