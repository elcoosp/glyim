pub(crate) mod ctx;
mod expr;
mod function;
mod monomorphize;
mod ops;
mod stmt;
mod string;
mod types;
mod coverage;

use crate::debug::DebugInfoGen;
use glyim_diag::Span;
use glyim_hir::{Hir, HirType};
use glyim_interner::{Interner, Symbol};
use inkwell::context::Context;
use inkwell::debug_info::{DISubprogram, DWARFEmissionKind};
use inkwell::module::Module;
use inkwell::types::IntType;
use std::cell::RefCell;
use std::collections::HashMap;

/// Controls the level of debug information emitted during codegen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DebugMode {
    /// No debug info (release builds)
    None,
    /// DWARF line tables only (fast builds with source locations)
    LineTablesOnly,
    /// Full DWARF debug info (debug builds)
    Full,
}

/// Controls the level of coverage instrumentation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CoverageMode {
    /// No instrumentation (default).
    Off,
    /// Instrument function entries only.
    Function,
    /// Instrument function entries and branch conditions.
    Branch,
    /// Instrument function entries, branch conditions, and expression evaluations.
    Full,
}

impl Default for CoverageMode {
    fn default() -> Self {
        Self::Off
    }
}

pub struct CodegenBuilder<'ctx> {
    coverage_mode: CoverageMode,
    context: &'ctx Context,
    interner: Interner,
    expr_types: Vec<HirType>,
    debug_mode: DebugMode,
    source: Option<String>,
    file_name: Option<String>,
    pub(crate) library_mode: bool,
}

impl<'ctx> CodegenBuilder<'ctx> {
    pub fn new(context: &'ctx Context, interner: Interner, expr_types: Vec<HirType>) -> Self {
        Self {
            context,
            interner,
            expr_types,
            debug_mode: DebugMode::None,
            coverage_mode: CoverageMode::Off,
            library_mode: false,
            source: None,
            file_name: None,
        }
    }

    pub fn build(mut self) -> Result<Codegen<'ctx>, String> {
        let module = self.context.create_module("glyim_out");
        let builder = self.context.create_builder();
        let option_sym = self.interner.intern("Option");
        let result_sym = self.interner.intern("Result");

        let debug_info = match self.debug_mode {
            DebugMode::Full => {
                let file_name = self.file_name.as_deref().unwrap_or("jit");
                DebugInfoGen::new(&module, file_name, DWARFEmissionKind::Full).ok()
            }
            DebugMode::LineTablesOnly => {
                let file_name = self.file_name.as_deref().unwrap_or("jit");
                DebugInfoGen::new(&module, file_name, DWARFEmissionKind::LineTablesOnly).ok()
            }
            DebugMode::None => None,
        };

        Ok(Codegen {
            coverage_mode: self.coverage_mode,
            context: self.context,
            module,
            builder,
            i64_type: self.context.i64_type(),
            i32_type: self.context.i32_type(),
            f64_type: self.context.f64_type(),
            interner: self.interner,
            string_counter: RefCell::new(0),
            expr_types: self.expr_types,
            mono_cache: RefCell::new(HashMap::new()),
            struct_types: RefCell::new(HashMap::new()),
            struct_field_indices: RefCell::new(HashMap::new()),
            enum_types: RefCell::new(HashMap::new()),
            enum_struct_types: RefCell::new(HashMap::new()),
            enum_variant_tags: RefCell::new(HashMap::new()),
            option_sym,
            result_sym,
            debug_info,
            source_str: self.source,
            current_subprogram: None,
            no_std: false,
            extern_methods: std::collections::HashMap::new(),
            effect_analysis: None,
            jit_mode: false,
            library_mode: self.library_mode,
            target_triple: None,
            macro_fn_names: RefCell::new(std::collections::HashSet::new()),
            errors: RefCell::new(Vec::new()),
        })
    }

    pub fn with_library_mode(mut self) -> Self {
        self.library_mode = true;
        self
    }
}

