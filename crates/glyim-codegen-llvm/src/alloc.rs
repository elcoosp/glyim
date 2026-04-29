/// Emit extern declarations for malloc and free.
/// Does NOT redeclare abort (already in emit_runtime_shims).
pub fn emit_alloc_shims(module: &inkwell::module::Module<'_>) {
    let i64_type = module.get_context().i64_type();
    let void_type = module.get_context().void_type();
    let ptr_type = module
        .get_context()
        .ptr_type(inkwell::AddressSpace::from(0u16));

    let malloc_ty = ptr_type.fn_type(&[i64_type.into()], false);
    module.add_function("malloc", malloc_ty, None);

    let free_ty = void_type.fn_type(&[ptr_type.into()], false);
    module.add_function("free", free_ty, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn emit_shims_declares_malloc() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module);
        assert!(module.get_function("malloc").is_some());
    }

    #[test]
    fn emit_shims_declares_free() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module);
        assert!(module.get_function("free").is_some());
    }

    #[test]
    fn emit_shims_does_not_declare_abort() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module);
        assert!(module.get_function("abort").is_none());
    }

    #[test]
    fn emit_shims_allows_subsequent_function_creation() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module);
        let fn_type = ctx.i32_type().fn_type(&[], false);
        let _main = module.add_function("main", fn_type, None);
        assert!(module.get_function("main").is_some());
    }

    #[test]
    fn emit_shims_idempotent() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        emit_alloc_shims(&module);
        emit_alloc_shims(&module);
        assert!(module.get_function("malloc").is_some());
    }
}
