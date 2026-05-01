mod control;
mod data;
mod float_ops;

use crate::codegen::ctx::FunctionContext;
use crate::Codegen;
use glyim_hir::{HirBinOp, HirExpr, HirType, HirUnOp};
use inkwell::types::BasicType;
use inkwell::values::IntValue;

pub(crate) fn codegen_expr<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    cg.set_debug_location_for_span(expr.get_span());
    match expr {
        HirExpr::IntLit { value: n, .. } => Some(cg.i64_type.const_int(*n as u64, true)),
        HirExpr::Ident { name: sym, .. } => {
            let ptr = fctx.vars.get(sym)?;
            cg.builder
                .build_load(cg.i64_type, *ptr, cg.interner.resolve(*sym))
                .ok()
                .map(|v| v.into_int_value())
        }
        HirExpr::Binary {
            id: _, op, lhs, rhs, ..
        } => {
            let lhs_id = lhs.get_id();
            let is_float = cg
                .expr_types
                .get(lhs_id.as_usize())
                .map(|t| matches!(t, HirType::Float))
                .unwrap_or(false);
            if is_float {
                let l_i64 = codegen_expr(cg, lhs, fctx)?;
                let r_i64 = codegen_expr(cg, rhs, fctx)?;
                let ptr_ty = cg.context.ptr_type(inkwell::AddressSpace::from(0u16));
                let l_ptr = cg.builder.build_int_to_ptr(l_i64, ptr_ty, "fl_ptr").ok()?;
                let r_ptr = cg.builder.build_int_to_ptr(r_i64, ptr_ty, "fr_ptr").ok()?;
                let l_f = cg
                    .builder
                    .build_load(cg.f64_type, l_ptr, "fl_val")
                    .ok()?
                    .into_float_value();
                let r_f = cg
                    .builder
                    .build_load(cg.f64_type, r_ptr, "fr_val")
                    .ok()?
                    .into_float_value();
                let is_cmp = matches!(
                    op,
                    HirBinOp::Eq
                        | HirBinOp::Neq
                        | HirBinOp::Lt
                        | HirBinOp::Gt
                        | HirBinOp::Lte
                        | HirBinOp::Gte
                );
                if is_cmp {
                    super::ops::codegen_float_cmp(cg, op, l_f, r_f)
                } else {
                    let result_f = super::ops::codegen_float_binop(cg, op, l_f, r_f)?;
                    let alloca = cg.builder.build_alloca(cg.f64_type, "fres_tmp").ok()?;
                    cg.builder.build_store(alloca, result_f).ok()?;
                    cg.builder
                        .build_ptr_to_int(alloca, cg.i64_type, "fres_i64")
                        .ok()
                }
            } else {
                let l = codegen_expr(cg, lhs, fctx)?;
                let r = codegen_expr(cg, rhs, fctx)?;
                super::ops::codegen_binop(cg, op.clone(), l, r)
            }
        }
        HirExpr::Unary { op, operand, .. } => {
            let val = codegen_expr(cg, operand, fctx)?;
            match op {
                HirUnOp::Neg => {
                    let zero = cg.i64_type.const_int(0, false);
                    cg.builder.build_int_sub(zero, val, "neg").ok()
                }
                HirUnOp::Not => cg.builder.build_not(val, "not").ok(),
            }
        }
        HirExpr::BoolLit { value: b, .. } => {
            let i1 = cg
                .context
                .bool_type()
                .const_int(if *b { 1 } else { 0 }, false);
            Some(
                cg.builder
                    .build_int_z_extend(i1, cg.i64_type, "bool_zext")
                    .ok()?,
            )
        }
        HirExpr::UnitLit { .. } => Some(cg.i64_type.const_int(0, false)),
        HirExpr::StrLit { value: s, .. } => super::string::codegen_string_literal(cg, s),
        HirExpr::SizeOf { target_type, .. } => {
            if let Some(llvm_type) = cg.hir_type_to_llvm(target_type) {
                Some(
                    llvm_type
                        .size_of()
                        .unwrap_or_else(|| cg.i64_type.const_int(0, false)),
                )
            } else {
                Some(cg.i64_type.const_int(0, false))
            }
        }
        HirExpr::Println { arg, .. } => super::string::codegen_println(cg, arg, fctx),
        HirExpr::Assert {
            condition, message, ..
        } => super::string::codegen_assert(cg, condition, message, fctx),
        HirExpr::Call { callee, args, .. } => super::string::codegen_call(cg, callee, args, fctx),
        HirExpr::Block { stmts, .. } => {
            let mut last = Some(cg.i64_type.const_int(0, false));
            for stmt in stmts {
                if let Some(v) = super::stmt::codegen_stmt(cg, stmt, fctx) {
                    last = Some(v);
                }
            }
            last
        }
        HirExpr::ForIn { .. } => control::codegen_while(cg, expr, fctx),
        HirExpr::While { .. } => control::codegen_while(cg, expr, fctx),
        HirExpr::If { condition: _, .. } => control::codegen_if(cg, expr, fctx),
        HirExpr::Match { .. } => control::codegen_match(cg, expr, fctx),
        HirExpr::StructLit { .. } => data::codegen_struct_lit(cg, expr, fctx),
        HirExpr::EnumVariant { .. } => data::codegen_enum_variant(cg, expr, fctx),
        HirExpr::FieldAccess { .. } => data::codegen_field_access(cg, expr, fctx),
        HirExpr::TupleLit { .. } => data::codegen_tuple_lit(cg, expr, fctx),
        HirExpr::Return { value, .. } => {
            let ret_val = match value {
                Some(v) => codegen_expr(cg, v, fctx)?,
                None => cg.i64_type.const_int(0, false),
            };
            cg.builder.build_return(Some(&ret_val)).ok()?;
            None
        }
        HirExpr::As {
            expr,
            target_type,
            id: _,
            ..
        } => {
            let src_val = codegen_expr(cg, expr, fctx)?;
            let src_ty = cg
                .expr_types
                .get(expr.get_id().as_usize())
                .cloned()
                .unwrap_or(HirType::Int);

            // Determine the resolved target type (unwrapping Named if needed)
            let resolve_ty = |ty: &HirType| -> HirType {
                match ty {
                    HirType::Named(sym) => {
                        let name = cg.interner.resolve(*sym);
                        match name {
                            "i64" | "Int" => HirType::Int,
                            "f64" | "Float" => HirType::Float,
                            "bool" | "Bool" => HirType::Bool,
                            "Str" | "str" => HirType::Str,
                            _ => ty.clone(),
                        }
                    }
                    _ => ty.clone(),
                }
            };

            let resolved_src = resolve_ty(&src_ty);
            let resolved_tgt = resolve_ty(target_type);

            use HirType::*;
            match (&resolved_src, &resolved_tgt) {
                // Integer to Float
                (Int, Float) => {
                    let fv = cg
                        .builder
                        .build_signed_int_to_float(src_val, cg.f64_type, "sitofp")
                        .ok()?;
                    let alloca = cg.builder.build_alloca(cg.f64_type, "cast_tmp").ok()?;
                    cg.builder.build_store(alloca, fv).ok()?;
                    cg.builder
                        .build_ptr_to_int(alloca, cg.i64_type, "f2i64")
                        .ok()
                }
                // Float to Integer
                (Float, Int) => {
                    let ptr = cg
                        .builder
                        .build_int_to_ptr(
                            src_val,
                            cg.context.ptr_type(inkwell::AddressSpace::from(0u16)),
                            "i2ptr",
                        )
                        .ok()?;
                    let fv = cg
                        .builder
                        .build_load(cg.f64_type, ptr, "load_f64")
                        .ok()?
                        .into_float_value();
                    cg.builder
                        .build_float_to_signed_int(fv, cg.i64_type, "fptosi")
                        .ok()
                }
                // Integer/Float to same type (identity)
                (Int, Int) | (Float, Float) => Some(src_val),
                // Integer to RawPtr
                (_, RawPtr(_)) => {
                    let ptr = cg
                        .builder
                        .build_int_to_ptr(
                            src_val,
                            cg.context.ptr_type(inkwell::AddressSpace::from(0u16)),
                            "inttoptr",
                        )
                        .ok()?;
                    cg.builder
                        .build_ptr_to_int(ptr, cg.i64_type, "ptr2i64")
                        .ok()
                }
                // RawPtr and everything else (identity or bitcast)
                _ => Some(src_val),
            }
        }
        HirExpr::FloatLit { value: f, .. } => {
            let fv = cg.f64_type.const_float(*f);
            let alloca = cg.builder.build_alloca(cg.f64_type, "float_tmp").ok()?;
            cg.builder.build_store(alloca, fv).ok()?;
            Some(
                cg.builder
                    .build_ptr_to_int(alloca, cg.i64_type, "f2i64")
                    .ok()?,
            )
        }
        HirExpr::Deref { expr, id, .. } => {
            let ptr_val = codegen_expr(cg, expr, fctx)?;
            let pointed_ty = cg
                .expr_types
                .get(id.as_usize())
                .cloned()
                .unwrap_or(HirType::Int);
            // Debug assertion: monomorphization should have resolved all generics.
            // If this fires, the pipeline didn't fully replace a generic type.
            debug_assert!(
                !matches!(pointed_ty, HirType::Generic(_, _)),
                "Deref sees unresolved generic type"
            );
            let load_type = cg
                .hir_type_to_llvm(&pointed_ty)
                .unwrap_or(cg.i64_type.into());
            let ptr = cg
                .builder
                .build_int_to_ptr(
                    ptr_val,
                    cg.context.ptr_type(inkwell::AddressSpace::from(0u16)),
                    "deref_ptr",
                )
                .ok()?;
            let loaded = cg.builder.build_load(load_type, ptr, "deref_val").ok()?;
            match loaded {
                inkwell::values::BasicValueEnum::IntValue(iv) => Some(iv),
                inkwell::values::BasicValueEnum::FloatValue(fv) => {
                    let alloca = cg.builder.build_alloca(cg.f64_type, "f_tmp").ok()?;
                    cg.builder.build_store(alloca, fv).ok()?;
                    cg.builder.build_ptr_to_int(alloca, cg.i64_type, "f2i").ok()
                }
                inkwell::values::BasicValueEnum::PointerValue(pv) => {
                    cg.builder.build_ptr_to_int(pv, cg.i64_type, "p2i").ok()
                }
                _ => Some(cg.i64_type.const_int(0, false)),
            }
        }
        HirExpr::MethodCall {
            receiver,
            method_name,
            args,
            ..
        } => {
            let receiver_val = codegen_expr(cg, receiver, fctx)?;
            let receiver_id = receiver.get_id();
            let receiver_ty = cg
                .expr_types
                .get(receiver_id.as_usize())
                .cloned()
                .unwrap_or(HirType::Int);
            let mangled_name = match &receiver_ty {
                HirType::Named(type_name) | HirType::Generic(type_name, _) => format!(
                    "{}_{}",
                    cg.interner.resolve(*type_name),
                    cg.interner.resolve(*method_name)
                ),
                _ => cg.interner.resolve(*method_name).to_string(),
            };
            let maybe_fn = cg.module.get_function(&mangled_name);
            if let Some(fn_val) = maybe_fn {
                let mut call_args: Vec<inkwell::values::BasicMetadataValueEnum> = Vec::new();
                call_args.push(inkwell::values::BasicMetadataValueEnum::IntValue(
                    receiver_val,
                ));
                for a in args {
                    if let Some(v) = codegen_expr(cg, a, fctx) {
                        call_args.push(inkwell::values::BasicMetadataValueEnum::IntValue(v));
                    }
                }
                let result = cg
                    .builder
                    .build_call(fn_val, &call_args, "method_call")
                    .ok()?;
                match result.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(basic_val) => {
                        Some(basic_val.into_int_value())
                    }
                    _ => Some(cg.i64_type.const_int(0, false)),
                }
            } else {
                Some(cg.i64_type.const_int(0, false))
            }
        }
    }
}
