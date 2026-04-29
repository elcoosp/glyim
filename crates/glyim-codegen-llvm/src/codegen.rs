use glyim_hir::{Hir, HirBinOp, HirExpr, HirStmt, HirUnOp};

use glyim_interner::{Interner, Symbol};
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicTypeEnum, IntType};
use inkwell::values::{
    BasicValue, BasicValueEnum, FunctionValue, IntValue, PointerValue, StructValue,
};

use inkwell::{AddressSpace, IntPredicate};
use std::cell::RefCell;
use std::collections::HashMap;

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    i64_type: IntType<'ctx>,
    i32_type: IntType<'ctx>,
    f64_type: inkwell::types::FloatType<'ctx>,
    interner: Interner,
    string_counter: RefCell<u32>,
    struct_types: RefCell<HashMap<Symbol, inkwell::types::StructType<'ctx>>>,
    enum_types: RefCell<
        HashMap<
            Symbol,
            (
                inkwell::types::IntType<'ctx>,
                inkwell::types::ArrayType<'ctx>,
            ),
        >,
    >,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, interner: Interner) -> Self {
        let module = context.create_module("glyim_out");
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
            i64_type: context.i64_type(),
            i32_type: context.i32_type(),
            f64_type: context.f64_type(),
            interner,
            string_counter: RefCell::new(0),
            struct_types: RefCell::new(HashMap::new()),
            enum_types: RefCell::new(HashMap::new()),
        }
    }

    pub fn generate(&mut self, hir: &Hir) -> Result<(), String> {
        crate::runtime_shims::emit_runtime_shims(self.context, &self.module);
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => self.codegen_fn(f)?,
                glyim_hir::item::HirItem::Struct(s) => self.codegen_struct_def(s),
                glyim_hir::item::HirItem::Enum(e) => self.codegen_enum_def(e),
                glyim_hir::item::HirItem::Extern(_) => {}
            }
        }
        if self.module.get_function("main").is_none() {
            Err("no 'main' function".into())
        } else {
            Ok(())
        }
    }

    fn codegen_struct_def(&self, def: &glyim_hir::item::StructDef) {
        let _name = self.interner.resolve(def.name);
        let field_types: Vec<BasicTypeEnum<'ctx>> = def
            .fields
            .iter()
            .map(|_| BasicTypeEnum::IntType(self.i64_type))
            .collect();
        let struct_type = self.context.struct_type(&field_types, false);
        self.struct_types.borrow_mut().insert(def.name, struct_type);
    }

    fn codegen_enum_def(&self, def: &glyim_hir::item::EnumDef) {
        // Compute max payload size (stub: all Int fields)
        let max_fields = def
            .variants
            .iter()
            .map(|v| v.fields.len())
            .max()
            .unwrap_or(0);
        let payload_bytes = (max_fields as u32) * 8; // each i64 is 8 bytes
        let tag_type = self.i32_type;
        let payload_type = self.context.i8_type().array_type(payload_bytes);
        self.enum_types
            .borrow_mut()
            .insert(def.name, (tag_type, payload_type));
    }

    fn codegen_fn(&mut self, f: &glyim_hir::HirFn) -> Result<(), String> {
        let name = self.interner.resolve(f.name);
        let is_main = name == "main";
        let ret_type = if is_main {
            self.i32_type
        } else {
            self.i64_type
        };
        let fn_type = ret_type.fn_type(&[], false);
        let fn_value = self.module.add_function(name, fn_type, None);
        let entry = self.context.append_basic_block(fn_value, "entry");
        self.builder.position_at_end(entry);

        let mut vars: HashMap<Symbol, PointerValue<'ctx>> = HashMap::new();
        for (i, param_sym) in f.params.iter().enumerate() {
            let param_val = fn_value.get_nth_param(i as u32).ok_or("missing param")?;
            let alloca = self
                .builder
                .build_alloca(self.i64_type, self.interner.resolve(*param_sym))
                .map_err(|e| e.to_string())?;
            self.builder
                .build_store(alloca, param_val)
                .map_err(|e| e.to_string())?;
            vars.insert(*param_sym, alloca);
        }

        let body_val = self
            .codegen_block(&f.body, &mut vars, fn_value)
            .ok_or("codegen fail")?;
        let ret_val = if is_main {
            self.builder
                .build_int_truncate(body_val, self.i32_type, "trunc")
                .map_err(|e| e.to_string())?
        } else {
            body_val
        };
        self.builder
            .build_return(Some(&ret_val))
            .map_err(|e| e.to_string())?;
        if !fn_value.verify(true) {
            return Err("verification fail".into());
        }
        Ok(())
    }

    fn codegen_block(
        &self,
        expr: &HirExpr,
        vars: &mut HashMap<Symbol, PointerValue<'ctx>>,
        fn_value: FunctionValue<'ctx>,
    ) -> Option<IntValue<'ctx>> {
        match expr {
            HirExpr::Block(stmts) => {
                let mut last = Some(self.i64_type.const_int(0, false));
                for stmt in stmts {
                    if let Some(v) = self.codegen_stmt(stmt, vars, fn_value) {
                        last = Some(v);
                    }
                }
                last
            }
            other => self.codegen_expr(other, vars, fn_value),
        }
    }

    fn codegen_stmt(
        &self,
        stmt: &HirStmt,
        vars: &mut HashMap<Symbol, PointerValue<'ctx>>,
        fn_value: FunctionValue<'ctx>,
    ) -> Option<IntValue<'ctx>> {
        match stmt {
            HirStmt::Let {
                name,
                mutable: _,
                value,
            } => {
                let val = self.codegen_expr(value, vars, fn_value)?;
                let alloca = self
                    .builder
                    .build_alloca(self.i64_type, self.interner.resolve(*name))
                    .ok()?;
                self.builder.build_store(alloca, val).ok()?;
                vars.insert(*name, alloca);
                None
            }
            HirStmt::Assign { target, value } => {
                let new_val = self.codegen_expr(value, vars, fn_value)?;
                if let Some(ptr) = vars.get(target).copied() {
                    self.builder.build_store(ptr, new_val).ok()?;
                }
                Some(new_val)
            }
            HirStmt::Expr(e) => self.codegen_expr(e, vars, fn_value),
        }
    }

    fn codegen_expr(
        &self,
        expr: &HirExpr,
        vars: &mut HashMap<Symbol, PointerValue<'ctx>>,
        fn_value: FunctionValue<'ctx>,
    ) -> Option<IntValue<'ctx>> {
        match expr {
            HirExpr::IntLit(n) => Some(self.i64_type.const_int(*n as u64, true)),
            HirExpr::Ident(sym) => {
                let ptr = vars.get(sym)?;
                self.builder
                    .build_load(self.i64_type, *ptr, self.interner.resolve(*sym))
                    .ok()
                    .map(|v| v.into_int_value())
            }
            HirExpr::Binary { op, lhs, rhs } => {
                let l = self.codegen_expr(lhs, vars, fn_value)?;
                let r = self.codegen_expr(rhs, vars, fn_value)?;
                codegen_binop(&self.builder, self.i64_type, op.clone(), l, r)
            }
            HirExpr::Unary { op, operand } => {
                let val = self.codegen_expr(operand, vars, fn_value)?;
                match op {
                    HirUnOp::Neg => {
                        let zero = self.i64_type.const_int(0, false);
                        self.builder.build_int_sub(zero, val, "neg").ok()
                    }
                    HirUnOp::Not => self.builder.build_not(val, "not").ok(),
                }
            }
            HirExpr::Block(stmts) => {
                let mut last = Some(self.i64_type.const_int(0, false));
                for stmt in stmts {
                    if let Some(v) = self.codegen_stmt(stmt, vars, fn_value) {
                        last = Some(v);
                    }
                }
                last
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.codegen_expr(condition, vars, fn_value)?;
                let cond_bool = self
                    .builder
                    .build_int_compare(
                        IntPredicate::NE,
                        cond_val,
                        self.i64_type.const_int(0, false),
                        "if_cond",
                    )
                    .ok()?;
                let then_bb = self.context.append_basic_block(fn_value, "then");
                let else_bb = self.context.append_basic_block(fn_value, "else");
                let merge_bb = self.context.append_basic_block(fn_value, "merge");
                self.builder
                    .build_conditional_branch(cond_bool, then_bb, else_bb)
                    .ok()?;
                self.builder.position_at_end(then_bb);
                let then_val = self.codegen_block(then_branch, vars, fn_value)?;
                self.builder.build_unconditional_branch(merge_bb).ok()?;
                let then_bb_final = self.builder.get_insert_block().unwrap();
                self.builder.position_at_end(else_bb);
                let else_val = match else_branch {
                    Some(e) => self.codegen_block(e, vars, fn_value)?,
                    None => self.i64_type.const_int(0, false),
                };
                self.builder.build_unconditional_branch(merge_bb).ok()?;
                let else_bb_final = self.builder.get_insert_block().unwrap();
                self.builder.position_at_end(merge_bb);
                let phi = self.builder.build_phi(self.i64_type, "if_result").ok()?;
                phi.add_incoming(&[
                    (&then_val as &dyn BasicValue, then_bb_final),
                    (&else_val as &dyn BasicValue, else_bb_final),
                ]);
                Some(phi.as_basic_value().into_int_value())
            }
            HirExpr::StrLit(s) => self.codegen_string_literal(s),
            HirExpr::Println(arg) => self.codegen_println(arg, vars, fn_value),
            HirExpr::Assert { condition, message } => {
                self.codegen_assert(condition, message, vars, fn_value)
            }
            // v0.3.0: stub implementations for new expression variants
            HirExpr::FloatLit(_)
            | HirExpr::BoolLit(_)
            | HirExpr::UnitLit
            | HirExpr::As { .. }
            | HirExpr::Match { .. }
            | HirExpr::EnumVariant { .. } => Some(self.i64_type.const_int(0, false)),
            HirExpr::StructLit { .. } => {
                // Stub: return 0 until proper struct layout codegen is implemented
                Some(self.i64_type.const_int(0, false))
            }
            HirExpr::FieldAccess { object, field: _ } => {
                let obj_val = self.codegen_expr(object, vars, fn_value)?;
                #[allow(unused_variables)]
                let _ptr = self
                    .builder
                    .build_int_to_ptr(
                        obj_val,
                        self.context.ptr_type(AddressSpace::from(0u16)),
                        "to_ptr",
                    )
                    .ok()?;
                // Find field index: need the struct type. We don't know the struct type at this point.
                // For now, hardcode field 0; proper implementation requires type info.
                // We'll look up the struct from the object? Not possible without type info.
                // Stub: return 0.
                Some(self.i64_type.const_int(0, false))
            }
        }
    }

    fn codegen_string_literal(&self, s: &str) -> Option<IntValue<'ctx>> {
        let bytes = s.trim_matches('"').as_bytes();
        let ty = self.context.i8_type().array_type(bytes.len() as u32);
        let mut counter = self.string_counter.borrow_mut();
        let name = format!("str.{}", *counter);
        *counter += 1;
        drop(counter);
        let global = self
            .module
            .add_global(ty, Some(AddressSpace::from(0u16)), &name);
        let elems: Vec<_> = bytes
            .iter()
            .map(|b| self.context.i8_type().const_int(*b as u64, false))
            .collect();
        let arr = unsafe { inkwell::values::ArrayValue::new_const_array(&ty, &elems) };
        global.set_initializer(&arr);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        let zero32 = self.context.i32_type().const_int(0, false);
        let ptr = unsafe {
            self.builder
                .build_gep(ty, global.as_pointer_value(), &[zero32, zero32], "str_ptr")
                .ok()?
        };
        self.builder
            .build_ptr_to_int(ptr, self.i64_type, "str_as_int")
            .ok()
    }

    fn codegen_println(
        &self,
        arg: &HirExpr,
        vars: &mut HashMap<Symbol, PointerValue<'ctx>>,
        fn_value: FunctionValue<'ctx>,
    ) -> Option<IntValue<'ctx>> {
        let val = self.codegen_expr(arg, vars, fn_value)?;
        if matches!(arg, HirExpr::StrLit(_)) {
            let fat = self.build_str_fat_ptr(arg)?;
            let shim = self.module.get_function("glyim_println_str").unwrap();
            self.builder
                .build_call(shim, &[fat.into()], "println")
                .ok()?;
        } else {
            let shim = self.module.get_function("glyim_println_int").unwrap();
            self.builder
                .build_call(shim, &[val.into()], "println")
                .ok()?;
        }
        Some(self.i64_type.const_int(0, false))
    }

    fn codegen_assert(
        &self,
        condition: &HirExpr,
        message: &Option<Box<HirExpr>>,
        vars: &mut HashMap<Symbol, PointerValue<'ctx>>,
        fn_value: FunctionValue<'ctx>,
    ) -> Option<IntValue<'ctx>> {
        let cond = self.codegen_expr(condition, vars, fn_value)?;
        let is_true = self
            .builder
            .build_int_compare(
                IntPredicate::NE,
                cond,
                self.i64_type.const_int(0, false),
                "assert_cond",
            )
            .ok()?;
        let pass_bb = self.context.append_basic_block(fn_value, "assert_pass");
        let fail_bb = self.context.append_basic_block(fn_value, "assert_fail");
        self.builder
            .build_conditional_branch(is_true, pass_bb, fail_bb)
            .ok()?;

        self.builder.position_at_end(fail_bb);
        let shim = self.module.get_function("glyim_assert_fail").unwrap();
        let null_ptr = self.context.ptr_type(AddressSpace::from(0u16)).const_null();
        let zero = self.i64_type.const_int(0, false);
        let (p, l) = match message {
            Some(m) if matches!(m.as_ref(), HirExpr::StrLit(_)) => {
                let fat = self.build_str_fat_ptr(m.as_ref())?;
                let ptr = self.builder.build_extract_value(fat, 0, "msg_p").ok()?;
                let len = self.builder.build_extract_value(fat, 1, "msg_l").ok()?;
                (ptr, len)
            }
            _ => (
                BasicValueEnum::PointerValue(null_ptr),
                BasicValueEnum::IntValue(zero),
            ),
        };
        self.builder
            .build_call(shim, &[p.into(), l.into()], "assert_fail")
            .ok()?;
        self.builder.build_unreachable().ok()?;

        self.builder.position_at_end(pass_bb);
        Some(self.i64_type.const_int(0, false))
    }

    fn build_str_fat_ptr(&self, arg: &HirExpr) -> Option<StructValue<'ctx>> {
        let s = match arg {
            HirExpr::StrLit(s) => s.clone(),
            _ => return None,
        };
        let bytes = s.trim_matches('"').as_bytes();
        let ty = self.context.i8_type().array_type(bytes.len() as u32);
        let mut counter = self.string_counter.borrow_mut();
        let name = format!("str.{}", *counter);
        *counter += 1;
        drop(counter);
        let global = self
            .module
            .add_global(ty, Some(AddressSpace::from(0u16)), &name);
        let elems: Vec<_> = bytes
            .iter()
            .map(|b| self.context.i8_type().const_int(*b as u64, false))
            .collect();
        let arr = unsafe { inkwell::values::ArrayValue::new_const_array(&ty, &elems) };
        global.set_initializer(&arr);
        global.set_constant(true);
        global.set_linkage(inkwell::module::Linkage::Private);
        let zero32 = self.context.i32_type().const_int(0, false);
        let ptr = unsafe {
            self.builder
                .build_gep(ty, global.as_pointer_value(), &[zero32, zero32], "ptr")
                .ok()?
        };
        let len = self.i64_type.const_int(bytes.len() as u64, false);
        let fat_type = self.context.struct_type(
            &[
                BasicTypeEnum::PointerType(self.context.ptr_type(AddressSpace::from(0u16))),
                BasicTypeEnum::IntType(self.i64_type),
            ],
            false,
        );
        Some(fat_type.const_named_struct(&[
            BasicValueEnum::PointerValue(ptr),
            BasicValueEnum::IntValue(len),
        ]))
    }

    pub fn ir_string(&self) -> String {
        self.module.print_to_string().to_string()
    }
    pub fn module(&self) -> &Module<'ctx> {
        &self.module
    }
    pub fn write_object_file(&self, path: &std::path::Path) -> Result<(), String> {
        use inkwell::targets::{
            CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
        };
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let machine = target
            .create_target_machine(
                &triple,
                "",
                "",
                inkwell::OptimizationLevel::None,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("target machine")?;
        machine
            .write_to_file(&self.module, FileType::Object, path)
            .map_err(|e| e.to_string())
    }
}

