use crate::Codegen;
use crate::codegen::ctx::FunctionContext;
use crate::codegen::expr::codegen_expr;
use glyim_hir::HirExpr;
use glyim_interner::Symbol;
use inkwell::values::{BasicValueEnum, IntValue, StructValue};
use inkwell::{AddressSpace, types::BasicTypeEnum};

pub(crate) fn codegen_string_literal<'ctx>(cg: &Codegen<'ctx>, s: &str) -> Option<IntValue<'ctx>> {
    let bytes = s.trim_matches('"').as_bytes();
    let ty = cg.context.i8_type().array_type(bytes.len() as u32);
    let name = {
        let mut counter = cg.string_counter.borrow_mut();
        let name = format!("str.{}", *counter);
        *counter += 1;
        name
    };
    let global = cg
        .module
        .add_global(ty, Some(AddressSpace::from(0u16)), &name);
    let elems: Vec<_> = bytes
        .iter()
        .map(|b| cg.context.i8_type().const_int(*b as u64, false))
        .collect();
    let arr = unsafe { inkwell::values::ArrayValue::new_const_array(&ty, &elems) };
    global.set_initializer(&arr);
    global.set_constant(true);
    global.set_linkage(inkwell::module::Linkage::Private);
    let zero32 = cg.context.i32_type().const_int(0, false);
    let ptr = unsafe {
        cg.builder
            .build_gep(ty, global.as_pointer_value(), &[zero32, zero32], "str_ptr")
            .ok()?
    };
    cg.builder
        .build_ptr_to_int(ptr, cg.i64_type, "str_as_int")
        .ok()
}

pub(crate) fn extract_str_data_ptr<'ctx>(
    cg: &Codegen<'ctx>,
    str_val: IntValue<'ctx>,
) -> Option<IntValue<'ctx>> {
    // str_val is a pointer (as i64) to a stack-allocated {i8*, i64} struct.
    // Load the first field (data pointer) from that struct.
    let ptr_type = cg.context.ptr_type(AddressSpace::from(0u16));
    let ptr = cg
        .builder
        .build_int_to_ptr(str_val, ptr_type, "str_ptr")
        .ok()?;
    let data_ptr = cg
        .builder
        .build_load(cg.i64_type, ptr, "data_ptr_val")
        .ok()?;
    Some(data_ptr.into_int_value())
}

pub(crate) fn codegen_println<'ctx>(
    cg: &Codegen<'ctx>,
    arg: &HirExpr,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    let val = codegen_expr(cg, arg, fctx)?;
    if matches!(arg, HirExpr::StrLit { .. }) {
        let fat = build_str_fat_ptr(cg, arg)?;
        let ptr = cg.builder.build_extract_value(fat, 0, "str_ptr").ok()?;
        let len = cg.builder.build_extract_value(fat, 1, "str_len").ok()?;
        let shim = cg.module.get_function("__glyim_println_str").unwrap();
        cg.builder
            .build_call(shim, &[ptr.into(), len.into()], "println")
            .ok()?;
    } else {
        let shim = cg.module.get_function("__glyim_println_int").unwrap();
        cg.builder.build_call(shim, &[val.into()], "println").ok()?;
    }
    Some(cg.i64_type.const_int(0, false))
}

