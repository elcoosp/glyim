use crate::compiler::BytecodeFn;
use crate::op::{BytecodeOp, tag_to_binop, tag_to_unop};
use crate::value::Value;
use glyim_hir::node::{HirBinOp, HirUnOp};

pub struct BytecodeInterpreter { stack: Vec<Value> }
impl BytecodeInterpreter {
    pub fn new() -> Self { Self { stack: Vec::with_capacity(256) } }

    pub fn execute_fn(&mut self, bc_fn: &BytecodeFn, args: &[Value]) -> Value {
        self.stack.clear();
        let mut locals = vec![Value::Unit; bc_fn.local_count as usize];
        for (i, arg) in args.iter().enumerate() { if i < locals.len() { locals[i] = arg.clone(); } }
        let mut ip: usize = 0;
        while ip < bc_fn.instructions.len() {
            match &bc_fn.instructions[ip] {
                BytecodeOp::PushI64(n) => self.push(Value::Int(*n)),
                BytecodeOp::PushF64(f) => self.push(Value::Float(*f)),
                BytecodeOp::PushBool(b) => self.push(Value::Bool(*b)),
                BytecodeOp::PushStr(s) => self.push(Value::Str(s.clone())),
                BytecodeOp::PushUnit => self.push(Value::Unit),
                BytecodeOp::LoadLocal(i) => self.push(locals[*i as usize].clone()),
                BytecodeOp::StoreLocal(i) => { let v = self.pop(); locals[*i as usize] = v; }
                BytecodeOp::BinOp(tag) => { if let Some(op) = tag_to_binop(*tag) { let r = self.pop(); let l = self.pop(); self.push(eval_binop(op, l, r)); } }
                BytecodeOp::UnOp(tag) => { if let Some(op) = tag_to_unop(*tag) { let o = self.pop(); self.push(eval_unop(op, o)); } }
                BytecodeOp::Jump(t) => { ip = *t as usize; continue; }
                BytecodeOp::JumpIfFalse(t) => { if !self.pop().is_truthy() { ip = *t as usize; continue; } }
                BytecodeOp::Return => return self.stack.pop().unwrap_or(Value::Unit),
                BytecodeOp::Call { arg_count, .. } => { for _ in 0..*arg_count { self.pop(); } self.push(Value::Unit); }
                BytecodeOp::AllocStruct { field_count } => self.push(Value::Struct(vec![Value::Unit; *field_count as usize])),
                BytecodeOp::FieldAccess { index } => { if let Value::Struct(f) = self.pop() { self.push(f.get(*index as usize).cloned().unwrap_or(Value::Unit)); } }
                BytecodeOp::FieldSet { index } => { let v = self.pop(); if let Value::Struct(mut f) = self.pop() { if (*index as usize) < f.len() { f[*index as usize] = v; } self.push(Value::Struct(f)); } }
                BytecodeOp::EnumVariant { tag } => { let p = self.pop(); self.push(Value::Enum(*tag, Box::new(p))); }
                BytecodeOp::Println => { eprintln!("{}", self.pop()); self.push(Value::Unit); }
                BytecodeOp::Assert { message } => { if !self.pop().is_truthy() { eprintln!("ASSERT: {}", message.as_deref().unwrap_or("assertion failed")); } self.push(Value::Unit); }
                BytecodeOp::Nop => {}
            }
            ip += 1;
        }
        self.stack.pop().unwrap_or(Value::Unit)
    }
    fn push(&mut self, v: Value) { self.stack.push(v); }
    fn pop(&mut self) -> Value { self.stack.pop().unwrap_or(Value::Unit) }
}
impl Default for BytecodeInterpreter { fn default() -> Self { Self::new() } }

fn eval_binop(op: HirBinOp, l: Value, r: Value) -> Value {
    match op {
        HirBinOp::Add => i2(l, r, |a,b| a+b, |a,b| a+b),
        HirBinOp::Sub => i2(l, r, |a,b| a-b, |a,b| a-b),
        HirBinOp::Mul => i2(l, r, |a,b| a*b, |a,b| a*b),
        HirBinOp::Div => i2(l, r, |a,b| if b==0{0}else{a/b}, |a,b| a/b),
        HirBinOp::Mod => i2(l, r, |a,b| if b==0{0}else{a%b}, |_,_| 0.0),
        HirBinOp::Eq => match (l,r) { (Value::Int(a),Value::Int(b))=>Value::Bool(a==b), _=>Value::Bool(false) },
        HirBinOp::Neq => match (l,r) { (Value::Int(a),Value::Int(b))=>Value::Bool(a!=b), _=>Value::Bool(true) },
        HirBinOp::Lt => match (l,r) { (Value::Int(a),Value::Int(b))=>Value::Bool(a<b), _=>Value::Bool(false) },
        HirBinOp::Gt => match (l,r) { (Value::Int(a),Value::Int(b))=>Value::Bool(a>b), _=>Value::Bool(false) },
        HirBinOp::Lte => match (l,r) { (Value::Int(a),Value::Int(b))=>Value::Bool(a<=b), _=>Value::Bool(false) },
        HirBinOp::Gte => match (l,r) { (Value::Int(a),Value::Int(b))=>Value::Bool(a>=b), _=>Value::Bool(false) },
        HirBinOp::And => match (l,r) { (Value::Bool(a),Value::Bool(b))=>Value::Bool(a&&b), _=>Value::Bool(false) },
        HirBinOp::Or => match (l,r) { (Value::Bool(a),Value::Bool(b))=>Value::Bool(a||b), _=>Value::Bool(false) },
    }
}
fn eval_unop(op: HirUnOp, o: Value) -> Value {
    match op { HirUnOp::Neg => match o { Value::Int(n)=>Value::Int(-n), Value::Float(f)=>Value::Float(-f), _=>Value::Int(0) }, HirUnOp::Not => match o { Value::Bool(b)=>Value::Bool(!b), _=>Value::Bool(false) } }
}
fn i2(l: Value, r: Value, fi: fn(i64,i64)->i64, ff: fn(f64,f64)->f64) -> Value {
    match (l,r) { (Value::Int(a),Value::Int(b))=>Value::Int(fi(a,b)), (Value::Float(a),Value::Float(b))=>Value::Float(ff(a,b)), _=>Value::Int(0) }
}
