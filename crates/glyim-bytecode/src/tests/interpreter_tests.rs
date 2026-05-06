use crate::compiler::BytecodeCompiler;
use crate::interpreter::BytecodeInterpreter;
use crate::op::{BytecodeOp, binop_to_tag};
use crate::value::Value;
use glyim_hir::node::{HirBinOp, HirExpr, HirFn};
use glyim_hir::types::{ExprId, HirType};
use glyim_diag::Span;
use glyim_interner::Interner;

fn s() -> Span { Span::new(0, 0) }
fn int(v: i64) -> HirExpr { HirExpr::IntLit { id: ExprId::new(0), value: v, span: s() } }
fn compile(source_interner: &Interner, hir_fn: &HirFn) -> crate::compiler::BytecodeFn {
    let mut compiler = BytecodeCompiler::new(source_interner);
    compiler.compile_fn(hir_fn)
}

fn mkfn(i: &mut Interner, name: &str, param: &str, body: HirExpr) -> HirFn {
    HirFn { doc: None, name: i.intern(name), type_params: vec![],
        params: vec![(i.intern(param), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body, span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false }
}

#[test] fn interpret_int_literal() {
    let mut i=Interner::new(); let f=mkfn(&mut i,"a","x",int(42));
    let bc=compile(&i,&f); assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Int(42));
}
#[test] fn interpret_add() {
    let mut i=Interner::new();
    let f=mkfn(&mut i,"a","x",HirExpr::Binary{id:ExprId::new(0),op:HirBinOp::Add,lhs:Box::new(int(3)),rhs:Box::new(int(4)),span:s()});
    let bc=compile(&i,&f); assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Int(7));
}
#[test] fn interpret_bool() {
    let mut i=Interner::new(); let b=HirExpr::BoolLit{id:ExprId::new(0),value:true,span:s()};
    let f=mkfn(&mut i,"t","x",b); let bc=compile(&i,&f);
    assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Bool(true));
}
#[test] fn interpret_sub() {
    let mut i=Interner::new();
    let f=mkfn(&mut i,"s","x",HirExpr::Binary{id:ExprId::new(0),op:HirBinOp::Sub,lhs:Box::new(int(10)),rhs:Box::new(int(3)),span:s()});
    let bc=compile(&i,&f); assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Int(7));
}
#[test] fn interpret_mul() {
    let mut i=Interner::new();
    let f=mkfn(&mut i,"m","x",HirExpr::Binary{id:ExprId::new(0),op:HirBinOp::Mul,lhs:Box::new(int(6)),rhs:Box::new(int(7)),span:s()});
    let bc=compile(&i,&f); assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Int(42));
}
#[test] fn interpret_neg() {
    let mut i=Interner::new();
    let f=mkfn(&mut i,"n","x",HirExpr::Unary{id:ExprId::new(0),op:glyim_hir::node::HirUnOp::Neg,operand:Box::new(int(5)),span:s()});
    let bc=compile(&i,&f); assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Int(-5));
}
#[test] fn interpret_eq() {
    let mut i=Interner::new();
    let f=mkfn(&mut i,"e","x",HirExpr::Binary{id:ExprId::new(0),op:HirBinOp::Eq,lhs:Box::new(int(5)),rhs:Box::new(int(5)),span:s()});
    let bc=compile(&i,&f); assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Bool(true));
}
#[test] fn interpret_param() {
    let mut i=Interner::new(); let x=i.intern("x");
    let f=mkfn(&mut i,"id","x",HirExpr::Ident{id:ExprId::new(0),name:x,span:s()});
    let bc=compile(&i,&f); assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[Value::Int(99)]),Value::Int(99));
}
#[test] fn interpret_manual_add() {
    use crate::compiler::BytecodeFn;
    let bc=BytecodeFn{name:"a".into(),instructions:vec![
        BytecodeOp::PushI64(3),BytecodeOp::PushI64(4),
        BytecodeOp::BinOp(binop_to_tag(HirBinOp::Add)),BytecodeOp::Return],
        local_count:0,param_count:0};
    assert_eq!(BytecodeInterpreter::new().execute_fn(&bc,&[]),Value::Int(7));
}
