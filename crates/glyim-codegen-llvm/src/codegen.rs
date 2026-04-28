use std::collections::HashMap;
use glyim_interner::{Interner,Symbol};
use glyim_hir::{Hir,HirBinOp,HirExpr,HirStmt,HirUnOp};

pub struct Codegen<'ctx> {
    context: &'ctx inkwell::context::Context,
    module: inkwell::module::Module<'ctx>,
    builder: inkwell::builder::Builder<'ctx>,
    i64_type: inkwell::types::IntType<'ctx>,
    i32_type: inkwell::types::IntType<'ctx>,
    interner: Interner,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx inkwell::context::Context, interner: Interner) -> Self {
        let module = context.create_module("glyim_out");
        let builder = context.create_builder();
        Self { context, module, builder, i64_type: context.i64_type(), i32_type: context.i32_type(), interner }
    }
    pub fn generate(&mut self, hir: &Hir) -> Result<(), String> {
        for f in &hir.fns { self.codegen_fn(f)?; }
        if self.module.get_function("main").is_none() { Err("no 'main' function".into()) } else { Ok(()) }
    }
    fn codegen_fn(&mut self, f: &glyim_hir::HirFn) -> Result<(), String> {
        let name = self.interner.resolve(f.name);
        let is_main = name == "main";
        let ret_type = if is_main { self.i32_type } else { self.i64_type };
        let fn_type = ret_type.fn_type(&[], false);
        let fn_val = self.module.add_function(name, fn_type, None);
        let entry = self.context.append_basic_block(fn_val, "entry");
        self.builder.position_at_end(entry);
        let mut vars = HashMap::new();
        for (i, p) in f.params.iter().enumerate() {
            vars.insert(*p, fn_val.get_nth_param(i as u32).ok_or("missing param")?.into_int_value());
        }
        let body = self.codegen_expr(&f.body, &vars).ok_or("codegen fail")?;
        let ret = if is_main { self.builder.build_int_truncate(body, self.i32_type, "trunc").map_err(|e| e.to_string())? } else { body };
        self.builder.build_return(Some(&ret)).map_err(|e| e.to_string())?;
        if !fn_val.verify(true) { return Err("verification fail".into()); }
        Ok(())
    }
    fn codegen_expr(&self, e: &HirExpr, vars: &HashMap<Symbol, inkwell::values::IntValue<'ctx>>) -> Option<inkwell::values::IntValue<'ctx>> {
        match e {
            HirExpr::IntLit(n) => Some(self.i64_type.const_int(*n as u64, true)),
            HirExpr::Ident(s) => vars.get(s).copied(),
            HirExpr::Binary { op, lhs, rhs } => {
                let l = self.codegen_expr(lhs, vars)?;
                let r = self.codegen_expr(rhs, vars)?;
                self.codegen_binop(op.clone(), l, r)
            }
            HirExpr::Unary { op, operand } => {
                let v = self.codegen_expr(operand, vars)?;
                match op { HirUnOp::Neg => { let z = self.i64_type.const_int(0, false); self.builder.build_int_sub(z, v, "neg").ok() } HirUnOp::Not => self.builder.build_not(v, "not").ok() }
            }
            HirExpr::Block(stmts) => {
                let mut last = Some(self.i64_type.const_int(0, false));
                for stmt in stmts {
                    match stmt {
                        HirStmt::Expr(inner) => last = self.codegen_expr(inner, vars),
                        HirStmt::Let { .. } | HirStmt::Assign { .. } => last = Some(self.i64_type.const_int(0, false)),
                    }
                }
                last
            }
            HirExpr::StrLit(_) => Some(self.i64_type.const_int(0, false)), // stub
            HirExpr::If { .. } => Some(self.i64_type.const_int(0, false)), // stub
            HirExpr::Println(_) => Some(self.i64_type.const_int(0, false)), // stub
            HirExpr::Assert { .. } => Some(self.i64_type.const_int(0, false)), // stub
        }
    }
    fn codegen_binop(&self, op: HirBinOp, l: inkwell::values::IntValue<'ctx>, r: inkwell::values::IntValue<'ctx>) -> Option<inkwell::values::IntValue<'ctx>> {
        use inkwell::IntPredicate;
        match op {
            HirBinOp::Add => self.builder.build_int_add(l,r,"add").ok(),
            HirBinOp::Sub => self.builder.build_int_sub(l,r,"sub").ok(),
            HirBinOp::Mul => self.builder.build_int_mul(l,r,"mul").ok(),
            HirBinOp::Div => self.builder.build_int_signed_div(l,r,"div").ok(),
            HirBinOp::Mod => self.builder.build_int_signed_rem(l,r,"rem").ok(),
            HirBinOp::Eq => self.cmp_extend(IntPredicate::EQ,l,r),
            HirBinOp::Neq => self.cmp_extend(IntPredicate::NE,l,r),
            HirBinOp::Lt => self.cmp_extend(IntPredicate::SLT,l,r),
            HirBinOp::Gt => self.cmp_extend(IntPredicate::SGT,l,r),
            HirBinOp::Lte => self.cmp_extend(IntPredicate::SLE,l,r),
            HirBinOp::Gte => self.cmp_extend(IntPredicate::SGE,l,r),
            HirBinOp::And => self.builder.build_and(l,r,"and").ok(),
            HirBinOp::Or => self.builder.build_or(l,r,"or").ok(),
        }
    }
    fn cmp_extend(&self, pred: inkwell::IntPredicate, l: inkwell::values::IntValue<'ctx>, r: inkwell::values::IntValue<'ctx>) -> Option<inkwell::values::IntValue<'ctx>> {
        let c = self.builder.build_int_compare(pred,l,r,"cmp").ok()?;
        self.builder.build_int_z_extend(c, self.i64_type, "zext").ok()
    }
    pub fn ir_string(&self) -> String { self.module.print_to_string().to_string() }
    pub fn module(&self) -> &inkwell::module::Module<'ctx> { &self.module }
    pub fn write_object_file(&self, path: &std::path::Path) -> Result<(), String> {
        use inkwell::targets::{Target,TargetMachine,CodeModel,FileType,RelocMode,InitializationConfig};
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let machine = target.create_target_machine(&triple,"","",inkwell::OptimizationLevel::None,RelocMode::PIC,CodeModel::Default).ok_or("target machine")?;
        machine.write_to_file(&self.module, FileType::Object, path).map_err(|e| e.to_string())
    }
}

pub fn compile_to_ir(source: &str) -> Result<String,String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() { return Err(format!("parse: {:?}", out.errors)); }
    let hir = glyim_hir::lower(&out.ast, &out.interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = Codegen::new(&ctx, out.interner);
    cg.generate(&hir)?;
    Ok(cg.ir_string())
}
