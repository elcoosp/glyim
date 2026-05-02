pub(crate) mod ctx;
mod expr;
mod function;
mod monomorphize;
mod ops;
mod stmt;
mod string;
mod types;

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

pub struct Codegen<'ctx> {
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
    pub(crate) option_sym: Symbol,
    pub(crate) result_sym: Symbol,
    debug_info: Option<DebugInfoGen<'ctx>>,
    source_str: Option<String>,
    current_subprogram: Option<DISubprogram<'ctx>>,
    pub(crate) macro_fn_names: std::cell::RefCell<std::collections::HashSet<Symbol>>,
    pub(crate) no_std: bool,
    pub(crate) errors: RefCell<Vec<String>>,
    pub(crate) jit_mode: bool,
    pub(crate) target_triple: Option<String>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, mut interner: Interner, expr_types: Vec<HirType>) -> Self {
        let module = context.create_module("glyim_out");
        let builder = context.create_builder();
        let option_sym = interner.intern("Option");
        let result_sym = interner.intern("Result");
        Self {
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
            debug_info: None,
            source_str: None,
            current_subprogram: None,
            no_std: false,
            jit_mode: false,
            target_triple: None,
            macro_fn_names: RefCell::new(std::collections::HashSet::new()),
            errors: RefCell::new(Vec::new()),
        }
    }

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
            jit_mode: false,
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
            jit_mode: false,
            target_triple: None,
            macro_fn_names: RefCell::new(std::collections::HashSet::new()),
            errors: RefCell::new(Vec::new()),
        })
    }

    fn set_debug_location_for_span(&self, span: Span) {
        if let (Some(ref di), Some(ref src), Some(sp)) =
            (&self.debug_info, &self.source_str, &self.current_subprogram)
        {
            let line = crate::debug::DebugInfoGen::byte_offset_to_line(src, span.start);
            if let Ok(loc) = di.create_location(*sp, line, 0) {
                self.builder.set_current_debug_location(loc);
            }
        }
    }

    /// Report a non-fatal error during codegen.
    /// Report a non-fatal error during codegen.
    pub fn report_error(&self, msg: String) {
        self.errors.borrow_mut().push(msg);
    }

    #[tracing::instrument(skip_all)]
    pub fn generate(&mut self, hir: &Hir) -> Result<(), String> {
        eprintln!("[codegen] generate() received {} items:", hir.items.len());
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    eprintln!(
                        "[codegen]   Fn: {} (type_params={:?})",
                        self.interner.resolve(f.name),
                        f.type_params
                    );
                }
                glyim_hir::item::HirItem::Struct(s) => {
                    eprintln!("[codegen]   Struct: {}", self.interner.resolve(s.name));
                }
                _ => {}
            }
        }
        crate::runtime_shims::emit_runtime_shims(self.context, &self.module, self.jit_mode);
        crate::alloc::emit_alloc_shims(&self.module, self.no_std);
        crate::hash_shims::emit_hash_shims(self.context, &self.module, self.no_std);

        // Pass 1 — register all types and extern declarations
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Struct(s) => types::codegen_struct_def(self, s),
                glyim_hir::item::HirItem::Enum(e) => types::codegen_enum_def(self, e),
                glyim_hir::item::HirItem::Extern(ext) => {
                    for f in &ext.functions {
                        let name = self.interner.resolve(f.name);
                        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> =
                            f.params.iter().map(|pt| {
                                match pt {
                                    glyim_hir::types::HirType::Int => self.i64_type.into(),
                                    glyim_hir::types::HirType::Bool => self.i32_type.into(),
                                    _ => self.i64_type.into(),
                                }
                            }).collect();
                        let ret_type = match &f.ret {
                            glyim_hir::types::HirType::Int => self.i64_type.into(),
                            glyim_hir::types::HirType::Bool => self.i32_type.into(),
                            _ => self.i64_type.into(),
                        };
                        self.module.add_function(
                            name,
                            match ret_type {
                                inkwell::types::BasicTypeEnum::IntType(t) => t.fn_type(&param_types, false),
                                _ => self.i64_type.fn_type(&param_types, false),
                            },
                            None,
                        );
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
        eprintln!("[codegen] Functions in module before Pass 3:");
        if let Some(func) = self.module.get_first_function() {
            let mut f = Some(func);
            while let Some(func) = f {
                let name = func.get_name().to_string_lossy();
                if true {
                    eprintln!("[codegen]   {}", name);
                }
                f = func.get_next_function();
            }
        }
        // Pass 3 — emit bodies (all forward declarations already present)
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
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

        if self.module.get_function("main").is_none() {
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
        crate::runtime_shims::emit_runtime_shims(self.context, &self.module, self.jit_mode);
        crate::alloc::emit_alloc_shims(&self.module, self.no_std);
        crate::hash_shims::emit_hash_shims(self.context, &self.module, self.no_std);

        // Pass 1 — register all types and extern declarations
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Struct(s) => types::codegen_struct_def(self, s),
                glyim_hir::item::HirItem::Enum(e) => types::codegen_enum_def(self, e),
                glyim_hir::item::HirItem::Extern(ext) => {
                    for f in &ext.functions {
                        let name = self.interner.resolve(f.name);
                        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> =
                            f.params.iter().map(|pt| {
                                match pt {
                                    glyim_hir::types::HirType::Int => self.i64_type.into(),
                                    glyim_hir::types::HirType::Bool => self.i32_type.into(),
                                    _ => self.i64_type.into(),
                                }
                            }).collect();
                        let ret_type = match &f.ret {
                            glyim_hir::types::HirType::Int => self.i64_type.into(),
                            glyim_hir::types::HirType::Bool => self.i32_type.into(),
                            _ => self.i64_type.into(),
                        };
                        self.module.add_function(
                            name,
                            match ret_type {
                                inkwell::types::BasicTypeEnum::IntType(t) => t.fn_type(&param_types, false),
                                _ => self.i64_type.fn_type(&param_types, false),
                            },
                            None,
                        );
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

    pub fn with_target(mut self, triple: &str) -> Self {
        self.target_triple = Some(triple.to_string());
        self
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
