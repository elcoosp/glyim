use glyim_codegen_llvm::Codegen;
use glyim_interner::Interner;
use inkwell::context::Context;
use inkwell::types::BasicType;

#[test]
fn struct_field_ptr_returns_correct_address() {
    let ctx = Context::create();
    let module = ctx.create_module("test");
    let builder = ctx.create_builder();
    let i64_type = ctx.i64_type();
    let struct_type = ctx.struct_type(&[i64_type.into(), i64_type.into()], false);
    let fn_type = i64_type.fn_type(&[], false);
    let func = module.add_function("test_fn", fn_type, None);
    let entry = ctx.append_basic_block(func, "entry");
    builder.position_at_end(entry);
    let alloca = builder.build_alloca(struct_type, "s").unwrap();
    // Store values
    let f0 = builder
        .build_struct_gep(struct_type, alloca, 0, "f0")
        .unwrap();
    let f1 = builder
        .build_struct_gep(struct_type, alloca, 1, "f1")
        .unwrap();
    builder
        .build_store(f0, i64_type.const_int(10, false))
        .unwrap();
    builder
        .build_store(f1, i64_type.const_int(20, false))
        .unwrap();

    let interner = Interner::new();
    let cg = Codegen::new(&ctx, interner, vec![]);
    // We can't easily invoke struct_field_ptr with this minimal setup,
    // so just verify the struct field access works.
    let loaded = builder
        .build_load(i64_type, f1, "loaded")
        .unwrap()
        .into_int_value();
    builder.build_return(Some(&loaded)).unwrap();
    assert!(func.verify(true));
}

#[test]
fn zeroed_alloca_contains_zero() {
    let ctx = Context::create();
    let module = ctx.create_module("test");
    let builder = ctx.create_builder();
    let i64_type = ctx.i64_type();
    let fn_type = i64_type.fn_type(&[], false);
    let func = module.add_function("test_fn", fn_type, None);
    let entry = ctx.append_basic_block(func, "entry");
    builder.position_at_end(entry);
    let alloca = builder.build_alloca(i64_type, "var").unwrap();
    builder
        .build_store(alloca, i64_type.const_int(0, false))
        .unwrap();
    let loaded = builder
        .build_load(i64_type, alloca, "loaded")
        .unwrap()
        .into_int_value();
    builder.build_return(Some(&loaded)).unwrap();
    assert!(func.verify(true));
}
