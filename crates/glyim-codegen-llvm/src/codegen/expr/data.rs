use crate::codegen::ctx::FunctionContext;
use crate::codegen::expr::codegen_expr;
use crate::Codegen;
use glyim_hir::{HirExpr, HirType};
use inkwell::values::IntValue;
use inkwell::AddressSpace;

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
                    .get_function("__glyim_alloc")
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
            None => {
                let fallback_size = cg.i64_type.const_int((fields.len() as u64) * 8, false);
                let alloc_fn = cg
                    .module
                    .get_function("__glyim_alloc")
                    .or_else(|| cg.module.get_function("malloc"))?;
                let call_result = cg
                    .builder
                    .build_call(alloc_fn, &[fallback_size.into()], "struct_alloc")
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
                    let i8_ptr = unsafe {
                        cg.builder
                            .build_gep(cg.context.i8_type(), ptr, indices, "field")
                            .ok()?
                    };
                    cg.builder.build_store(i8_ptr, fv).ok()?;
                }
                cg.builder
                    .build_ptr_to_int(ptr, cg.i64_type, "struct_ptr")
                    .ok()
            }
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
        let tag_map = cg.enum_variant_tags.borrow();
        let tag = tag_map
            .get(&(*enum_name, *variant_name))
            .copied()
            .unwrap_or(0);
        drop(tag_map);

        if args.is_empty() {
            // None / unit variant – heap-allocate { i32 }
            let st = cg.context.struct_type(&[cg.i32_type.into()], false);
            let size = st.size_of().unwrap_or(cg.i64_type.const_int(4, false));
            let alloc_fn = cg
                .module
                .get_function("__glyim_alloc")
                .or_else(|| cg.module.get_function("malloc"))?;
            let call_result = cg
                .builder
                .build_call(alloc_fn, &[size.into()], "enum_alloc")
                .ok()?
                .try_as_basic_value();
            let ptr = match call_result {
                inkwell::values::ValueKind::Basic(basic_val) => basic_val.into_pointer_value(),
                _ => return Some(cg.i64_type.const_int(0, false)),
            };
            let tag_ptr = cg.builder.build_struct_gep(st, ptr, 0, "tag_ptr").unwrap();
            cg.builder
                .build_store(tag_ptr, cg.i32_type.const_int(tag as u64, false))
                .unwrap();
            return cg
                .builder
                .build_ptr_to_int(ptr, cg.i64_type, "enum_ptr")
                .ok();
        }

        let arg_val = codegen_expr(cg, &args[0], fctx).unwrap_or(cg.i64_type.const_int(0, false));

        // Uniform representation: { i32, i64 } – tag + payload pointer/value
        let st = cg
            .context
            .struct_type(&[cg.i32_type.into(), cg.i64_type.into()], false);
        let size = st.size_of().unwrap_or(cg.i64_type.const_int(8, false));
        let alloc_fn = cg
            .module
            .get_function("__glyim_alloc")
            .or_else(|| cg.module.get_function("malloc"))?;
        let call_result = cg
            .builder
            .build_call(alloc_fn, &[size.into()], "enum_alloc")
            .ok()?
            .try_as_basic_value();
        let ptr = match call_result {
            inkwell::values::ValueKind::Basic(basic_val) => basic_val.into_pointer_value(),
            _ => return Some(cg.i64_type.const_int(0, false)),
        };

        let tag_ptr = cg.builder.build_struct_gep(st, ptr, 0, "tag_ptr").unwrap();
        cg.builder
            .build_store(tag_ptr, cg.i32_type.const_int(tag as u64, false))
            .unwrap();

        let payload_ptr = cg
            .builder
            .build_struct_gep(st, ptr, 1, "payload_ptr")
            .unwrap();
        cg.builder.build_store(payload_ptr, arg_val).unwrap();

        cg.builder
            .build_ptr_to_int(ptr, cg.i64_type, "enum_ptr")
            .ok()
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
                    let field_types =
                        vec![inkwell::types::BasicTypeEnum::IntType(cg.i64_type); elems.len()];
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
                    Some(*st)
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
        let field_val_raw = cg
            .builder
            .build_load(cg.i64_type, field_ptr, "field_val")
            .ok()?;
        let field_val_int = field_val_raw.into_int_value();
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
        let field_types = vec![inkwell::types::BasicTypeEnum::IntType(cg.i64_type); elems.len()];
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
