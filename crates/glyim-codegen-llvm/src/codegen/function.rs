use crate::codegen::ctx::FunctionContext;
use crate::Codegen;
use glyim_hir::HirFn;
use glyim_interner::Symbol;
use inkwell::values::PointerValue;
use std::collections::HashMap;

#[tracing::instrument(skip_all)]
#[tracing::instrument(skip_all)]
pub(crate) fn declare_fn<'ctx>(cg: &mut Codegen<'ctx>, f: &HirFn) {
    let name = cg.interner.resolve(f.name);
    eprintln!("[codegen] declare_fn: {} (type_params={:?})", name, f.type_params);
    if cg.module.get_function(name).is_some() {
        eprintln!("[codegen]   -> already exists, skipping");
        return;
    }
    let is_main = name == "main";
    let ret_type = if is_main { cg.i32_type } else { cg.i64_type };
    let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> =
        f.params.iter().map(|_| cg.i64_type.into()).collect();
    cg.module
        .add_function(name, ret_type.fn_type(&param_types, false), None);
}

#[tracing::instrument(skip_all)]
pub(crate) fn codegen_fn<'ctx>(cg: &mut Codegen<'ctx>, f: &HirFn) -> Result<(), String> {
    declare_fn(cg, f);
    let name = cg.interner.resolve(f.name);
    let is_main = name == "main";
    // Fetch the already-declared FunctionValue — never call add_function again
    let fn_value = cg
        .module
        .get_function(name)
        .ok_or_else(|| format!("declare_fn failed for '{}'", name))?;

    // Register DWARF subprogram
    if let Some(ref di) = cg.debug_info {
        let line = crate::debug::DebugInfoGen::byte_offset_to_line(
            cg.source_str.as_deref().unwrap_or(""),
            f.span.start,
        );
        if let Ok(subprogram) = di.create_subprogram(name, line, f.is_macro_generated) {
            cg.macro_fn_names.borrow_mut().insert(f.name);
            di.register_subprogram(f.name, subprogram);
            cg.current_subprogram = Some(subprogram);
        }
    }

    let entry = cg.context.append_basic_block(fn_value, "entry");
    cg.builder.position_at_end(entry);
    let mut vars: HashMap<Symbol, PointerValue<'ctx>> = HashMap::new();
    for (i, (param_sym, _ty)) in f.params.iter().enumerate() {
        let param_val = fn_value.get_nth_param(i as u32).ok_or("missing param")?;
        let alloca = cg
            .builder
            .build_alloca(cg.i64_type, cg.interner.resolve(*param_sym))
            .map_err(|e| e.to_string())?;
        cg.builder
            .build_store(alloca, param_val)
            .map_err(|e| e.to_string())?;
        vars.insert(*param_sym, alloca);
    }
    let mut fctx = FunctionContext { vars, fn_value };
    let body_val = super::stmt::codegen_block(cg, &f.body, &mut fctx).ok_or("codegen fail")?;
    let ret_val = if is_main {
        cg.builder
            .build_int_truncate(body_val, cg.i32_type, "trunc")
            .map_err(|e| e.to_string())?
    } else {
        body_val
    };
    cg.builder
        .build_return(Some(&ret_val))
        .map_err(|e| e.to_string())?;

    // Clear subprogram
    cg.current_subprogram = None;

    if !fn_value.verify(true) {
        return Err("verification fail".into());
    }
    Ok(())
}
