#![allow(clippy::missing_safety_doc)]
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::AddressSpace;

// Rust-native implementations (mapped into JIT via add_global_mapping)
extern "C" { fn printf(fmt: *const libc::c_char, ...) -> libc::c_int; }
extern "C" { fn write(fd: libc::c_int, buf: *const libc::c_void, count: libc::size_t) -> libc::ssize_t; }
extern "C" { fn abort() -> !; }

#[unsafe(no_mangle)] pub unsafe extern "C" fn glyim_println_int_impl(val: i64)  { let f = b"%lld\n\0".as_ptr() as *const libc::c_char; unsafe { printf(f, val); } }
#[unsafe(no_mangle)] pub unsafe extern "C" fn glyim_println_str_impl(ptr: *const u8, len: i64) { let s = unsafe { std::slice::from_raw_parts(ptr, len as usize) }; unsafe { write(1, s.as_ptr() as *const libc::c_void, s.len()); write(1, b"\n".as_ptr() as *const libc::c_void, 1); } }
#[unsafe(no_mangle)] pub unsafe extern "C" fn glyim_assert_fail_impl(msg: *const u8, len: i64) { let p = b"assertion failed"; unsafe { write(2, p.as_ptr() as *const libc::c_void, p.len()); if len > 0 && !msg.is_null() { let s = std::slice::from_raw_parts(msg, len as usize); write(2, s.as_ptr() as *const libc::c_void, s.len()); } write(2, b"\n".as_ptr() as *const libc::c_void, 1); abort(); } }

// ── Module declarations (AOT or JIT) ──
pub(crate) fn emit_runtime_shims<'a>(context: &'a Context, module: &Module<'a>, jit: bool) {
    let i32_type = context.i32_type();
    let i64_type = context.i64_type();
    let void_type = context.void_type();
    let ptr_type = context.ptr_type(AddressSpace::from(0u16));

    let write_type = i64_type.fn_type(&[i32_type.into(), ptr_type.into(), i64_type.into()], false);
    module.add_function("write", write_type, None);
    module.add_function("abort", void_type.fn_type(&[], false), None);
    module.add_function("printf", i32_type.fn_type(&[ptr_type.into()], true), None);

    // glyim_println_int(i64)
    let pint_fn = module.add_function("glyim_println_int", void_type.fn_type(&[i64_type.into()], false), None);
    // glyim_println_str(ptr, len) — flat params
    let pstr_fn = module.add_function("glyim_println_str", void_type.fn_type(&[ptr_type.into(), i64_type.into()], false), None);
    // glyim_assert_fail(ptr, len)
    let afail_fn = module.add_function("glyim_assert_fail", void_type.fn_type(&[ptr_type.into(), i64_type.into()], false), None);

    if jit {
        // No bodies — the JIT resolves these via add_global_mapping
        return;
    }

    // AOT: emit IR bodies
    let newline_fmt = context.const_string(b"%lld\n", true);
    let str_fmt     = context.const_string(b"%s\n",   true);

    {
        let b = context.create_builder();
        b.position_at_end(context.append_basic_block(pint_fn, "entry"));
        let v = pint_fn.get_nth_param(0).unwrap().into_int_value();
        b.build_call(module.get_function("printf").unwrap(), &[newline_fmt.into(), v.into()], "c").unwrap();
        b.build_return(None).unwrap();
    }
    {
        let b = context.create_builder();
        b.position_at_end(context.append_basic_block(pstr_fn, "entry"));
        let p = pstr_fn.get_nth_param(0).unwrap().into_pointer_value();
        b.build_call(module.get_function("printf").unwrap(), &[str_fmt.into(), p.into()], "c").unwrap();
        b.build_return(None).unwrap();
    }
    {
        let b = context.create_builder();
        b.position_at_end(context.append_basic_block(afail_fn, "entry"));
        let m = afail_fn.get_nth_param(0).unwrap().into_pointer_value();
        let l = afail_fn.get_nth_param(1).unwrap().into_int_value();
        b.build_call(module.get_function("write").unwrap(), &[i32_type.const_int(2,false).into(), m.into(), l.into()], "w").unwrap();
        b.build_call(module.get_function("abort").unwrap(), &[], "a").unwrap();
        b.build_unreachable().unwrap();
    }
}

pub fn map_runtime_shims_for_jit(
    engine: &inkwell::execution_engine::ExecutionEngine,
    module: &Module,
    custom_assert_fn: Option<unsafe extern "C" fn(*const u8, i64)>,
    custom_abort_fn: Option<unsafe extern "C" fn()>,
) {
    unsafe {
        if let Some(f) = module.get_function("glyim_println_int") { engine.add_global_mapping(&f, glyim_println_int_impl  as *const () as usize); }
        if let Some(f) = module.get_function("glyim_println_str") { engine.add_global_mapping(&f, glyim_println_str_impl  as *const () as usize); }
        if let Some(f) = module.get_function("glyim_assert_fail") {
            let ptr = custom_assert_fn.unwrap_or(glyim_assert_fail_impl);
            engine.add_global_mapping(&f, ptr as *const () as usize);
        }
        if let Some(f) = module.get_function("abort") {
            // CUSTOM_ABORT_FN is now directly `fn()`, no transmute needed
            let ptr: unsafe extern "C" fn() = custom_abort_fn.unwrap_or(abort_handler_default);
            engine.add_global_mapping(&f, ptr as *const () as usize);
        }

        // Default abort handler (for non-custom paths) that actually aborts
        #[unsafe(no_mangle)]
        unsafe extern "C" fn abort_handler_default() {
            std::process::abort();
        }
    }
}
