use crate::Codegen;
use crate::codegen::ctx::FunctionContext;
use glyim_hir::{HirExpr, HirStmt, HirType};
use inkwell::AddressSpace;
use inkwell::types::BasicTypeEnum;
use inkwell::values::IntValue;

pub(crate) fn codegen_block<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    match expr {
        HirExpr::Block { stmts, .. } => {
            let mut last = Some(cg.i64_type.const_int(0, false));
            for stmt in stmts {
                if let Some(v) = codegen_stmt(cg, stmt, fctx) {
                    last = Some(v);
                }
            }
            last
        }
        other => super::expr::codegen_expr(cg, other, fctx),
    }
}

pub(crate) fn codegen_stmt<'ctx>(
    cg: &Codegen<'ctx>,
    stmt: &HirStmt,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    eprintln!("STMT: {:?}", stmt);
    let span = match stmt {
        HirStmt::Let { span, .. } => *span,
        HirStmt::LetPat { span, .. } => *span,
        HirStmt::Assign { span, .. } => *span,
        HirStmt::AssignDeref { span, .. } => *span,
        HirStmt::AssignField { span, .. } => *span,
        HirStmt::Expr(e) => e.get_span(),
    };
    cg.set_debug_location_for_span(span);

    match stmt {
        HirStmt::Let {
            name,
            mutable: _,
            value,
            span,
        } => {
            let val = super::expr::codegen_expr(cg, value, fctx)?;
            eprintln!("LET {} = {}", cg.interner.resolve(*name), val);
            let alloca = cg
                .builder
                .build_alloca(cg.i64_type, cg.interner.resolve(*name))
                .ok()?;
            cg.builder.build_store(alloca, val).ok()?;

            if let (Some(di), Some(src), Some(sp)) =
                (&cg.debug_info, &cg.source_str, &cg.current_subprogram)
            {
                let line = crate::debug::DebugInfoGen::byte_offset_to_line(src, span.start);
                let resolved_name = cg.interner.resolve(*name);
                if let Ok(var) = di.create_local_variable(resolved_name, *sp, line)
                    && let Ok(loc) = di.create_location(*sp, line, 0)
                {
                    let _ = di.insert_declare(&cg.builder, &cg.module, var, alloca, loc);
                }
            }

            fctx.vars.insert(*name, alloca);
            None
        }
        HirStmt::LetPat {
            pattern,
            value,
            span,
            ..
        } => {
            let val = super::expr::codegen_expr(cg, value, fctx)?;
            if let glyim_hir::HirPattern::Var(name) = pattern {
                let alloca = cg
                    .builder
                    .build_alloca(cg.i64_type, cg.interner.resolve(*name))
                    .ok()?;
                cg.builder.build_store(alloca, val).ok()?;
                if let (Some(di), Some(src), Some(sp)) =
                    (&cg.debug_info, &cg.source_str, &cg.current_subprogram)
                {
                    let line = crate::debug::DebugInfoGen::byte_offset_to_line(src, span.start);
                    let resolved_name = cg.interner.resolve(*name);
                    if let Ok(var) = di.create_local_variable(resolved_name, *sp, line)
                        && let Ok(loc) = di.create_location(*sp, line, 0)
                    {
                        let _ = di.insert_declare(&cg.builder, &cg.module, var, alloca, loc);
                    }
                }
                fctx.vars.insert(*name, alloca);
                None
            } else {
                codegen_pattern_bind(cg, pattern, val, fctx);
                None
            }
        }
        HirStmt::Assign { target, value, .. } => {
            let new_val = super::expr::codegen_expr(cg, value, fctx)?;
            if let Some(ptr) = fctx.vars.get(target).copied() {
                cg.builder.build_store(ptr, new_val).ok()?;
            }
            Some(new_val)
        }
        HirStmt::AssignDeref { target, value, .. } => {
            // target is a Deref expression; extract the pointer operand from it
            let pointer_expr = if let HirExpr::Deref { expr, .. } = target.as_ref() {
                expr.as_ref()
            } else {
                target
            };
            let ptr_val = super::expr::codegen_expr(cg, pointer_expr, fctx)?;
            let new_val = super::expr::codegen_expr(cg, value, fctx)?;
            // Determine the pointed type from the target's type
            let target_id = target.get_id();
            let target_ty = cg.expr_types.get(target_id.as_usize()).cloned();
            let pointed_ty = match target_ty {
                Some(HirType::RawPtr(inner)) => *inner,
                other => other.unwrap_or(HirType::Int),
            };
            // If the pointed type is a struct, deep copy from the value pointer
            if let Some(st) = cg.resolve_struct_type(&pointed_ty) {
                let val_ptr = cg
                    .builder
                    .build_int_to_ptr(
                        new_val,
                        cg.context.ptr_type(AddressSpace::from(0u16)),
                        "val_ptr",
                    )
                    .ok()?;
                let loaded = cg.builder.build_load(st, val_ptr, "struct_val").ok()?;
                let target_typed = cg
                    .builder
                    .build_int_to_ptr(
                        ptr_val,
                        cg.context.ptr_type(AddressSpace::from(0u16)),
                        "target_ptr",
                    )
                    .ok()?;
                cg.builder.build_store(target_typed, loaded).ok()?;
                return Some(new_val);
            }
            // Non-struct: plain i64 store
            let ptr = cg
                .builder
                .build_int_to_ptr(
                    ptr_val,
                    cg.context.ptr_type(AddressSpace::from(0u16)),
                    "store_ptr",
                )
                .ok()?;
            cg.builder.build_store(ptr, new_val).ok()?;
            Some(new_val)
        }
        HirStmt::AssignField {
            object,
            field,
            value,
            ..
        } => {
            let obj_val = super::expr::codegen_expr(cg, object, fctx)?;
            let obj_ptr = cg
                .builder
                .build_int_to_ptr(
                    obj_val,
                    cg.context.ptr_type(AddressSpace::from(0u16)),
                    "obj_ptr",
                )
                .ok()?;
            let index_map = cg.struct_field_indices.borrow();
            let field_idx = index_map
                .iter()
                .filter(|((_, f), _)| f == field)
                .map(|(_, &idx)| idx)
                .next()
                .unwrap_or(0);
            drop(index_map);
            let obj_id = object.get_id();
            let struct_type_opt = match &cg.expr_types.get(obj_id.as_usize()) {
                Some(HirType::Named(name)) | Some(HirType::Generic(name, _)) => {
                    cg.struct_types.borrow().get(name).copied()
                }
                _ => None,
            };
            let struct_type_opt = struct_type_opt.or_else(|| {
                let struct_types = cg.struct_types.borrow();
                let idx_map = cg.struct_field_indices.borrow();
                struct_types.iter().find_map(|(sym, st)| {
                    if idx_map.contains_key(&(*sym, *field)) {
                        Some(*st)
                    } else {
                        None
                    }
                })
            });
            let field_ptr = if let Some(st) = struct_type_opt {
                let indices = &[
                    cg.i32_type.const_int(0, false),
                    cg.i32_type.const_int(field_idx as u64, false),
                ];
                unsafe {
                    cg.builder
                        .build_gep(st, obj_ptr, indices, "assign_field")
                        .ok()?
                }
            } else {
                return Some(cg.i64_type.const_int(0, false));
            };
            let new_val = super::expr::codegen_expr(cg, value, fctx)?;
            cg.builder.build_store(field_ptr, new_val).ok()?;
            Some(new_val)
        }
        HirStmt::Expr(e) => super::expr::codegen_expr(cg, e, fctx),
    }
}