pub(crate) fn codegen_assert<'ctx>(
    cg: &Codegen<'ctx>,
    condition: &HirExpr,
    message: &Option<Box<HirExpr>>,
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    let cond = codegen_expr(cg, condition, fctx)?;
    let is_true = cg
        .builder
        .build_int_compare(
            inkwell::IntPredicate::NE,
            cond,
            cg.i64_type.const_int(0, false),
            "assert_cond",
        )
        .ok()?;
    let pass_bb = cg.context.append_basic_block(fctx.fn_value, "assert_pass");
    let fail_bb = cg.context.append_basic_block(fctx.fn_value, "assert_fail");
    cg.builder
        .build_conditional_branch(is_true, pass_bb, fail_bb)
        .ok()?;
    cg.builder.position_at_end(fail_bb);
    let shim = cg.module.get_function("__glyim_assert_fail").unwrap();
    let null_ptr = cg.context.ptr_type(AddressSpace::from(0u16)).const_null();
    let zero = cg.i64_type.const_int(0, false);
    let (p, l) = match message {
        Some(m) if matches!(m.as_ref(), HirExpr::StrLit { .. }) => {
            let fat = build_str_fat_ptr(cg, m.as_ref())?;
            let ptr = cg.builder.build_extract_value(fat, 0, "msg_p").ok()?;
            let len = cg.builder.build_extract_value(fat, 1, "msg_l").ok()?;
            (ptr, len)
        }
        _ => (
            BasicValueEnum::PointerValue(null_ptr),
            BasicValueEnum::IntValue(zero),
        ),
    };
    cg.builder
        .build_call(shim, &[p.into(), l.into()], "assert_fail")
        .ok()?;
    cg.builder.build_unreachable().ok()?;
    cg.builder.position_at_end(pass_bb);
    Some(cg.i64_type.const_int(0, false))
}

pub(crate) fn codegen_call<'ctx>(
    cg: &Codegen<'ctx>,
    callee: &Symbol,
    args: &[HirExpr],
    fctx: &mut FunctionContext<'ctx>,
) -> Option<IntValue<'ctx>> {
    let fn_name = match cg.interner.try_resolve(*callee) {
        Some(name) => name,
        None => {
            // Callee symbol not in interner – fallback to zero.
            return Some(cg.i64_type.const_int(0, false));
        }
    };

    // __ptr_offset built‑in:
    if fn_name == "__ptr_offset" && args.len() == 2 {
        let ptr_val = codegen_expr(cg, &args[0], fctx)?;
        let offset_val = codegen_expr(cg, &args[1], fctx)?;
        let ptr_type = cg.context.ptr_type(inkwell::AddressSpace::from(0u16));
        let base_ptr = cg
            .builder
            .build_int_to_ptr(ptr_val, ptr_type, "ptr_cast")
            .ok()?;
        let gep = unsafe {
            cg.builder
                .build_gep(cg.context.i8_type(), base_ptr, &[offset_val], "ptr_offset")
                .ok()?
        };
        return cg
            .builder
            .build_ptr_to_int(gep, cg.i64_type, "ptr_to_int")
            .ok();
    }

    let mut fn_val = cg.module.get_function(fn_name);
    if fn_val.is_none() {
        // Fallback: try `fn_name__i64` for zero-arg generic calls
        let guess = format!("{}__i64", fn_name);
        fn_val = cg.module.get_function(&guess);
        if fn_val.is_none() {
            // Fallback: try `fn_name__i64_i64` for two-arg generics
            let guess2 = format!("{}__i64_i64", fn_name);
            fn_val = cg.module.get_function(&guess2);
        }
        if fn_val.is_none() {
            // Fallback: search any function starting with `fn_name__`
            let prefix = format!("{}__", fn_name);
            let mut found = None;
            if let Some(first) = cg.module.get_first_function() {
                let mut cur = Some(first);
                while let Some(f) = cur {
                    let name = f.get_name().to_string_lossy();
                    if name.starts_with(&prefix) {
                        found = Some(f);
                        break;
                    }
                    cur = f.get_next_function();
                }
            }
            fn_val = found;
        }
    }
    if let Some(fn_val) = fn_val {
        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> =
            fn_val.get_type().get_param_types().into_iter().collect();
        let call_args: Vec<inkwell::values::BasicMetadataValueEnum> = args
            .iter()
            .filter_map(|a| {
                let val = codegen_expr(cg, a, fctx)?;
                // If Glyim type is Str and parameter expects a pointer, extract the data pointer
                let arg_type = cg.expr_types.get(a.get_id().as_usize()).cloned();
                if arg_type == Some(glyim_hir::types::HirType::Str) {
                    let param_idx = args.iter().position(|x| std::ptr::eq(x, a)).unwrap_or(0);
                    let param_type = param_types.get(param_idx);
                    if param_type.is_some_and(|ty| ty.is_pointer_type()) {
                        return extract_str_data_ptr(cg, val);
                    }
                }
                Some(val)
            })
            .enumerate()
            .map(|(i, int_val)| {
                let param_type = param_types.get(i);
                // Truncate i64 to i32 if extern parameter expects i32
                if let Some(inkwell::types::BasicMetadataTypeEnum::IntType(t)) = param_type
                    && t.get_bit_width() == 32
                    && let Ok(trunc) =
                        cg.builder
                            .build_int_truncate(int_val, cg.i32_type, "i32_trunc")
                {
                    return inkwell::values::BasicMetadataValueEnum::IntValue(trunc);
                }
                if param_type.is_some_and(|ty| ty.is_pointer_type()) {
                    // Convert i64 to pointer for extern functions expecting ptr
                    match cg.builder.build_int_to_ptr(
                        int_val,
                        cg.context.ptr_type(inkwell::AddressSpace::from(0u16)),
                        "inttoptr_cast",
                    ) {
                        Ok(ptr) => inkwell::values::BasicMetadataValueEnum::PointerValue(ptr),
                        Err(_) => inkwell::values::BasicMetadataValueEnum::IntValue(int_val),
                    }
                } else {
                    inkwell::values::BasicMetadataValueEnum::IntValue(int_val)
                }
            })
            .collect();
        let result = cg.builder.build_call(fn_val, &call_args, "call").ok()?;
        eprintln!("CALL {} returned {:?}", fn_name, result);
        match result.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(basic_val) => match basic_val {
                inkwell::values::BasicValueEnum::IntValue(iv) => Some(iv),
                inkwell::values::BasicValueEnum::PointerValue(pv) => cg
                    .builder
                    .build_ptr_to_int(pv, cg.i64_type, "ptrtoint")
                    .ok(),
                _ => Some(cg.i64_type.const_int(0, false)),
            },
            _ => Some(cg.i64_type.const_int(0, false)),
        }
    } else {
        cg.report_error(format!("function '{}' not found", fn_name));
        Some(cg.i64_type.const_int(0, false))
    }
}

