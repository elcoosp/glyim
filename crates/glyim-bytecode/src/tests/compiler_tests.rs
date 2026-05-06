use crate::compiler::BytecodeCompiler;
use crate::op::{BytecodeOp, binop_to_tag};
use glyim_hir::node::{HirBinOp, HirExpr, HirFn, HirStmt};
use glyim_hir::types::{ExprId, HirType};
use glyim_diag::Span;
use glyim_interner::Interner;

fn s() -> Span { Span::new(0, 0) }
fn int(v: i64) -> HirExpr { HirExpr::IntLit { id: ExprId::new(0), value: v, span: s() } }
fn bin(op: HirBinOp, l: HirExpr, r: HirExpr) -> HirExpr {
    HirExpr::Binary { id: ExprId::new(0), op, lhs: Box::new(l), rhs: Box::new(r), span: s() }
}
// Helper: compile_fn creates a compiler from the interner, compiles hir_fn, then returns the bc_fn.
// The compiler reference is dropped before we use hir_fn, so borrows don't conflict.
fn compile(source_interner: &Interner, hir_fn: &HirFn) -> crate::compiler::BytecodeFn {
    let mut compiler = BytecodeCompiler::new(source_interner);
    compiler.compile_fn(hir_fn)
}

#[test] fn compile_int_literal() {
    let mut i=Interner::new(); let f = HirFn { doc: None, name: i.intern("a"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body: int(42), span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert!(bc.instructions.contains(&BytecodeOp::PushI64(42)));
    assert!(bc.instructions.contains(&BytecodeOp::Return));
}
#[test] fn compile_add() {
    let mut i=Interner::new(); let f = HirFn { doc: None, name: i.intern("a"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body: bin(HirBinOp::Add, int(1), int(2)), span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert!(bc.instructions.contains(&BytecodeOp::BinOp(binop_to_tag(HirBinOp::Add))));
}
#[test] fn compile_bool() {
    let mut i=Interner::new(); let b=HirExpr::BoolLit{id:ExprId::new(0),value:true,span:s()};
    let f = HirFn { doc: None, name: i.intern("t"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body: b, span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert!(bc.instructions.contains(&BytecodeOp::PushBool(true)));
}
#[test] fn compile_param_count() {
    let mut i=Interner::new(); let f = HirFn { doc: None, name: i.intern("t"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body: int(0), span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert_eq!(bc.param_count, 1);
}
#[test] fn ends_with_return() {
    let mut i=Interner::new(); let f = HirFn { doc: None, name: i.intern("t"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body: int(99), span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert_eq!(*bc.instructions.last().unwrap(), BytecodeOp::Return);
}
#[test] fn compile_let_and_load() {
    let mut i=Interner::new(); let y=i.intern("y");
    let body = HirExpr::Block { id: ExprId::new(0), stmts: vec![
        HirStmt::Let { name: y, mutable: false, value: int(10), span: s() },
        HirStmt::Expr(HirExpr::Ident { id: ExprId::new(1), name: y, span: s() }) ], span: s() };
    let f = HirFn { doc: None, name: i.intern("lt"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body, span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert!(bc.instructions.contains(&BytecodeOp::PushI64(10)));
    assert!(bc.instructions.contains(&BytecodeOp::StoreLocal(1)));
    assert!(bc.instructions.contains(&BytecodeOp::LoadLocal(1)));
}
#[test] fn compile_if_else() {
    let mut i=Interner::new();
    let body = HirExpr::If { id: ExprId::new(0), condition: Box::new(HirExpr::BoolLit{id:ExprId::new(0),value:true,span:s()}),
        then_branch: Box::new(int(1)), else_branch: Some(Box::new(int(2))), span: s() };
    let f = HirFn { doc: None, name: i.intern("ie"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body, span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert!(bc.instructions.iter().any(|op| matches!(op, BytecodeOp::JumpIfFalse(_))));
}
#[test] fn compile_call() {
    let mut i=Interner::new(); let callee=i.intern("helper");
    let body = HirExpr::Call { id: ExprId::new(0), callee, args: vec![int(1),int(2)], span: s() };
    let f = HirFn { doc: None, name: i.intern("cl"), type_params: vec![],
        params: vec![(i.intern("x"), HirType::Int)], param_mutability: vec![false],
        ret: Some(HirType::Int), body, span: s(), is_pub: false,
        is_macro_generated: false, is_extern_backed: false };
    let bc = compile(&i, &f);
    assert!(bc.instructions.contains(&BytecodeOp::Call { name: "helper".into(), arg_count: 2 }));
}
