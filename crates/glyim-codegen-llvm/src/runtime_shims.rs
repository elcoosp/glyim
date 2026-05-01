use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::BasicTypeEnum;
use inkwell::AddressSpace;

// Rust-native implementations (mapped into JIT)
extern "C" { fn printf(fmt: *const libc::c_char, ...) -> libc::c_int; }
extern "C" { fn write(fd: libc::c_int, buf: *const libc::c_void, count: libc::size_t) -> libc::ssize_t; }
extern "C" { fn abort() -> !; }

#[unsafe(no_mangle)] pub unsafe extern "C" fn glyim_println_int_impl(val: i64) {
    let fmt = b"%lld\n\0".as_ptr() as *const libc::c_char;
    unsafe { printf(fmt, val); }
}
#[unsafe(no_mangle)] pub unsafe extern "C" fn glyim_println_str_impl(ptr: *const u8, len: i64) {
    let s = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    unsafe { write(1, s.as_ptr() as *const libc::c_void, s.len()); }
    unsafe { write(1, b"\n".as_ptr() as *const libc::c_void, 1); }
}
#[unsafe(no_mangle)] pub unsafe extern "C" fn glyim_assert_fail_impl(msg: *const u8, len: i64) {
    let pre = b"assertion failed";
    unsafe { write(2, pre.as_ptr() as *const libc::c_void, pre.len()); }
    if len > 0 && !msg.is_null() {
        let s = unsafe { std::slice::from_raw_parts(msg, len as usize) };
        unsafe { write(2, s.as_ptr() as *const libc::c_void, s.len()); }
    }
    unsafe { write(2, b"\n".as_ptr() as *const libc::c_void, 1); }
    unsafe { abort(); }
}

// ── Always emit LLVM IR bodies (AOT) ──────────────────────────
pub(crate) fn emit_runtime_shims<'a>(context: &'a Context, module: &Module<'a>) {
    let i32_type = context.i32_type();
    let i64_type = context.i64_type();
    let void_type = context.void_type();
    let ptr_type = context.ptr_type(AddressSpace::from(0u16));

    let write_type = i64_type.fn_type(&[i32_type.into(), ptr_type.into(), i64_type.into()], false);
    module.add_function("write", write_type, None);
    module.add_function("abort", void_type.fn_type(&[], false), None);
    module.add_function("printf", i32_type.fn_type(&[ptr_type.into()], true), None);

    let newline_fmt = context.const_string(b"%lld\n", true);
    let str_fmt     = context.const_string(b"%s\n",   true);

    // glyim_println_int(i64)
    {
        let fn_val = module.add_function("glyim_println_int", void_type.fn_type(&[i64_type.into()], false), None);
        let builder = context.create_builder();
        let entry = context.append_basic_block(fn_val, "entry");
        builder.position_at_end(entry);
        let val = fn_val.get_nth_param(0).unwrap().into_int_value();
        builder.build_call(module.get_function("printf").unwrap(), &[newline_fmt.into(), val.into()], "printf_call").unwrap();
        builder.build_return(None).unwrap();
    }
    // glyim_println_str(ptr, len)
    {
        let fn_val = module.add_function("glyim_println_str", void_type.fn_type(&[ptr_type.into(), i64_type.into()], false), None);
        let builder = context.create_builder();
        let entry = context.append_basic_block(fn_val, "entry");
        builder.position_at_end(entry);
        let ptr = fn_val.get_nth_param(0).unwrap().into_pointer_value();
        builder.build_call(module.get_function("printf").unwrap(), &[str_fmt.into(), ptr.into()], "printf_call").unwrap();
        builder.build_return(None).unwrap();
    }
    // glyim_assert_fail(ptr, len)
    {
        let fn_val = module.add_function("glyim_assert_fail", void_type.fn_type(&[ptr_type.into(), i64_type.into()], false), None);
        let builder = context.create_builder();
        let entry = context.append_basic_block(fn_val, "entry");
        builder.position_at_end(entry);
        let msg = fn_val.get_nth_param(0).unwrap().into_pointer_value();
        let len = fn_val.get_nth_param(1).unwrap().into_int_value();
        let stderr = i32_type.const_int(2, false);
        builder.build_call(module.get_function("write").unwrap(), &[stderr.into(), msg.into(), len.into()], "write_stderr").unwrap();
        builder.build_call(module.get_function("abort").unwrap(), &[], "abort").unwrap();
        builder.build_unreachable().unwrap();
    }
}

pub fn map_runtime_shims_for_jit(engine: &inkwell::execution_engine::ExecutionEngine, module: &Module) {
    unsafe {
        if let Some(f) = module.get_function("glyim_println_int")  { engine.add_global_mapping(&f, glyim_println_int_impl  as *const () as usize); }
        if let Some(f) = module.get_function("glyim_println_str")  { engine.add_global_mapping(&f, glyim_println_str_impl  as *const () as usize); }
        if let Some(f) = module.get_function("glyim_assert_fail")  { engine.add_global_mapping(&f, glyim_assert_fail_impl  as *const () as usize); }
    }
}
