mod abi_classify;
mod aggregate_lowering;
mod assert_terminator;
mod bitwise_ops;
mod call_abi;
mod call_c_abi;
mod call_cleanup;
mod call_direct;
mod call_fn_ptr;
mod call_large_struct_arg;
mod call_multi_args;
mod call_sret;
mod call_sret_with_args;
mod call_struct_args;
mod call_void_return;
mod cast_ops;
mod comparisons;
mod compliance_tests;
mod debug_info;
mod drop_dealloc;
mod edge_cases;
mod field_projection;
mod float_arith;
mod float_lowering;
mod fn_sig_missing;
mod helpers;
mod integer_arith;
mod len_lowering;
mod llvm_backend_tests;
mod llvm_module;
mod logical_ops;
mod object_format_tests;
mod optimisation;
mod overflow_checks;
mod pass_mode;
mod repeat_lowering;
mod rvalue_aggregate;
mod s09_mappings;
mod slice_lowering;
mod stress_tests;
mod string_constant;
mod switch_edge_cases;
mod switch_int;
mod type_lowering;
mod u_codegen;
mod unary_ops;
mod unsigned_cmp;


/// Test-only helper: pair each `Arc<Body>` with the plain legacy LLVM
/// symbol (`__glyim_fn_<local_id>`). Tests don't exercise generic
/// instantiations, so the un-mangled form is sufficient — it's what the
/// pipeline would produce for a non-generic fn.
pub(crate) fn to_pairs(
    bodies: &[std::sync::Arc<glyim_mir::Body>],
) -> Vec<(String, std::sync::Arc<glyim_mir::Body>)> {
    bodies
        .iter()
        .map(|b| {
            let sym = format!("__glyim_fn_{}", b.owner.local_id.to_raw());
            (sym, b.clone())
        })
        .collect()
}
