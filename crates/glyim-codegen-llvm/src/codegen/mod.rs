pub(crate) mod ctx;
mod expr;
mod function;
mod ops;
mod stmt;
mod string;
mod types;

use glyim_hir::{Hir, HirType};
use glyim_interner::{Interner, Symbol};
use inkwell::context::Context;
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
    pub(crate) mono_cache: RefCell<HashMap<(Symbol, Vec<HirType>), inkwell::values::FunctionValue<'ctx>>>,
    pub(crate) struct_types: RefCell<HashMap<Symbol, inkwell::types::StructType<'ctx>>>,
    pub(crate) struct_field_indices: RefCell<HashMap<(Symbol, Symbol), usize>>,
    pub(crate) enum_types: RefCell<HashMap<Symbol, (IntType<'ctx>, inkwell::types::ArrayType<'ctx>)>>,
    pub(crate) enum_struct_types: RefCell<HashMap<Symbol, inkwell::types::StructType<'ctx>>>,
    pub(crate) enum_variant_tags: RefCell<HashMap<(Symbol, Symbol), u32>>,
    pub(crate) option_sym: Symbol,
    pub(crate) result_sym: Symbol,
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
        }
    }

    pub fn generate(&mut self, hir: &Hir) -> Result<(), String> {
        crate::runtime_shims::emit_runtime_shims(self.context, &self.module);
        types::register_builtin_enums(self);
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => function::codegen_fn(self, f)?,
                glyim_hir::item::HirItem::Struct(s) => types::codegen_struct_def(self, s),
                glyim_hir::item::HirItem::Enum(e) => types::codegen_enum_def(self, e),
                glyim_hir::item::HirItem::Extern(_) => {}
                glyim_hir::item::HirItem::Impl(_) => {}
            }
        }
        if self.module.get_function("main").is_none() {
            Err("no 'main' function".into())
        } else {
            Ok(())
        }
    }

    pub fn ir_string(&self) -> String { self.module.print_to_string().to_string() }
    pub fn get_module(&self) -> &Module<'ctx> { &self.module }
    pub fn write_object_file(&self, path: &std::path::Path) -> Result<(), String> {
        use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let machine = target.create_target_machine(
            &triple, "", "", inkwell::OptimizationLevel::None, RelocMode::PIC, CodeModel::Default)
            .ok_or("target machine")?;
        machine.write_to_file(&self.module, FileType::Object, path).map_err(|e| e.to_string())
    }
}

pub use self::ctx::FunctionContext;