pub struct Codegen<'ctx> {
    pub(crate) coverage_mode: CoverageMode,
    pub(crate) context: &'ctx Context,
    pub(crate) module: Module<'ctx>,
    pub(crate) builder: inkwell::builder::Builder<'ctx>,
    pub(crate) i64_type: IntType<'ctx>,
    pub(crate) i32_type: IntType<'ctx>,
    #[allow(dead_code)]
    pub(crate) f64_type: inkwell::types::FloatType<'ctx>,
    pub(crate) interner: Interner,
    pub(crate) string_counter: RefCell<u32>,
    pub(crate) expr_types: Vec<HirType>,
    #[allow(dead_code)]
    pub(crate) mono_cache:
        RefCell<HashMap<(Symbol, Vec<HirType>), inkwell::values::FunctionValue<'ctx>>>,
    pub(crate) struct_types: RefCell<HashMap<Symbol, inkwell::types::StructType<'ctx>>>,
    pub(crate) struct_field_indices: RefCell<HashMap<(Symbol, Symbol), usize>>,
    pub(crate) enum_types:
        RefCell<HashMap<Symbol, (IntType<'ctx>, inkwell::types::ArrayType<'ctx>)>>,
    pub(crate) enum_struct_types: RefCell<HashMap<Symbol, inkwell::types::StructType<'ctx>>>,
    pub(crate) enum_variant_tags: RefCell<HashMap<(Symbol, Symbol), u32>>,
    #[allow(dead_code)]
    pub(crate) option_sym: Symbol,
    #[allow(dead_code)]
    pub(crate) result_sym: Symbol,
    debug_info: Option<DebugInfoGen<'ctx>>,
    source_str: Option<String>,
    current_subprogram: Option<DISubprogram<'ctx>>,
    pub(crate) macro_fn_names: std::cell::RefCell<std::collections::HashSet<Symbol>>,
    pub(crate) no_std: bool,
    pub(crate) extern_methods:
        std::collections::HashMap<glyim_interner::Symbol, glyim_interner::Symbol>,
    pub(crate) errors: RefCell<Vec<String>>,
    pub(crate) jit_mode: bool,
    pub(crate) library_mode: bool,
    /// Effect analysis results (Phase 3) – drives LLVM attribute annotation.
    pub(crate) effect_analysis: Option<glyim_hir::effects::EffectSet>,
    pub(crate) target_triple: Option<String>,
}

impl<'ctx> Codegen<'ctx> {
    // Use CodegenBuilder::new(...).build() instead

    pub fn with_debug(
        context: &'ctx Context,
        mut interner: Interner,
        expr_types: Vec<HirType>,
        source_str: String,
        file_name: &str,
    ) -> Result<Self, String> {
        let module = context.create_module("glyim_out");
        let builder = context.create_builder();
        let option_sym = interner.intern("Option");
        let result_sym = interner.intern("Result");
        let debug_info = DebugInfoGen::new(&module, file_name, DWARFEmissionKind::Full).ok();
        Ok(Self {
            coverage_mode: CoverageMode::Off,
            context,
            module,
            builder,
            i64_type: context.i64_type(),
            i32_type: context.i32_type(),
            f64_type: context.f64_type(),
            interner,
            string_counter: RefCell::new(0),
            expr_types,
            mono_cache: RefCell::new(HashMap::new()),
            struct_types: RefCell::new(HashMap::new()),
            struct_field_indices: RefCell::new(HashMap::new()),
            enum_types: RefCell::new(HashMap::new()),
            enum_struct_types: RefCell::new(HashMap::new()),
            enum_variant_tags: RefCell::new(HashMap::new()),
            option_sym,
            result_sym,
            debug_info,
            source_str: Some(source_str),
            current_subprogram: None,
            no_std: false,
            extern_methods: std::collections::HashMap::new(),
            effect_analysis: None,
            jit_mode: false,
            library_mode: false,
            target_triple: None,
            macro_fn_names: RefCell::new(std::collections::HashSet::new()),
            errors: RefCell::new(Vec::new()),
        })
    }

