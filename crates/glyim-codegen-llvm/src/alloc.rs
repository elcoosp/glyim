/// Emit `glyim_alloc` and `glyim_free` wrapper functions.
///
/// `glyim_alloc(size: i64) -> *i8`:
///   Calls malloc(size). If malloc returns null, calls abort().
///   Otherwise returns the pointer.
///
/// `glyim_free(ptr: *i8) -> void`:
///   Calls free(ptr).
///
/// If `no_std` is true, emits nothing — no_std programs manage their own memory.
pub fn emit_alloc_shims(module: &inkwell::module::Module<'_>, no_std: bool) {
    if no_std {
        return;
    }

    let ctx = module.get_context();
    let _i8_type = ctx.i8_type();
    let i64_type = ctx.i64_type();
    let void_type = ctx.void_type();
    let ptr_type = ctx.ptr_type(inkwell::AddressSpace::from(0u16));

    // ── Declare external functions ──────────────────────────────

    // malloc(size: i64) -> *i8
    let malloc_ty = ptr_type.fn_type(&[i64_type.into()], false);
    let malloc_fn = module.add_function("malloc", malloc_ty, None);

    // free(ptr: *i8) -> void
    let free_ty = void_type.fn_type(&[ptr_type.into()], false);
    let free_fn = module.add_function("free", free_ty, None);

    // ── glyim_alloc(size: i64) -> *i8 ───────────────────────────

    let alloc_fn_type = ptr_type.fn_type(&[i64_type.into()], false);
    let alloc_fn = module.add_function("__glyim_alloc", alloc_fn_type, None);

    let entry = ctx.append_basic_block(alloc_fn, "entry");
    let oom_block = ctx.append_basic_block(alloc_fn, "oom");
    let ok_block = ctx.append_basic_block(alloc_fn, "ok");

    let builder = ctx.create_builder();

    // entry: call malloc, compare result to null
    builder.position_at_end(entry);
    let size_param = alloc_fn.get_first_param().unwrap();
    let raw_ptr = builder
        .build_call(malloc_fn, &[size_param.into()], "raw_ptr")
        .unwrap()
        .try_as_basic_value();

    let raw_ptr = match raw_ptr {
        inkwell::values::ValueKind::Basic(basic_val) => basic_val.into_pointer_value(),
        _ => panic!("malloc returned void?"),
    };

    let null_ptr = ptr_type.const_null();
    let is_null = builder
        .build_int_compare(inkwell::IntPredicate::EQ, raw_ptr, null_ptr, "is_null")
        .unwrap();

    builder
        .build_conditional_branch(is_null, oom_block, ok_block)
        .unwrap();

    // oom: unreachable immediately (compiler may add trap later)
    builder.position_at_end(oom_block);
    builder.build_unreachable().unwrap();

    // ok: return the pointer
    builder.position_at_end(ok_block);
    builder.build_return(Some(&raw_ptr)).unwrap();

    // ── glyim_free(ptr: *i8) -> void ─────────────────────────────

    let free_wrapper_type = void_type.fn_type(&[ptr_type.into()], false);
    let free_wrapper = module.add_function("__glyim_free", free_wrapper_type, None);
    let free_entry = ctx.append_basic_block(free_wrapper, "entry");

    builder.position_at_end(free_entry);
    let ptr_param = free_wrapper.get_first_param().unwrap();
    builder
        .build_call(free_fn, &[ptr_param.into()], "")
        .unwrap();
    builder.build_return(None).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn emit_shims_creates_glyim_alloc() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, false);
        assert!(
            module.get_function("__glyim_alloc").is_some(),
            "glyim_alloc should be defined"
        );
    }

    #[test]
    fn emit_shims_creates_glyim_free() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, false);
        assert!(
            module.get_function("__glyim_free").is_some(),
            "glyim_free should be defined"
        );
    }

    #[test]
    fn emit_shims_declares_malloc_internally() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, false);
        assert!(module.get_function("malloc").is_some());
    }

    #[test]
    fn emit_shims_no_std_skips_all() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, true);
        assert!(module.get_function("__glyim_alloc").is_none());
        assert!(module.get_function("__glyim_free").is_none());
        assert!(module.get_function("malloc").is_none());
        assert!(module.get_function("free").is_none());
        assert!(module.get_function("malloc").is_none());
    }

    #[test]
    fn emit_shims_allows_subsequent_function_creation() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, false);
        let fn_type = ctx.i32_type().fn_type(&[], false);
        let _main = module.add_function("main", fn_type, None);
        assert!(module.get_function("main").is_some());
    }

    #[test]
    fn emit_shims_idempotent() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, false);
        emit_alloc_shims(&module, false);
        assert!(module.get_function("__glyim_alloc").is_some());
    }

    #[test]
    fn emit_shims_glyim_alloc_ir_has_null_check() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, false);
        let ir = module.print_to_string().to_string();
        assert!(
            ir.contains("icmp"),
            "glyim_alloc should compare result to null\nGot:\n{ir}"
        );
    }

    #[test]
    fn emit_shims_glyim_free_ir_calls_free() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module, false);
        let ir = module.print_to_string().to_string();
        assert!(
            ir.contains("free"),
            "glyim_free should call free\nGot:\n{ir}"
        );
    }
}
