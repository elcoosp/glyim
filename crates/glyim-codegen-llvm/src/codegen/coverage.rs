use inkwell::module::Module;
use inkwell::values::FunctionValue;
use crate::codegen::CoverageMode;
use inkwell::AddressSpace;
use glyim_coverage::data::{LocationKind, SourceLocation};
use std::collections::HashMap;

pub struct CoverageInstrumenter {
    pub counter_id: u64,
    pub metadata: HashMap<u64, SourceLocation>,
}

impl CoverageInstrumenter {
    pub fn new() -> Self {
        Self { counter_id: 0, metadata: HashMap::new() }
    }

    pub fn record_function_entry(&mut self, file_id: u32, line: u32) -> u64 {
        let id = self.counter_id;
        self.metadata.insert(id, SourceLocation {
            file_id,
            start_line: line,
            start_col: 0,
            end_line: line,
            end_col: 0,
            kind: LocationKind::FunctionEntry,
        });
        self.counter_id += 1;
        id
    }
}

/// Emit the global coverage counter array and the runtime dump function.
use inkwell::module::Module as InkModule;

pub fn emit_coverage_dump_global(
    module: &InkModule<'_>,
    instrumenter: &CoverageInstrumenter,
    file_path: &str,
) -> Result<(), String> {
    use std::collections::HashMap;
    let mut files = HashMap::new();
    files.insert(0u32, glyim_coverage::data::FileInfo {
        path: file_path.to_string(),
    });
    let dump = glyim_coverage::data::CoverageDump {
        files,
        counters: HashMap::new(),
        metadata: instrumenter.metadata.clone(),
        version: 1,
    };
    let json = serde_json::to_string(&dump).map_err(|e| e.to_string())?;
    let ctx = module.get_context();
    let i8_type = ctx.i8_type();
    let arr_type = i8_type.array_type(json.len() as u32);
    let global = module.add_global(
        arr_type,
        Some(inkwell::AddressSpace::from(0u16)),
        "__glyim_cov_dump",
    );
    let elems: Vec<_> = json.as_bytes().iter().map(|&b| i8_type.const_int(b as u64, false)).collect();
    let const_array = unsafe { inkwell::values::ArrayValue::new_const_array(&arr_type, &elems) };
    global.set_initializer(&const_array);
    global.set_constant(true);
    global.set_linkage(inkwell::module::Linkage::Private);
    Ok(())
}

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
    let _zero = i64_type.const_int(0, false);
    let initializer = array_type.const_zero();
    global.set_initializer(&initializer);
    global.set_linkage(inkwell::module::Linkage::Internal);
}

/// Insert a counter increment at the beginning of a function.
pub fn instrument_function_entry<'ctx>(
    module: &Module<'ctx>,
    function: FunctionValue<'ctx>,
    counter_index: u64,
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
    let _array_type = i64_type.array_type(0); // placeholder, we just need the type for GEP; size not needed
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

pub fn emit_coverage_flush_call(cg: &crate::codegen::Codegen) {
    if cg.coverage_mode == CoverageMode::Off {
        return;
    }
    let module = &cg.module;
    let counters_global = match module.get_global("__glyim_cov_counts") {
        Some(g) => g,
        None => return,
    };
    // Create dump global now with current instrumenter data
    let dump_global = if let Some(existing) = module.get_global("__glyim_cov_dump") {
        existing
    } else {
        use std::collections::HashMap;
        let instr_meta = match cg.coverage_instrumenter.as_ref() {
            Some(i) => i.metadata.clone(),
            None => HashMap::new(),
        };
        let mut files = HashMap::new();
        files.insert(0u32, glyim_coverage::data::FileInfo {
            path: cg.source_str.as_deref().unwrap_or("unknown").to_string(),
        });
        let dump = glyim_coverage::data::CoverageDump {
            files,
            counters: HashMap::new(),
            metadata: instr_meta,
            version: 1,
        };
        let json = serde_json::to_string(&dump).unwrap();
        let ctx = module.get_context();
        let i8_type = ctx.i8_type();
        let arr_type = i8_type.array_type(json.len() as u32);
        let g = module.add_global(
            arr_type,
            Some(inkwell::AddressSpace::from(0u16)),
            "__glyim_cov_dump",
        );
        let elems: Vec<_> = json.as_bytes().iter().map(|&b| i8_type.const_int(b as u64, false)).collect();
        let const_array = unsafe { inkwell::values::ArrayValue::new_const_array(&arr_type, &elems) };
        g.set_initializer(&const_array);
        g.set_constant(true);
        g.set_linkage(inkwell::module::Linkage::Private);
        g
    };
    let flush_fn = module.get_function("glyim_cov_flush_impl").unwrap_or_else(|| {
        let ctx = module.get_context();
        let void_type = ctx.void_type();
        let i64_type = ctx.i64_type();
        let ptr_type = ctx.ptr_type(inkwell::AddressSpace::from(0u16));
        let fn_type = void_type.fn_type(&[
            ptr_type.into(),
            i64_type.into(),
            ptr_type.into(),
            i64_type.into(),
            ptr_type.into(),
        ], false);
        module.add_function("glyim_cov_flush_impl", fn_type, None)
    });

    let builder = &cg.builder;
    let i64_type = cg.i64_type;
    let counters_ptr = counters_global.as_pointer_value();
    let counters_len = match cg.coverage_instrumenter.as_ref() {
        Some(instr) => i64_type.const_int(instr.counter_id, false),
        None => i64_type.const_int(0, false),
    };
    let dump_ptr = dump_global.as_pointer_value();
    let dump_len = {
        let arr_type = dump_global.get_value_type();
        arr_type.size_of().unwrap_or(i64_type.const_int(0, false))
    };
    let out_path_ptr = {
        let path = std::env::var("GLYIM_COV_FILE").unwrap_or_else(|_| "glyim-cov.json".to_string());
        let path_bytes = format!("{}\0", path);
        let bytes = path_bytes.as_bytes();
        let i8_type = cg.context.i8_type();
        let arr_type = i8_type.array_type(bytes.len() as u32);
        let global = module.add_global(arr_type, Some(inkwell::AddressSpace::from(0u16)), "cov_out_path");
        let elems: Vec<_> = bytes.iter().map(|&b| i8_type.const_int(b as u64, false)).collect();
        let const_array = unsafe { inkwell::values::ArrayValue::new_const_array(&arr_type, &elems) };
        global.set_initializer(&const_array);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        let zero = cg.i32_type.const_int(0, false);
        unsafe { cg.builder.build_gep(arr_type, global.as_pointer_value(), &[zero, zero], "cov_path_ptr") }.unwrap()
    };

    let _ = builder.build_call(
        flush_fn,
        &[
            counters_ptr.into(),
            counters_len.into(),
            dump_ptr.into(),
            dump_len.into(),
            out_path_ptr.into(),
        ],
        "cov_flush",
    );
}
