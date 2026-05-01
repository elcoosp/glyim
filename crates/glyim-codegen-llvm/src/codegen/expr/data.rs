use crate::codegen::ctx::FunctionContext;
use crate::codegen::expr::codegen_expr;
use crate::Codegen;
use glyim_hir::{HirExpr, HirType};
use inkwell::values::IntValue;
use inkwell::{types::BasicTypeEnum, AddressSpace};

pub(crate) fn codegen_struct_lit<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    if let HirExpr::StructLit {
        struct_name,
        fields,
        ..
    } = expr
    {
        let struct_type_opt = cg.struct_types.borrow().get(struct_name).copied();
        match struct_type_opt {
            Some(st) => {
                let size = st.size_of()?;
                let alloc_fn = cg
                    .module
                    .get_function("glyim_alloc")
                    .or_else(|| cg.module.get_function("malloc"))?;
                let call_result = cg
                    .builder
                    .build_call(alloc_fn, &[size.into()], "struct_alloc")
                    .ok()?
                    .try_as_basic_value();
                let ptr = match call_result {
                    inkwell::values::ValueKind::Basic(basic_val) => basic_val.into_pointer_value(),
                    _ => return Some(cg.i64_type.const_int(0, false)),
                };
                for (i, (_fn, fe)) in fields.iter().enumerate() {
                    let fv = codegen_expr(cg, fe, fctx)?;
                    let indices = &[
                        cg.i32_type.const_int(0, false),
                        cg.i32_type.const_int(i as u64, false),
                    ];
                    let fp = unsafe { cg.builder.build_gep(st, ptr, indices, "field").ok()? };
                    cg.builder.build_store(fp, fv).ok()?;
                }
                cg.builder
                    .build_ptr_to_int(ptr, cg.i64_type, "struct_ptr")
                    .ok()
            }
            None => Some(cg.i64_type.const_int(0, false)),
        }
    } else {
        None
    }
}

pub(crate) fn codegen_enum_variant<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    if let HirExpr::EnumVariant {
        enum_name,
        variant_name,
        args,
        ..
    } = expr
    {
        let enum_struct_type = cg.enum_struct_types.borrow().get(enum_name).copied();
        let tag_map = cg.enum_variant_tags.borrow();
        let tag = tag_map
            .get(&(*enum_name, *variant_name))
            .copied()
            .unwrap_or(0);
        drop(tag_map);
        if let Some(st) = enum_struct_type {
            let alloca = cg.builder.build_alloca(st, "enum_tmp").unwrap();
            let tag_val = cg.i32_type.const_int(tag as u64, false);
            let tag_ptr = cg
                .builder
                .build_struct_gep(st, alloca, 0, "tag_ptr")
                .unwrap();
            cg.builder.build_store(tag_ptr, tag_val).unwrap();
            if !args.is_empty() {
                let payload_ptr = cg
                    .builder
                    .build_struct_gep(st, alloca, 1, "payload_ptr")
                    .unwrap();
                let arg_ptr = cg
                    .builder
                    .build_bit_cast(
                        payload_ptr,
                        cg.context.ptr_type(AddressSpace::from(0u16)),
                        "arg_ptr",
                    )
                    .unwrap()
                    .into_pointer_value();
                let arg_val =
                    codegen_expr(cg, &args[0], fctx).unwrap_or(cg.i64_type.const_int(0, false));
                cg.builder.build_store(arg_ptr, arg_val).unwrap();
            }
            let ptr_i64 = cg
                .builder
                .build_ptr_to_int(alloca, cg.i64_type, "enum_ptr")
                .unwrap();
            Some(ptr_i64)
        } else {
            args.first()
                .and_then(|a| codegen_expr(cg, a, fctx))
                .or(Some(cg.i64_type.const_int(0, false)))
        }
    } else {
        None
    }
}

