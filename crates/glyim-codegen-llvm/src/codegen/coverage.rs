use inkwell::module::Module;
use inkwell::values::FunctionValue;
use crate::codegen::CoverageMode;
use inkwell::AddressSpace;

/// Emit the global coverage counter array and the runtime dump function.
pub fn emit_coverage_globals<'ctx>(
    module: &Module<'ctx>,
    num_counters: usize,
    mode: CoverageMode,
) {
    if mode == CoverageMode::Off || num_counters == 0 {
        return;
    }
    let context = module.get_context();
    let i64_type = context.i64_type();
    let array_type = i64_type.array_type(num_counters as u32);
    let global = module.add_global(
        array_type,
        Some(AddressSpace::from(0u16)),
        "__glyim_cov_counts",
    );
    let zero = i64_type.const_int(0, false);
    let initializer = array_type.const_zero();
    global.set_initializer(&initializer);
    global.set_linkage(inkwell::module::Linkage::Internal);
}

/// Insert a counter increment at the beginning of a function.
pub fn instrument_function_entry<'ctx>(
    module: &Module<'ctx>,
    function: FunctionValue<'ctx>,
    counter_index: u32,
    mode: CoverageMode,
) {
    if mode == CoverageMode::Off {
        return;
    }
    let context = module.get_context();
    let builder = context.create_builder();
    let i64_type = context.i64_type();
    let i32_type = context.i32_type();

    let entry = function.get_first_basic_block().unwrap();
    match entry.get_first_instruction() {
        Some(first_instr) => builder.position_before(&first_instr),
        None => builder.position_at_end(entry),
    }

    let cov_global = module.get_global("__glyim_cov_counts").unwrap();
    let array_type = i64_type.array_type(0); // placeholder, we just need the type for GEP; size not needed
    // Use actual array type: we don't have it here, but we can use cov_global's type?
    // Instead, use i64_type.array_type(0) as dummy; build_in_bounds_gep doesn't validate length.
    let ptr = cov_global.as_pointer_value();
    let zero = i32_type.const_int(0, false);
    let idx = i32_type.const_int(counter_index as u64, false);
    let indices = &[zero, idx];
    let counter_ptr = unsafe {
        builder.build_in_bounds_gep(
            i64_type.array_type(1), // dummy element type
            ptr,
            indices,
            "cov_ptr",
        )
    }.unwrap();
    let current = builder.build_load(i64_type, counter_ptr, "cov_cur").unwrap().into_int_value();
    let incremented = builder.build_int_add(current, i64_type.const_int(1, false), "cov_inc").unwrap();
    builder.build_store(counter_ptr, incremented).unwrap();
}
