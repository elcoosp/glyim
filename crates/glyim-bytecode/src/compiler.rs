use crate::op::{BytecodeOp, binop_to_tag, unop_to_tag};
use glyim_hir::node::{HirExpr, HirFn, HirStmt};
use glyim_hir::types::HirPattern;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct BytecodeFn {
    pub name: String,
    pub instructions: Vec<BytecodeOp>,
    pub local_count: u32,
    pub param_count: u32,
}

pub struct BytecodeCompiler<'a> {
    interner: &'a Interner,
}

impl<'a> BytecodeCompiler<'a> {
    pub fn new(interner: &'a Interner) -> Self { Self { interner } }

    pub fn compile_fn(&mut self, hir_fn: &HirFn) -> BytecodeFn {
        let mut ctx = FnCtx::new(self.interner);
        for &(sym, _) in &hir_fn.params { ctx.register_local(sym); }
        self.compile_expr(&mut ctx, &hir_fn.body);
        if !matches!(ctx.instructions.last(), Some(BytecodeOp::Return)) {
            ctx.emit(BytecodeOp::Return);
        }
        BytecodeFn {
            name: self.resolve(hir_fn.name),
            instructions: ctx.instructions,
            local_count: ctx.next_local,
            param_count: hir_fn.params.len() as u32,
        }
    }

    fn compile_expr(&self, ctx: &mut FnCtx, expr: &HirExpr) {
        match expr {
            HirExpr::IntLit { value, .. } => ctx.emit(BytecodeOp::PushI64(*value)),
            HirExpr::FloatLit { value, .. } => ctx.emit(BytecodeOp::PushF64(*value)),
            HirExpr::BoolLit { value, .. } => ctx.emit(BytecodeOp::PushBool(*value)),
            HirExpr::StrLit { value, .. } => ctx.emit(BytecodeOp::PushStr(value.clone())),
            HirExpr::UnitLit { .. } => ctx.emit(BytecodeOp::PushUnit),
            HirExpr::Ident { name, .. } => {
                if let Some(&id) = ctx.local_map.get(name) { ctx.emit(BytecodeOp::LoadLocal(id)); }
                else { ctx.emit(BytecodeOp::Call { name: self.resolve(*name), arg_count: 0 }); }
            }
            HirExpr::Binary { op, lhs, rhs, .. } => { self.compile_expr(ctx, lhs); self.compile_expr(ctx, rhs); ctx.emit(BytecodeOp::BinOp(binop_to_tag(*op))); }
            HirExpr::Unary { op, operand, .. } => { self.compile_expr(ctx, operand); ctx.emit(BytecodeOp::UnOp(unop_to_tag(*op))); }
            HirExpr::Block { stmts, .. } => { for s in stmts { self.compile_stmt(ctx, s); } }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.compile_expr(ctx, condition);
                let jmp_else = ctx.instructions.len(); ctx.emit(BytecodeOp::JumpIfFalse(0));
                self.compile_expr(ctx, then_branch);
                let jmp_over = ctx.instructions.len(); ctx.emit(BytecodeOp::Jump(0));
                let else_start = ctx.instructions.len() as u32; ctx.patch_jump(jmp_else, else_start);
                if let Some(e) = else_branch { self.compile_expr(ctx, e); } else { ctx.emit(BytecodeOp::PushUnit); }
                ctx.patch_jump(jmp_over, ctx.instructions.len() as u32);
            }
            HirExpr::Call { callee, args, .. } => {
                for a in args { self.compile_expr(ctx, a); }
                ctx.emit(BytecodeOp::Call { name: self.resolve(*callee), arg_count: args.len() as u32 });
            }
            HirExpr::Return { value, .. } => { if let Some(v) = value { self.compile_expr(ctx, v); } else { ctx.emit(BytecodeOp::PushUnit); } ctx.emit(BytecodeOp::Return); }
            HirExpr::Println { arg, .. } => { self.compile_expr(ctx, arg); ctx.emit(BytecodeOp::Println); }
            HirExpr::While { condition, body, .. } => {
                let loop_start = ctx.instructions.len() as u32;
                self.compile_expr(ctx, condition);
                let jmp_out = ctx.instructions.len(); ctx.emit(BytecodeOp::JumpIfFalse(0));
                self.compile_expr(ctx, body);
                ctx.emit(BytecodeOp::Jump(loop_start));
                ctx.patch_jump(jmp_out, ctx.instructions.len() as u32);
            }
            HirExpr::As { expr, .. } | HirExpr::Deref { expr, .. } => self.compile_expr(ctx, expr),
            HirExpr::SizeOf { .. } => ctx.emit(BytecodeOp::PushI64(8)),
            HirExpr::TupleLit { elements, .. } => { for e in elements { self.compile_expr(ctx, e); } }
            HirExpr::FieldAccess { object, field, .. } => { self.compile_expr(ctx, object); ctx.emit(BytecodeOp::FieldAccess { index: self.resolve(*field).len() as u32 }); }
            HirExpr::StructLit { fields, .. } => {
                ctx.emit(BytecodeOp::AllocStruct { field_count: fields.len() as u32 });
                for (i, (_, v)) in fields.iter().enumerate() { self.compile_expr(ctx, v); ctx.emit(BytecodeOp::FieldSet { index: i as u32 }); }
            }
            HirExpr::EnumVariant { args, .. } => {
                if let Some(a) = args.first() { self.compile_expr(ctx, a); } else { ctx.emit(BytecodeOp::PushUnit); }
                ctx.emit(BytecodeOp::EnumVariant { tag: 0 });
            }
            _ => ctx.emit(BytecodeOp::PushUnit),
        }
    }