fn codegen_pattern_bind<'ctx>(
    cg: &Codegen<'ctx>,
    pattern: &glyim_hir::HirPattern,
    val: inkwell::values::IntValue<'ctx>,
    fctx: &mut FunctionContext<'ctx>,
) {
    match pattern {
        glyim_hir::HirPattern::Var(sym) => {
            let alloca = cg
                .builder
                .build_alloca(cg.i64_type, cg.interner.resolve(*sym))
                .ok();
            if let Some(a) = alloca {
                cg.builder.build_store(a, val).ok();
                fctx.vars.insert(*sym, a);
            }
        }
        glyim_hir::HirPattern::Tuple { elements, .. } => {
            let ptr = cg
                .builder
                .build_int_to_ptr(
                    val,
                    cg.context.ptr_type(AddressSpace::from(0u16)),
                    "tuple_ptr",
                )
                .ok();
            if let Some(ptr) = ptr {
                for (i, elem_pat) in elements.iter().enumerate() {
                    let zero = cg.i32_type.const_int(0, false);
                    let idx = cg.i32_type.const_int(i as u64, false);
                    let field_types = vec![BasicTypeEnum::IntType(cg.i64_type); elements.len()];
                    let struct_ty = cg.context.struct_type(&field_types, false);
                    let field_ptr = unsafe {
                        cg.builder
                            .build_gep(struct_ty, ptr, &[zero, idx], "field")
                            .ok()
                    };
                    if let Some(fp) = field_ptr {
                        let field_val = cg
                            .builder
                            .build_load(cg.i64_type, fp, "elem")
                            .ok()
                            .and_then(|v| v.into_int_value().into())
                            .unwrap_or(cg.i64_type.const_int(0, false));
                        codegen_pattern_bind(cg, elem_pat, field_val, fctx);
                    }
                }
            }
        }
        glyim_hir::HirPattern::Struct { name, bindings, .. } => {
            let ptr = cg
                .builder
                .build_int_to_ptr(
                    val,
                    cg.context.ptr_type(AddressSpace::from(0u16)),
                    "struct_ptr",
                )
                .ok();
            if let Some(ptr) = ptr
                && let Some(st) = cg.struct_types.borrow().get(name).copied()
            {
                for (field_sym, field_pat) in bindings {
                    if let Some(field_idx) = cg
                        .struct_field_indices
                        .borrow()
                        .get(&(*name, *field_sym))
                        .copied()
                    {
                        let zero = cg.i32_type.const_int(0, false);
                        let idx = cg.i32_type.const_int(field_idx as u64, false);
                        let field_ptr =
                            unsafe { cg.builder.build_gep(st, ptr, &[zero, idx], "field").ok() };
                        if let Some(fp) = field_ptr {
                            let field_val = cg
                                .builder
                                .build_load(cg.i64_type, fp, "field_val")
                                .ok()
                                .and_then(|v| v.into_int_value().into())
                                .unwrap_or(cg.i64_type.const_int(0, false));
                            codegen_pattern_bind(cg, field_pat, field_val, fctx);
                        }
                    }
                }
            }
        }
        glyim_hir::HirPattern::Wild | glyim_hir::HirPattern::Unit => {}
        _ => {}
    }
}