fn codegen_binop<'ctx>(
    builder: &Builder<'ctx>,
    i64_type: IntType<'ctx>,
    op: HirBinOp,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
) -> Option<IntValue<'ctx>> {
    match op {
        HirBinOp::Add => builder.build_int_add(l, r, "add").ok(),
        HirBinOp::Sub => builder.build_int_sub(l, r, "sub").ok(),
        HirBinOp::Mul => builder.build_int_mul(l, r, "mul").ok(),
        HirBinOp::Div => builder.build_int_signed_div(l, r, "div").ok(),
        HirBinOp::Mod => builder.build_int_signed_rem(l, r, "rem").ok(),
        HirBinOp::Eq => cmp_extend(builder, i64_type, IntPredicate::EQ, l, r),
        HirBinOp::Neq => cmp_extend(builder, i64_type, IntPredicate::NE, l, r),
        HirBinOp::Lt => cmp_extend(builder, i64_type, IntPredicate::SLT, l, r),
        HirBinOp::Gt => cmp_extend(builder, i64_type, IntPredicate::SGT, l, r),
        HirBinOp::Lte => cmp_extend(builder, i64_type, IntPredicate::SLE, l, r),
        HirBinOp::Gte => cmp_extend(builder, i64_type, IntPredicate::SGE, l, r),
        HirBinOp::And => builder.build_and(l, r, "and").ok(),
        HirBinOp::Or => builder.build_or(l, r, "or").ok(),
    }
}

fn cmp_extend<'ctx>(
    builder: &Builder<'ctx>,
    i64_type: IntType<'ctx>,
    pred: IntPredicate,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
) -> Option<IntValue<'ctx>> {
    let c = builder.build_int_compare(pred, l, r, "cmp").ok()?;
    builder.build_int_z_extend(c, i64_type, "zext").ok()
}

pub fn compile_to_ir(source: &str) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = Context::create();
    let mut cg = Codegen::new(&ctx, interner);
    cg.generate(&hir)?;
    Ok(cg.ir_string())
}