    fn compile_stmt(&self, ctx: &mut FnCtx, stmt: &HirStmt) {
        match stmt {
            HirStmt::Let { name, value, .. } => { let id = ctx.register_local(*name); self.compile_expr(ctx, value); ctx.emit(BytecodeOp::StoreLocal(id)); }
            HirStmt::LetPat { pattern, value, .. } => { self.compile_expr(ctx, value); for sym in collect_bindings(pattern) { let id = ctx.register_local(sym); ctx.emit(BytecodeOp::StoreLocal(id)); } }
            HirStmt::Assign { target, value, .. } => { self.compile_expr(ctx, value); if let Some(&id) = ctx.local_map.get(target) { ctx.emit(BytecodeOp::StoreLocal(id)); } }
            HirStmt::Expr(e) => self.compile_expr(ctx, e),
            _ => {}
        }
    }

    fn resolve(&self, sym: Symbol) -> String { self.interner.resolve(sym).to_string() }
}

struct FnCtx<'a> {
    _interner: &'a Interner,
    instructions: Vec<BytecodeOp>,
    local_map: HashMap<Symbol, u32>,
    next_local: u32,
}
impl<'a> FnCtx<'a> {
    fn new(interner: &'a Interner) -> Self { Self { _interner: interner, instructions: Vec::new(), local_map: HashMap::new(), next_local: 0 } }
    fn emit(&mut self, op: BytecodeOp) { self.instructions.push(op); }
    fn register_local(&mut self, sym: Symbol) -> u32 { let id = self.next_local; self.local_map.insert(sym, id); self.next_local += 1; id }
    fn patch_jump(&mut self, idx: usize, target: u32) { match &mut self.instructions[idx] { BytecodeOp::Jump(t) | BytecodeOp::JumpIfFalse(t) => *t = target, _ => {} } }
}

fn collect_bindings(pat: &HirPattern) -> Vec<Symbol> { let mut v = Vec::new(); rec(pat, &mut v); v }
fn rec(pat: &HirPattern, v: &mut Vec<Symbol>) {
    match pat {
        HirPattern::Var(s) => v.push(*s),
        HirPattern::Struct { bindings, .. } | HirPattern::EnumVariant { bindings, .. } => { for (_, p) in bindings { rec(p, v); } }
        HirPattern::Tuple { elements, .. } => { for p in elements { rec(p, v); } }
        HirPattern::OptionSome(p) | HirPattern::ResultOk(p) | HirPattern::ResultErr(p) => rec(p, v),
        _ => {}
    }
}