fn build_str_fat_ptr<'ctx>(cg: &Codegen<'ctx>, arg: &HirExpr) -> Option<StructValue<'ctx>> {
    let s = match arg {
        HirExpr::StrLit { value: s, .. } => s.clone(),
        _ => return None,
    };
    let bytes = s.trim_matches('"').as_bytes();
    let ty = cg.context.i8_type().array_type(bytes.len() as u32);
    let name = {
        let mut counter = cg.string_counter.borrow_mut();
        let name = format!("str.{}", *counter);
        *counter += 1;
        name
    };
    let global = cg
        .module
        .add_global(ty, Some(AddressSpace::from(0u16)), &name);
    let elems: Vec<_> = bytes
        .iter()
        .map(|b| cg.context.i8_type().const_int(*b as u64, false))
        .collect();
    let arr = unsafe { inkwell::values::ArrayValue::new_const_array(&ty, &elems) };
    global.set_initializer(&arr);
    global.set_constant(true);
    global.set_linkage(inkwell::module::Linkage::Private);
    let zero32 = cg.context.i32_type().const_int(0, false);
    let ptr = unsafe {
        cg.builder
            .build_gep(ty, global.as_pointer_value(), &[zero32, zero32], "ptr")
            .ok()?
    };
    let len = cg.i64_type.const_int(bytes.len() as u64, false);
    let fat_type = cg.context.struct_type(
        &[
            BasicTypeEnum::PointerType(cg.context.ptr_type(AddressSpace::from(0u16))),
            BasicTypeEnum::IntType(cg.i64_type),
        ],
        false,
    );
    Some(fat_type.const_named_struct(&[
        BasicValueEnum::PointerValue(ptr),
        BasicValueEnum::IntValue(len),
    ]))
}
