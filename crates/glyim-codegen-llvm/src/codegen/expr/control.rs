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
    if let HirExpr::While { condition, body, .. } = expr {
        let cond_bb = cg.context.append_basic_block(fctx.fn_value, "while.cond");
        let body_bb = cg.context.append_basic_block(fctx.fn_value, "while.body");
        let end_bb = cg.context.append_basic_block(fctx.fn_value, "while.end");

        // Jump to condition check
        cg.builder.build_unconditional_branch(cond_bb).ok()?;

        // Condition block
        cg.builder.position_at_end(cond_bb);
        let cond_val = codegen_expr(cg, condition, fctx)?;
        let cond_bool = cg.builder
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

        // Body block
        cg.builder.position_at_end(body_bb);
        codegen_block(cg, body, fctx)?;
        cg.builder.build_unconditional_branch(cond_bb).ok()?;

        // End block
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
        if let Some((pattern, _, body)) = arms.first() {
            match pattern {
                HirPattern::OptionSome(inner) | HirPattern::ResultOk(inner) => {
                    if let HirPattern::Var(name) = inner.as_ref() {
                        let enum_ptr = cg
                            .builder
                            .build_int_to_ptr(
                                scrutinee_val,
                                cg.context.ptr_type(AddressSpace::from(0u16)),
                                "enum_ptr",
                            )
                            .ok()?;
                        let enum_name = if matches!(pattern, HirPattern::OptionSome(_)) {
                            cg.option_sym
                        } else {
                            cg.result_sym
                        };
                        if let Some(st) = cg.enum_struct_types.borrow().get(&enum_name).copied() {
                            let payload_ptr = cg
                                .builder
                                .build_struct_gep(st, enum_ptr, 1, "payload_ptr")
                                .ok()?;
                            let arg_ptr = cg
                                .builder
                                .build_bit_cast(
                                    payload_ptr,
                                    cg.context.ptr_type(AddressSpace::from(0u16)),
                                    "arg_ptr",
                                )
                                .ok()?
                                .into_pointer_value();
                            let payload_val = cg
                                .builder
                                .build_load(cg.i64_type, arg_ptr, "payload_val")
                                .ok()?
                                .into_int_value();
                            let alloca = cg
                                .builder
                                .build_alloca(cg.i64_type, cg.interner.resolve(*name))
                                .ok()?;
                            cg.builder.build_store(alloca, payload_val).ok()?;
                            fctx.vars.insert(*name, alloca);
                        }
                    }
                }
                _ => {}
            }
            codegen_expr(cg, body, fctx)
        } else {
            Some(cg.i64_type.const_int(0, false))
        }
    } else {
        None
    }
}
