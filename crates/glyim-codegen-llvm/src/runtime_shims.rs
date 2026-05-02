#![allow(clippy::missing_safety_doc)]
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{ArrayValue, IntValue, PointerValue};
use inkwell::AddressSpace;

extern "C" {
    fn printf(fmt: *const libc::c_char, ...) -> libc::c_int;
}
extern "C" {
    fn write(fd: libc::c_int, buf: *const libc::c_void, count: libc::size_t) -> libc::ssize_t;
}
extern "C" {
    fn abort() -> !;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_println_int_impl(val: i64) {
    let f = b"%lld\n\0".as_ptr() as *const libc::c_char;
    unsafe {
        printf(f, val);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_println_str_impl(ptr: *const u8, len: i64) {
    let s = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    unsafe {
        write(1, s.as_ptr() as *const libc::c_void, s.len());
        write(1, b"\n".as_ptr() as *const libc::c_void, 1);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_assert_fail_impl(msg: *const u8, len: i64) {
    let p = b"assertion failed";
    unsafe {
        write(2, p.as_ptr() as *const libc::c_void, p.len());
        if len > 0 && !msg.is_null() {
            let s = std::slice::from_raw_parts(msg, len as usize);
            write(2, s.as_ptr() as *const libc::c_void, s.len());
        }
        write(2, b"\n".as_ptr() as *const libc::c_void, 1);
        abort();
    }
}

/// Helper: create a global constant null-terminated string and return an i8* pointer to it.
unsafe fn create_fmt_ptr<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    bytes: &[u8],
    name: &str,
) -> PointerValue<'ctx> {
    let ty = context.i8_type().array_type(bytes.len() as u32);
    let global = module.add_global(ty, Some(AddressSpace::from(0u16)), name);
    let elems: Vec<IntValue> = bytes
        .iter()
        .map(|&b| context.i8_type().const_int(b as u64, false))
        .collect();
    let arr = ArrayValue::new_const_array(&ty, &elems);
    global.set_initializer(&arr);
    global.set_constant(true);
    global.set_linkage(inkwell::module::Linkage::Private);
    let zero = context.i32_type().const_int(0, false);
    global
        .as_pointer_value()
        .const_in_bounds_gep(ty, &[zero, zero])
}

pub(crate) fn emit_runtime_shims<'a>(context: &'a Context, module: &Module<'a>, jit: bool) {
    let i32_type = context.i32_type();
    let i64_type = context.i64_type();
    let void_type = context.void_type();
    let ptr_type = context.ptr_type(AddressSpace::from(0u16));

    let write_type = i64_type.fn_type(&[i32_type.into(), ptr_type.into(), i64_type.into()], false);
    if module.get_function("write").is_none() {
        module.add_function("write", write_type, None);
    }
    let open_type = i64_type.fn_type(&[ptr_type.into(), i32_type.into()], false);
    if module.get_function("open").is_none() {
        module.add_function("open", open_type, None);
    }
    let close_type = i32_type.fn_type(&[i32_type.into()], false);
    if module.get_function("close").is_none() {
        module.add_function("close", close_type, None);
    }

    if module.get_function("abort").is_none() {
        module.add_function("abort", void_type.fn_type(&[], false), None);
    }
    if module.get_function("printf").is_none() {
        module.add_function("printf", i32_type.fn_type(&[ptr_type.into()], true), None);
    }
    if module.get_function("abort").is_none() {
        module.add_function("abort", void_type.fn_type(&[], false), None);
    }
    if module.get_function("printf").is_none() {
        module.add_function("printf", i32_type.fn_type(&[ptr_type.into()], true), None);
    }

    let pint_fn = module.add_function(
        "__glyim_println_int",
        void_type.fn_type(&[i64_type.into()], false),
        None,
    );
    let pstr_fn = module.add_function(
        "__glyim_println_str",
        void_type.fn_type(&[ptr_type.into(), i64_type.into()], false),
        None,
    );
    let afail_fn = module.add_function(
        "__glyim_assert_fail",
        void_type.fn_type(&[ptr_type.into(), i64_type.into()], false),
        None,
    );

    if jit {
        return;
    }

    // AOT: emit IR bodies with correct format string pointers
    unsafe {
        let newline_fmt = create_fmt_ptr(context, module, b"%lld\n\0", "newline_fmt");
        let str_fmt = create_fmt_ptr(context, module, b"%s\n\0", "str_fmt");

        {
            let b = context.create_builder();
            b.position_at_end(context.append_basic_block(pint_fn, "entry"));
            let v = pint_fn.get_nth_param(0).unwrap().into_int_value();
            b.build_call(
                module.get_function("printf").unwrap(),
                &[newline_fmt.into(), v.into()],
                "c",
            )
            .unwrap();
            b.build_return(None).unwrap();
        }
        {
            let b = context.create_builder();
            b.position_at_end(context.append_basic_block(pstr_fn, "entry"));
            let p = pstr_fn.get_nth_param(0).unwrap().into_pointer_value();
            b.build_call(
                module.get_function("printf").unwrap(),
                &[str_fmt.into(), p.into()],
                "c",
            )
            .unwrap();
            b.build_return(None).unwrap();
        }
        {
            let b = context.create_builder();
            b.position_at_end(context.append_basic_block(afail_fn, "entry"));
            let m = afail_fn.get_nth_param(0).unwrap().into_pointer_value();
            let l = afail_fn.get_nth_param(1).unwrap().into_int_value();
            b.build_call(
                module.get_function("write").unwrap(),
                &[i32_type.const_int(2, false).into(), m.into(), l.into()],
                "w",
            )
            .unwrap();
            b.build_call(module.get_function("abort").unwrap(), &[], "a")
                .unwrap();
            b.build_unreachable().unwrap();
        }
    }
}

pub fn map_runtime_shims_for_jit(
    engine: &inkwell::execution_engine::ExecutionEngine,
    module: &Module,
    custom_assert_fn: Option<unsafe extern "C" fn(*const u8, i64)>,
    custom_abort_fn: Option<unsafe extern "C" fn()>,
) {
    if let Some(f) = module.get_function("__glyim_println_int") {
        engine.add_global_mapping(&f, glyim_println_int_impl as *const () as usize);
    }
    if let Some(f) = module.get_function("__glyim_println_str") {
        engine.add_global_mapping(&f, glyim_println_str_impl as *const () as usize);
    }
    if let Some(f) = module.get_function("__glyim_assert_fail") {
        let ptr = custom_assert_fn.unwrap_or(glyim_assert_fail_impl);
        engine.add_global_mapping(&f, ptr as *const () as usize);
    }
    if let Some(f) = module.get_function("open") {
        engine.add_global_mapping(&f, libc::open as *const () as usize);
    }
    if let Some(f) = module.get_function("close") {
        engine.add_global_mapping(&f, libc::close as *const () as usize);
    }
    if let Some(f) = module.get_function("abort") {
        let ptr: unsafe extern "C" fn() = custom_abort_fn.unwrap_or(abort_handler_default);
        engine.add_global_mapping(&f, ptr as *const () as usize);
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn abort_handler_default() {
        std::process::abort();
    }
}