    pub fn with_line_tables(
        context: &'ctx Context,
        mut interner: Interner,
        expr_types: Vec<HirType>,
        source_str: String,
        file_name: &str,
    ) -> Result<Self, String> {
        let module = context.create_module("glyim_out");
        let builder = context.create_builder();
        let option_sym = interner.intern("Option");
        let result_sym = interner.intern("Result");
        let debug_info =
            DebugInfoGen::new(&module, file_name, DWARFEmissionKind::LineTablesOnly).ok();
        Ok(Self {
            coverage_mode: CoverageMode::Off,
            context,
            module,
            builder,
            i64_type: context.i64_type(),
            i32_type: context.i32_type(),
            f64_type: context.f64_type(),
            interner,
            string_counter: RefCell::new(0),
            expr_types,
            mono_cache: RefCell::new(HashMap::new()),
            struct_types: RefCell::new(HashMap::new()),
            struct_field_indices: RefCell::new(HashMap::new()),
            enum_types: RefCell::new(HashMap::new()),
            enum_struct_types: RefCell::new(HashMap::new()),
            enum_variant_tags: RefCell::new(HashMap::new()),
            option_sym,
            result_sym,
            debug_info,
            source_str: Some(source_str),
            current_subprogram: None,
            no_std: false,
            extern_methods: std::collections::HashMap::new(),
            effect_analysis: None,
            jit_mode: false,
            library_mode: false,
            target_triple: None,
            macro_fn_names: RefCell::new(std::collections::HashSet::new()),
            errors: RefCell::new(Vec::new()),
        })
    }

    fn set_debug_location_for_span(&self, span: Span) {
        if let (Some(di), Some(src), Some(sp)) =
            (&self.debug_info, &self.source_str, &self.current_subprogram)
        {
            let line = crate::debug::DebugInfoGen::byte_offset_to_line(src, span.start);
            if let Ok(loc) = di.create_location(*sp, line, 0) {
                self.builder.set_current_debug_location(loc);
            }
        }
    }

    /// Report a non-fatal error during codegen.
    pub fn report_error(&self, msg: String) {
        self.errors.borrow_mut().push(msg);
    }