pub(crate) fn codegen_field_access<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    if let HirExpr::FieldAccess { object, field, .. } = expr {
        let obj_val = codegen_expr(cg, object, fctx)?;
        let obj_id = object.get_id();
        let obj_ty = cg.expr_types.get(obj_id.as_usize()).cloned();
        if let Some(HirType::Tuple(elems)) = obj_ty {
            let field_name = cg.interner.resolve(*field);
            if let Some(idx) = field_name
                .strip_prefix('_')
                .and_then(|s| s.parse::<usize>().ok())
            {
                if idx < elems.len() {
                    let field_types = vec![BasicTypeEnum::IntType(cg.i64_type); elems.len()];
                    let struct_ty = cg.context.struct_type(&field_types, false);
                    let alloca = cg
                        .builder
                        .build_int_to_ptr(
                            obj_val,
                            cg.context.ptr_type(AddressSpace::from(0u16)),
                            "tuple_ptr",
                        )
                        .ok()?;
                    let field_ptr = cg
                        .builder
                        .build_struct_gep(struct_ty, alloca, idx as u32, "field")
                        .ok()?;
                    return cg
                        .builder
                        .build_load(cg.i64_type, field_ptr, "elem_val")
                        .ok()
                        .map(|v| v.into_int_value());
                }
            }
        }
        let obj_ptr = cg
            .builder
            .build_int_to_ptr(
                obj_val,
                cg.context.ptr_type(AddressSpace::from(0u16)),
                "to_ptr",
            )
            .ok()?;
        let index_map = cg.struct_field_indices.borrow();
        let field_idx = index_map
            .iter()
            .find(|((_, f), _)| f == field)
            .map(|(_, &idx)| idx)
            .or_else(|| {
                index_map
                    .iter()
                    .filter(|((_, f), _)| f == field)
                    .map(|(_, &idx)| idx)
                    .next()
            })
            .unwrap_or(0);
        drop(index_map);
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
                    Some(st.clone())
                } else {
                    None
                }
            })
        });
        let indices = &[
            cg.i32_type.const_int(0, false),
            cg.i32_type.const_int(field_idx as u64, false),
        ];
        let field_ptr = if let Some(st_type) = struct_type_opt {
            unsafe {
                cg.builder
                    .build_gep(st_type, obj_ptr, indices, "field_access")
                    .ok()?
            }
        } else {
            return Some(cg.i64_type.const_int(0, false));
        };
        let field_val_raw = cg.builder
            .build_load(cg.i64_type, field_ptr, "field_val")
            .ok()?;
        let field_val_int = field_val_raw.into_int_value();
        eprintln!("FIELD_ACCESS: field={:?} idx={} loaded_val (from IR) = ? we can't print value at runtime, but we can print the IR instruction",
                cg.interner.resolve(*field), field_idx);
        Some(field_val_int)
    } else {
        None
    }
}

pub(crate) fn codegen_tuple_lit<'ctx>(
    cg: &Codegen<'ctx>,
    expr: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    if let HirExpr::TupleLit { elements, .. } = expr {
        let elems: Vec<IntValue> = elements
            .iter()
            .filter_map(|e| codegen_expr(cg, e, fctx))
            .collect();
        if elems.is_empty() {
            return Some(cg.i64_type.const_int(0, false));
        }
        let field_types = vec![BasicTypeEnum::IntType(cg.i64_type); elems.len()];
        let struct_ty = cg.context.struct_type(&field_types, false);
        let alloca = cg.builder.build_alloca(struct_ty, "tuple").ok()?;
        for (i, val) in elems.iter().enumerate() {
            let indices = &[
                cg.i32_type.const_int(0, false),
                cg.i32_type.const_int(i as u64, false),
            ];
            let ptr = unsafe {
                cg.builder
                    .build_gep(struct_ty, alloca, indices, "field")
                    .ok()?
            };
            cg.builder.build_store(ptr, *val).ok()?;
        }
        cg.builder
            .build_ptr_to_int(alloca, cg.i64_type, "tuple_ptr")
            .ok()
    } else {
        None
    }
}
