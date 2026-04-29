use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::BasicTypeEnum;
use inkwell::values::BasicValue;
use inkwell::AddressSpace;

pub(crate) fn emit_runtime_shims<'a>(context: &'a Context, module: &Module<'a>) {
    let i32_type = context.i32_type();
    let i64_type = context.i64_type();
    let void_type = context.void_type();
    let ptr_type = context.ptr_type(AddressSpace::from(0u16));

    let write_type = i64_type.fn_type(&[i32_type.into(), ptr_type.into(), i64_type.into()], false);
    module.add_function("write", write_type, None);
    module.add_function("abort", void_type.fn_type(&[], false), None);

    let printf_type = i32_type.fn_type(&[ptr_type.into()], true);
    module.add_function("printf", printf_type, None);

    let newline_fmt = context.const_string(b"%lld\n", true);
    let str_fmt = context.const_string(b"%s\n", true);

    // glyim_println_int(i64)
    {
        let fn_type = void_type.fn_type(&[i64_type.into()], false);
        let fn_val = module.add_function("glyim_println_int", fn_type, None);
        let builder = context.create_builder();
        let entry = context.append_basic_block(fn_val, "entry");
        builder.position_at_end(entry);
        let val = fn_val.get_nth_param(0).unwrap().into_int_value();
        builder
            .build_call(
                module.get_function("printf").unwrap(),
                &[newline_fmt.into(), val.into()],
                "printf_call",
            )
            .unwrap();
        builder.build_return(None).unwrap();
    }

    // glyim_println_str({i8*, i64})
    {
        let fat_ptr_type = context.struct_type(
            &[
                BasicTypeEnum::PointerType(ptr_type),
                BasicTypeEnum::IntType(i64_type),
            ],
            false,
        );
        let fn_type = void_type.fn_type(&[fat_ptr_type.into()], false);
        let fn_val = module.add_function("glyim_println_str", fn_type, None);
        let builder = context.create_builder();
        let entry = context.append_basic_block(fn_val, "entry");
        builder.position_at_end(entry);
        let fat = fn_val.get_nth_param(0).unwrap().into_struct_value();
        let data_ptr = builder
            .build_extract_value(fat, 0, "data")
            .unwrap()
            .into_pointer_value();
        builder
            .build_call(
                module.get_function("printf").unwrap(),
                &[str_fmt.into(), data_ptr.into()],
                "printf_call",
            )
            .unwrap();
        builder.build_return(None).unwrap();
    }

    // glyim_assert_fail(i8* msg, i64 len)
    {
        let fn_type = void_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
        let fn_val = module.add_function("glyim_assert_fail", fn_type, None);
        let builder = context.create_builder();
        let entry = context.append_basic_block(fn_val, "entry");
        builder.position_at_end(entry);
        let msg = fn_val.get_nth_param(0).unwrap().into_pointer_value();
        let len = fn_val.get_nth_param(1).unwrap().into_int_value();
        let stderr = i32_type.const_int(2, false);
        builder
            .build_call(
                module.get_function("write").unwrap(),
                &[stderr.into(), msg.into(), len.into()],
                "write_stderr",
            )
            .unwrap();
        builder
            .build_call(module.get_function("abort").unwrap(), &[], "abort")
            .unwrap();
        builder.build_unreachable().unwrap();
    }
}