    #[tracing::instrument(skip_all)]
    pub fn generate(&mut self, hir: &Hir) -> Result<(), String> {
        // Guard: no unresolved type params should reach codegen
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    glyim_hir::passes::no_type_params::assert_no_type_params(
                        &f.body,
                        &self.interner,
                    );
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        glyim_hir::passes::no_type_params::assert_no_type_params(
                            &m.body,
                            &self.interner,
                        );
                    }
                }
                _ => {}
            }
        }
        tracing::debug!("[codegen generate] =======================");
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    tracing::debug!(
                        "[codegen generate] Fn: {} (type_params={:?})",
                        self.interner.resolve(f.name),
                        f.type_params
                    );
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        tracing::debug!(
                            "[codegen generate] Impl method: {}",
                            self.interner.resolve(m.name)
                        );
                    }
                }
                _ => {}
            }
        }
        tracing::debug!("[codegen generate] =======================");

        tracing::debug!("[codegen] generate() received {} items:", hir.items.len());
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    tracing::debug!(
                        "[codegen]   Fn: {} (type_params={:?})",
                        self.interner.resolve(f.name),
                        f.type_params
                    );
                }
                glyim_hir::item::HirItem::Struct(s) => {
                    tracing::debug!("[codegen]   Struct: {}", self.interner.resolve(s.name));
                }
                glyim_hir::item::HirItem::Enum(e) => {
                    tracing::debug!(
                        "[codegen]   Enum: {} (variants: {})",
                        self.interner.resolve(e.name),
                        e.variants.len()
                    );
                    for v in &e.variants {
                        tracing::debug!(
                            "[codegen]     variant: {} fields: {} tag: {}",
                            self.interner.resolve(v.name),
                            v.fields.len(),
                            v.tag
                        );
                        for f in &v.fields {
                            tracing::debug!(
                                "[codegen]       field: {} type: {:?}",
                                self.interner.resolve(f.name),
                                f.ty
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        if !self.library_mode { crate::runtime_shims::emit_runtime_shims(self.context, &self.module, self.jit_mode); }
        if !self.library_mode { crate::alloc::emit_alloc_shims(&self.module, self.no_std); }
        if !self.library_mode { crate::hash_shims::emit_hash_shims(self.context, &self.module, self.no_std); }

        // Pass 1 — register all types and extern declarations
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Struct(s) => {
                    types::codegen_struct_def(self, s);
                    // Also register under base name for generic structs (all fields are i64)
                    if !s.type_params.is_empty() {
                        let base_name = s.name;
                        if !self.struct_types.borrow().contains_key(&base_name) {
                            let fields: Vec<inkwell::types::BasicTypeEnum> =
                                s.fields.iter().map(|_| self.i64_type.into()).collect();
                            let st = self.context.struct_type(&fields, false);
                            self.struct_types.borrow_mut().insert(base_name, st);
                        }
                    }
                }
                glyim_hir::item::HirItem::Enum(e) => types::codegen_enum_def(self, e),
                glyim_hir::item::HirItem::Extern(ext) => {
                    for f in &ext.functions {
                        let name = self.interner.resolve(f.name);
                        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = f
                            .params
                            .iter()
                            .map(|pt| match pt {
                                glyim_hir::types::HirType::Int => self.i64_type.into(),
                                glyim_hir::types::HirType::Bool => self.i32_type.into(),
                                glyim_hir::types::HirType::RawPtr(_) => self
                                    .context
                                    .ptr_type(inkwell::AddressSpace::from(0u16))
                                    .into(),
                                _ => self.i64_type.into(),
                            })
                            .collect();
                        let ret_type = match &f.ret {
                            glyim_hir::types::HirType::Int => self.i64_type.into(),
                            glyim_hir::types::HirType::Bool => self.i32_type.into(),
                            glyim_hir::types::HirType::RawPtr(_) => self
                                .context
                                .ptr_type(inkwell::AddressSpace::from(0u16))
                                .into(),
                            _ => self.i64_type.into(),
                        };
                        // Only add if not already declared (avoids duplicates)
                        if self.module.get_function(name).is_none() {
                            let _fn_val = self.module.add_function(
                                name,
                                match ret_type {
                                    inkwell::types::BasicTypeEnum::IntType(t) => {
                                        t.fn_type(&param_types, false)
                                    }
                                    _ => self.i64_type.fn_type(&param_types, false),
                                },
                                None,
                            );
                        }
                        // Register in extern_methods for impl backing
                        self.extern_methods.insert(f.name, f.name);
                    }
                }
                _ => {}
            }
        }

        // Pass 1b — register specialized struct types from the HIR
        for item in &hir.items {
            if let glyim_hir::item::HirItem::Struct(s) = item {
                types::codegen_struct_def(self, s);
            }
        }

        // Pass 2 — forward-declare ALL functions before any body is compiled
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    function::declare_fn(self, f);
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        function::declare_fn(self, m);
                    }
                }
                _ => {}
            }
        }

        // Pass 2b debug: list all functions in module
        tracing::debug!("[codegen] Functions in module before Pass 3:");
        if let Some(func) = self.module.get_first_function() {
            let mut f = Some(func);
            while let Some(func) = f {
                let name = func.get_name().to_string_lossy();
                if true {
                    tracing::debug!("[codegen]   {}", name);
                }
                f = func.get_next_function();
            }
        }
        // Phase 6B: Coverage instrumentation
        let mut cov_counter = 0u32;
        let num_fns = hir.items.iter().filter(|i| matches!(i, glyim_hir::HirItem::Fn(_) | glyim_hir::HirItem::Impl(_))).count();
        if self.coverage_mode != CoverageMode::Off && num_fns > 0 {
            coverage::emit_coverage_globals(&self.module, num_fns, self.coverage_mode);
        }

        // Pass 3 — emit bodies (all forward declarations already present)
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    // Monomorphizer now guarantees full concretization – no skip needed.
                    if let Err(e) = function::codegen_fn(self, f) {
                        self.report_error(e);
                    }
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if let Err(e) = function::codegen_fn(self, m) {
                            self.report_error(e);
                        }
                    }
                }
                _ => {}
            }
        }

        self.emit_macro_debug_section();
        if let Some(ref di) = self.debug_info {
            di.finalize();
        }

        let errors = self.errors.borrow().clone();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }

        if self.module.get_function("main").is_none() && !self.library_mode {
            Err("no 'main' function".into())
        } else {
            if let Err(msg) = self.module.verify() {
                Err(format!(
                    "LLVM module verification failed: {}",
                    msg.to_string()
                ))
            } else {
                Ok(())
            }
        }
    }

    pub fn ir_string(&self) -> String {
        self.module.print_to_string().to_string()
    }

    pub fn get_module(&self) -> &Module<'ctx> {
        &self.module
    }

    pub fn write_object_file(&self, path: &std::path::Path) -> Result<(), String> {
        self.write_object_file_with_opt(path, inkwell::OptimizationLevel::None)
    }

    pub fn write_object_file_with_opt(
        &self,
        path: &std::path::Path,
        opt_level: inkwell::OptimizationLevel,
    ) -> Result<(), String> {
        use inkwell::targets::{
            CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
            TargetTriple,
        };
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
        let triple_obj = if let Some(ref t) = self.target_triple {
            TargetTriple::create(t)
        } else {
            TargetMachine::get_default_triple()
        };
        let target = Target::from_triple(&triple_obj)
            .map_err(|e| format!("unsupported target triple '{}': {}", triple_obj, e))?;
        let machine = target
            .create_target_machine(
                &triple_obj,
                "",
                "",
                opt_level,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or("target machine")?;
        machine
            .write_to_file(&self.module, FileType::Object, path)
            .map_err(|e| e.to_string())
    }

    fn create_c_string_global(
        &self,
        s: &str,
    ) -> Result<inkwell::values::PointerValue<'ctx>, String> {
        use inkwell::AddressSpace;
        let bytes = s.as_bytes();
        let i8_type = self.context.i8_type();
        let arr_type = i8_type.array_type((bytes.len() + 1) as u32);
        let name = {
            let mut c = self.string_counter.borrow_mut();
            let n = *c;
            *c += 1;
            format!("test.str.{}", n)
        };
        let global = self
            .module
            .add_global(arr_type, Some(AddressSpace::from(0u16)), &name);
        let mut elems: Vec<_> = bytes
            .iter()
            .map(|b| i8_type.const_int(*b as u64, false))
            .collect();
        elems.push(i8_type.const_int(0, false));
        let arr = unsafe { inkwell::values::ArrayValue::new_const_array(&arr_type, &elems) };
        global.set_initializer(&arr);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        let zero = self.context.i32_type().const_int(0, false);
        unsafe {
            self.builder.build_gep(
                arr_type,
                global.as_pointer_value(),
                &[zero, zero],
                "str_ptr",
            )
        }
        .map_err(|e| e.to_string())
    }

    fn call_printf(
        &self,
        fmt_ptr: inkwell::values::PointerValue<'ctx>,
        str_ptr: inkwell::values::PointerValue<'ctx>,
    ) -> Result<(), String> {
        let printf = self
            .module
            .get_function("printf")
            .ok_or_else(|| "printf not declared".to_string())?;
        self.builder
            .build_call(printf, &[fmt_ptr.into(), str_ptr.into()], "printf")
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn generate_for_tests(
        &mut self,
        hir: &Hir,
        test_names: &[String],
        should_panic: &std::collections::HashSet<String>,
    ) -> Result<(), String> {
        if !self.library_mode { crate::runtime_shims::emit_runtime_shims(self.context, &self.module, self.jit_mode); }
        if !self.library_mode { crate::alloc::emit_alloc_shims(&self.module, self.no_std); }
        if !self.library_mode { crate::hash_shims::emit_hash_shims(self.context, &self.module, self.no_std); }

        // Pass 1 — register all types and extern declarations
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Struct(s) => {
                    types::codegen_struct_def(self, s);
                    // Also register under base name for generic structs (all fields are i64)
                    if !s.type_params.is_empty() {
                        let base_name = s.name;
                        if !self.struct_types.borrow().contains_key(&base_name) {
                            let fields: Vec<inkwell::types::BasicTypeEnum> =
                                s.fields.iter().map(|_| self.i64_type.into()).collect();
                            let st = self.context.struct_type(&fields, false);
                            self.struct_types.borrow_mut().insert(base_name, st);
                        }
                    }
                }
                glyim_hir::item::HirItem::Enum(e) => types::codegen_enum_def(self, e),
                glyim_hir::item::HirItem::Extern(ext) => {
                    for f in &ext.functions {
                        let name = self.interner.resolve(f.name);
                        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = f
                            .params
                            .iter()
                            .map(|pt| match pt {
                                glyim_hir::types::HirType::Int => self.i64_type.into(),
                                glyim_hir::types::HirType::Bool => self.i32_type.into(),
                                glyim_hir::types::HirType::RawPtr(_) => self
                                    .context
                                    .ptr_type(inkwell::AddressSpace::from(0u16))
                                    .into(),
                                _ => self.i64_type.into(),
                            })
                            .collect();
                        let ret_type = match &f.ret {
                            glyim_hir::types::HirType::Int => self.i64_type.into(),
                            glyim_hir::types::HirType::Bool => self.i32_type.into(),
                            glyim_hir::types::HirType::RawPtr(_) => self
                                .context
                                .ptr_type(inkwell::AddressSpace::from(0u16))
                                .into(),
                            _ => self.i64_type.into(),
                        };
                        // Only add if not already declared (avoids duplicates)
                        if self.module.get_function(name).is_none() {
                            let _fn_val = self.module.add_function(
                                name,
                                match ret_type {
                                    inkwell::types::BasicTypeEnum::IntType(t) => {
                                        t.fn_type(&param_types, false)
                                    }
                                    _ => self.i64_type.fn_type(&param_types, false),
                                },
                                None,
                            );
                        }
                        // Register in extern_methods for impl backing
                        self.extern_methods.insert(f.name, f.name);
                    }
                }
                _ => {}
            }
        }

        // Pass 2 — forward-declare ALL test functions before any body is compiled
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    let name = self.interner.resolve(f.name);
                    if name != "main" {
                        function::declare_fn(self, f);
                    }
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        function::declare_fn(self, m);
                    }
                }
                _ => {}
            }
        }

        // Pass 3 — emit bodies (skip user main; test harness creates its own)
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    let name = self.interner.resolve(f.name);
                    if name == "main" {
                        continue;
                    }
                    if let Err(e) = function::codegen_fn(self, f) {
                        self.report_error(e);
                    }
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if let Err(e) = function::codegen_fn(self, m) {
                            self.report_error(e);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(ref di) = self.debug_info {
            di.finalize();
        }

        let errors = self.errors.borrow().clone();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }

        self.emit_test_harness(test_names, should_panic)?;

        if let Err(msg) = self.module.verify() {
            Err(format!(
                "LLVM module verification failed: {}",
                msg.to_string()
            ))
        } else {
            Ok(())
        }
    }

    fn emit_test_harness(
        &mut self,
        test_names: &[String],
        should_panic: &std::collections::HashSet<String>,
    ) -> Result<(), String> {
        use inkwell::IntPredicate;
        if test_names.is_empty() {
            return Err("no test functions to generate harness for".into());
        }
        let i32_type = self.i32_type;
        let i64_type = self.i64_type;
        let zero32 = i32_type.const_int(0, false);
        let zero64 = i64_type.const_int(0, false);
        let one32 = i32_type.const_int(1, false);
        let main_type = i32_type.fn_type(&[], false);
        let main_fn = self.module.add_function("main", main_type, None);
        let entry = self.context.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);
        let fmt_ptr = self.create_c_string_global("%s")?;
        let header = format!("running {} tests\n", test_names.len());
        let header_ptr = self.create_c_string_global(&header)?;
        self.call_printf(fmt_ptr, header_ptr)?;
        let any_failed = self
            .builder
            .build_alloca(i32_type, "any_failed")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(any_failed, zero32)
            .map_err(|e| e.to_string())?;
        for test_name in test_names {
            let msg = format!("test {} ... ", test_name);
            let msg_ptr = self.create_c_string_global(&msg)?;
            self.call_printf(fmt_ptr, msg_ptr)?;
            let fn_val = self
                .module
                .get_function(test_name)
                .ok_or_else(|| format!("test function '{}' not found", test_name))?;
            let call_result = self
                .builder
                .build_call(fn_val, &[], "test_result")
                .map_err(|e| e.to_string())?;
            let result_val = match call_result.try_as_basic_value() {
                inkwell::values::ValueKind::Basic(basic_val) => basic_val.into_int_value(),
                _ => return Err(format!("test function '{}' returned void", test_name)),
            };
            let is_should_panic = should_panic.contains(test_name);
            let is_fail = if is_should_panic {
                self.builder
                    .build_int_compare(IntPredicate::EQ, result_val, zero64, "is_fail")
                    .map_err(|e| e.to_string())?
            } else {
                self.builder
                    .build_int_compare(IntPredicate::NE, result_val, zero64, "is_fail")
                    .map_err(|e| e.to_string())?
            };
            let pass_bb = self.context.append_basic_block(main_fn, "test_pass");
            let fail_bb = self.context.append_basic_block(main_fn, "test_fail");
            let next_bb = self.context.append_basic_block(main_fn, "test_next");
            self.builder
                .build_conditional_branch(is_fail, fail_bb, pass_bb)
                .map_err(|e| e.to_string())?;
            self.builder.position_at_end(fail_bb);
            let fail_msg_ptr = self.create_c_string_global("FAILED\n")?;
            self.call_printf(fmt_ptr, fail_msg_ptr)?;
            self.builder
                .build_store(any_failed, one32)
                .map_err(|e| e.to_string())?;
            self.builder
                .build_unconditional_branch(next_bb)
                .map_err(|e| e.to_string())?;
            self.builder.position_at_end(pass_bb);
            let ok_msg_ptr = self.create_c_string_global("ok\n")?;
            self.call_printf(fmt_ptr, ok_msg_ptr)?;
            self.builder
                .build_unconditional_branch(next_bb)
                .map_err(|e| e.to_string())?;
            self.builder.position_at_end(next_bb);
        }
        let summary = format!(
            "\ntest result: ok. {} passed; 0 failed; 0 ignored\n",
            test_names.len()
        );
        let summary_ptr = self.create_c_string_global(&summary)?;
        self.call_printf(fmt_ptr, summary_ptr)?;
        let failed_val = self
            .builder
            .build_load(i32_type, any_failed, "failed_val")
            .map_err(|e| e.to_string())?
            .into_int_value();
        self.builder
            .build_return(Some(&failed_val))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn with_no_std(mut self) -> Self {
        self.no_std = true;
        self
    }

    pub fn with_jit_mode(mut self) -> Self {
        self.jit_mode = true;
        self
    }
    /// Compile in library mode: no runtime shims and no main requirement.
    pub fn with_library_mode(mut self) -> Self {
        self.library_mode = true;
        self
    }

    /// Attach effect analysis results for LLVM attribute annotation.
    pub fn with_effects(mut self, effects: glyim_hir::effects::EffectSet) -> Self {
        self.effect_analysis = Some(effects);
        self
    }

    /// Given a HirType, return the LLVM StructType if it represents a struct.
    /// Handles Named and Generic (via mangling) types.
    pub(crate) fn resolve_struct_type(
        &self,
        ty: &HirType,
    ) -> Option<inkwell::types::StructType<'ctx>> {
        let struct_sym = match ty {
            HirType::Named(sym) => *sym,
            HirType::Generic(sym, args) => {
                let base_str = self.interner.resolve(*sym).to_string();
                let args_str = args
                    .iter()
                    .map(|a| glyim_hir::monomorphize::type_to_short_string(a, &self.interner))
                    .collect::<Vec<_>>()
                    .join("_");
                let mangled_str = format!("{}__{}", base_str, args_str);
                tracing::debug!(
                    "[resolve_struct_type] Generic: base_str={} args_str={} mangled={}",
                    base_str,
                    args_str,
                    mangled_str
                );
                if let Some(_found) = self.interner.resolve_symbol(&mangled_str) {
                    tracing::debug!("[resolve_struct_type] FOUND in interner");
                } else {
                    tracing::debug!(
                        "[resolve_struct_type] NOT FOUND in interner ({} entries)",
                        self.interner.len()
                    );
                }
                self.interner.resolve_symbol(&mangled_str)?
            }
            HirType::RawPtr(inner) => return self.resolve_struct_type(inner),
            _ => return None,
        };
        self.struct_types.borrow().get(&struct_sym).copied()
    }

    pub fn with_target(mut self, triple: &str) -> Self {
        self.target_triple = Some(triple.to_string());
        self
    }

    /// Set the target triple without consuming self.
    pub fn set_target(&mut self, triple: &str) {
        self.target_triple = Some(triple.to_string());
    }

    fn emit_macro_debug_section(&self) {
        let names = self.macro_fn_names.borrow();
        if names.is_empty() {
            return;
        }
        let metadata = names
            .iter()
            .map(|sym| self.interner.resolve(*sym).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let metadata = format!("[{}]", metadata);
        let bytes = metadata.as_bytes();
        let i8_type = self.context.i8_type();
        let arr_type = i8_type.array_type(bytes.len() as u32);
        let global = self.module.add_global(
            arr_type,
            Some(inkwell::AddressSpace::from(0u16)),
            "_glyim_macro",
        );
        let elems: Vec<_> = bytes
            .iter()
            .map(|b| i8_type.const_int(*b as u64, false))
            .collect();
        let arr = unsafe { inkwell::values::ArrayValue::new_const_array(&arr_type, &elems) };
        global.set_initializer(&arr);
        global.set_constant(true);
        global.set_section(Some(".glyim.macro"));
    }
}
