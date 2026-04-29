use glyim_hir::{Hir, HirBinOp, HirExpr, HirPattern, HirStmt, HirUnOp};
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
    #[allow(dead_code)]
    f64_type: inkwell::types::FloatType<'ctx>,
    interner: Interner,
    string_counter: RefCell<u32>,
    struct_types: RefCell<HashMap<Symbol, inkwell::types::StructType<'ctx>>>,
    struct_field_indices: RefCell<HashMap<(Symbol, Symbol), usize>>,
    enum_types: RefCell<HashMap<Symbol, (IntType<'ctx>, inkwell::types::ArrayType<'ctx>)>>,
    enum_struct_types: RefCell<HashMap<Symbol, inkwell::types::StructType<'ctx>>>,
    enum_variant_tags: RefCell<HashMap<(Symbol, Symbol), u32>>,
    option_sym: Symbol,
    result_sym: Symbol,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, mut interner: Interner) -> Self {
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
        self.register_builtin_enums();
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

    fn register_builtin_enums(&mut self) {
        let tag_type = self.i32_type;
        let payload_type = self.context.i8_type().array_type(8); // one i64
        let enum_struct_type = self.context.struct_type(
            &[
                BasicTypeEnum::IntType(tag_type),
                BasicTypeEnum::ArrayType(payload_type),
            ],
            false,
        );
        let option_name = self.interner.intern("Option");
        self.enum_types
            .borrow_mut()
            .insert(option_name, (tag_type, payload_type));
        self.enum_struct_types
            .borrow_mut()
            .insert(option_name, enum_struct_type);
        let mut tag_map = self.enum_variant_tags.borrow_mut();
        tag_map.insert((option_name, self.interner.intern("None")), 0);
        tag_map.insert((option_name, self.interner.intern("Some")), 1);
        let result_name = self.interner.intern("Result");
        self.enum_types
            .borrow_mut()
            .insert(result_name, (tag_type, payload_type));
        self.enum_struct_types
            .borrow_mut()
            .insert(result_name, enum_struct_type);
        tag_map.insert((result_name, self.interner.intern("Ok")), 0);
        tag_map.insert((result_name, self.interner.intern("Err")), 1);
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
        let mut index_map = self.struct_field_indices.borrow_mut();
        for (i, field) in def.fields.iter().enumerate() {
            index_map.insert((def.name, field.name), i);
        }
    }

    fn codegen_enum_def(&self, def: &glyim_hir::item::EnumDef) {
        let max_fields = def
            .variants
            .iter()
            .map(|v| v.fields.len())
            .max()
            .unwrap_or(0);
        let payload_bytes = (max_fields as u32) * 8;
        let tag_type = self.i32_type;
        let payload_type = self.context.i8_type().array_type(payload_bytes);
        let enum_struct_type = self.context.struct_type(
            &[
                BasicTypeEnum::IntType(tag_type),
                BasicTypeEnum::ArrayType(payload_type),
            ],
            false,
        );
        self.enum_types
            .borrow_mut()
            .insert(def.name, (tag_type, payload_type));
        self.enum_struct_types
            .borrow_mut()
            .insert(def.name, enum_struct_type);
        let mut tag_map = self.enum_variant_tags.borrow_mut();
        for (i, variant) in def.variants.iter().enumerate() {
            tag_map.insert((def.name, variant.name), i as u32);
        }
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
            HirExpr::Call { callee, args } => {
                let fn_name = self.interner.resolve(*callee);
                if let Some(fn_val) = self.module.get_function(fn_name) {
                    let call_args: Vec<inkwell::values::BasicMetadataValueEnum> = args
                        .iter()
                        .filter_map(|a| self.codegen_expr(a, vars, fn_value))
                        .map(|v| v.into())
                        .collect();
                    let result = self.builder.build_call(fn_val, &call_args, "call").ok()?;
                    match result.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(basic_val) => {
                            Some(basic_val.into_int_value())
                        }
                        _ => Some(self.i64_type.const_int(0, false)),
                    }
                } else {
                    Some(self.i64_type.const_int(0, false))
                }
            }
            HirExpr::Println(arg) => self.codegen_println(arg, vars, fn_value),
            HirExpr::Assert { condition, message } => {
                self.codegen_assert(condition, message, vars, fn_value)
            }
            HirExpr::BoolLit(b) => Some(self.i64_type.const_int(if *b { 1 } else { 0 }, false)),
            HirExpr::UnitLit => Some(self.i64_type.const_int(0, false)),
            HirExpr::Match { scrutinee, arms } => {
                let scrutinee_val = self.codegen_expr(scrutinee, vars, fn_value)?;
                if let Some((pattern, _, body)) = arms.first() {
                    match pattern {
                        HirPattern::OptionSome(inner) | HirPattern::ResultOk(inner) => {
                            if let HirPattern::Var(name) = inner.as_ref() {
                                // Convert the int (which is a pointer) back to an actual pointer
                                let enum_ptr = self
                                    .builder
                                    .build_int_to_ptr(
                                        scrutinee_val,
                                        self.context.ptr_type(AddressSpace::from(0u16)),
                                        "enum_ptr",
                                    )
                                    .ok()?;

                                // Determine which enum type this is
                                let enum_name = if matches!(pattern, HirPattern::OptionSome(_)) {
                                    self.option_sym
                                } else {
                                    self.result_sym
                                };

                                if let Some(st) =
                                    self.enum_struct_types.borrow().get(&enum_name).copied()
                                {
                                    // Get pointer to payload field (index 1)
                                    let payload_ptr = self
                                        .builder
                                        .build_struct_gep(st, enum_ptr, 1, "payload_ptr")
                                        .ok()?;
                                    // Bitcast [i8 x 8] to i64*
                                    let arg_ptr = self
                                        .builder
                                        .build_bit_cast(
                                            payload_ptr,
                                            self.context.ptr_type(AddressSpace::from(0u16)),
                                            "arg_ptr",
                                        )
                                        .ok()?
                                        .into_pointer_value();
                                    // Load the actual payload value
                                    let payload_val = self
                                        .builder
                                        .build_load(self.i64_type, arg_ptr, "payload_val")
                                        .ok()?
                                        .into_int_value();

                                    let alloca = self
                                        .builder
                                        .build_alloca(self.i64_type, self.interner.resolve(*name))
                                        .ok()?;
                                    self.builder.build_store(alloca, payload_val).ok()?;
                                    vars.insert(*name, alloca);
                                }
                            }
                        }
                        _ => {}
                    }
                    self.codegen_expr(body, vars, fn_value)
                } else {
                    Some(self.i64_type.const_int(0, false))
                }
            }
            HirExpr::EnumVariant {
                enum_name,
                variant_name,
                args,
            } => {
                let enum_struct_type = self.enum_struct_types.borrow().get(enum_name).copied();
                let tag_map = self.enum_variant_tags.borrow();
                let tag = tag_map
                    .get(&(*enum_name, *variant_name))
                    .copied()
                    .unwrap_or(0);
                drop(tag_map);
                if let Some(st) = enum_struct_type {
                    let alloca = self.builder.build_alloca(st, "enum_tmp").unwrap();
                    // store tag
                    let tag_val = self.i32_type.const_int(tag as u64, false);
                    let tag_ptr = self
                        .builder
                        .build_struct_gep(st, alloca, 0, "tag_ptr")
                        .unwrap();
                    self.builder.build_store(tag_ptr, tag_val).unwrap();
                    // store payload if any
                    if !args.is_empty() {
                        let payload_ptr = self
                            .builder
                            .build_struct_gep(st, alloca, 1, "payload_ptr")
                            .unwrap();
                        // bitcast payload [i8] to i64*
                        let arg_ptr = self
                            .builder
                            .build_bit_cast(
                                payload_ptr,
                                self.context.ptr_type(AddressSpace::from(0u16)),
                                "arg_ptr",
                            )
                            .unwrap()
                            .into_pointer_value();
                        let arg_val = self
                            .codegen_expr(&args[0], vars, fn_value)
                            .unwrap_or(self.i64_type.const_int(0, false));
                        self.builder.build_store(arg_ptr, arg_val).unwrap();
                    }
                    let ptr_i64 = self
                        .builder
                        .build_ptr_to_int(alloca, self.i64_type, "enum_ptr")
                        .unwrap();
                    Some(ptr_i64)
                } else {
                    args.first()
                        .and_then(|a| self.codegen_expr(a, vars, fn_value))
                        .or_else(|| Some(self.i64_type.const_int(0, false)))
                }
            }
            HirExpr::As { .. } | HirExpr::FloatLit(_) => Some(self.i64_type.const_int(0, false)),
            HirExpr::StructLit {
                struct_name,
                fields,
            } => {
                let struct_type_opt = self.struct_types.borrow().get(struct_name).copied();
                match struct_type_opt {
                    Some(st) => {
                        let alloca = self.builder.build_alloca(st, "struct_lit").ok()?;
                        let st_ptr = alloca;
                        for (i, (_field_name, field_expr)) in fields.iter().enumerate() {
                            let field_val = self.codegen_expr(field_expr, vars, fn_value)?;
                            let indices = &[
                                self.i32_type.const_int(0, false),
                                self.i32_type.const_int(i as u64, false),
                            ];
                            let field_ptr = unsafe {
                                self.builder.build_gep(st, st_ptr, indices, "field").ok()?
                            };
                            self.builder.build_store(field_ptr, field_val).ok()?;
                        }
                        let ptr_i64 = self
                            .builder
                            .build_ptr_to_int(alloca, self.i64_type, "struct_ptr")
                            .ok()?;
                        Some(ptr_i64)
                    }
                    None => Some(self.i64_type.const_int(0, false)),
                }
            }
            HirExpr::FieldAccess { object, field } => {
                let obj_val = self.codegen_expr(object, vars, fn_value)?;
                let obj_ptr = self
                    .builder
                    .build_int_to_ptr(
                        obj_val,
                        self.context.ptr_type(AddressSpace::from(0u16)),
                        "to_ptr",
                    )
                    .ok()?;
                let index_map = self.struct_field_indices.borrow();
                let field_idx = index_map
                    .iter()
                    .find(|((_, f), _)| f == field)
                    .map(|(_, &idx)| idx)
                    .unwrap_or(0);
                drop(index_map);
                let field_to_struct = self.struct_types.borrow();
                let struct_type_opt = field_to_struct.values().next().copied();
                let indices = &[
                    self.i32_type.const_int(0, false),
                    self.i32_type.const_int(field_idx as u64, false),
                ];
                let field_ptr = if let Some(st_type) = struct_type_opt {
                    unsafe {
                        self.builder
                            .build_gep(st_type, obj_ptr, indices, "field_access")
                            .ok()?
                    }
                } else {
                    return Some(self.i64_type.const_int(0, false));
                };
                let field_val = self
                    .builder
                    .build_load(self.i64_type, field_ptr, "field_val")
                    .ok()?
                    .into_int_value();
                Some(field_val)
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
