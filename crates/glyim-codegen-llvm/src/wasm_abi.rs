use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::FunctionValue;

/// Generate the Wasm ABI wrapper function `expand` that acts as the
/// entry point for macro execution.
///
/// The function signature is `(i32, i32, i32) -> i32`:
///   - param 0: input pointer (offset in linear memory)
///   - param 1: input length (bytes)
///   - param 2: output pointer (offset in linear memory)
///   - return: output length (bytes written)
///
/// This function calls the real Glyim function, converts the i64 result
/// to i32, and returns it.
pub fn generate_wasm_export_wrapper<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    real_fn: FunctionValue<'ctx>,
) -> FunctionValue<'ctx> {
    let i32_type = context.i32_type();
    let i64_type = context.i64_type();

    let wrapper_type =
        i32_type.fn_type(&[i32_type.into(), i32_type.into(), i32_type.into()], false);
    let wrapper = module.add_function("expand", wrapper_type, None);

    let entry = context.append_basic_block(wrapper, "entry");
    let builder = context.create_builder();
    builder.position_at_end(entry);

    // Call the real function (takes no args, returns i64)
    let call_result = builder
        .build_call(real_fn, &[], "call")
        .unwrap()
        .try_as_basic_value();

    let result_val = match call_result {
        inkwell::values::ValueKind::Basic(basic_val) => basic_val.into_int_value(),
        _ => i64_type.const_int(0, false),
    };

    // Truncate i64 to i32 for Wasm return
    let truncated = builder
        .build_int_truncate(result_val, i32_type, "trunc")
        .unwrap();

    builder.build_return(Some(&truncated)).unwrap();

    wrapper
}
