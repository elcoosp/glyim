use glyim_codegen_llvm::compile_to_ir;

fn check_filecheck(source: &str, patterns: &[&str]) {
    let ir = compile_to_ir(source).unwrap();
    for pattern in patterns {
        assert!(
            ir.contains(pattern),
            "IR missing pattern: {}\nFull IR:\n{}",
            pattern,
            ir
        );
    }
}

#[test]
fn filecheck_return_42() {
    check_filecheck("main = () => 42", &["define i32 @main", "ret i32"]);
}

#[test]
fn filecheck_let_and_add() {
    check_filecheck("main = () => { let x = 10; let y = 32; x + y }", &["define i32 @main", "add i64", "ret i32"]);
}

#[test]
fn filecheck_if_else() {
    check_filecheck("main = () => { if true { 1 } else { 0 } }", &["define i32 @main", "phi i64"]);
}

#[test]
fn filecheck_while_loop() {
    check_filecheck("main = () => { let mut i = 0; while i < 3 { i = i + 1 }; i }", &["define i32 @main", "icmp slt", "br i1"]);
}

#[test]
fn filecheck_struct_lit() {
    check_filecheck("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.x }", &["define i32 @main", "call ptr @__glyim_alloc", "getelementptr", "store i64"]);
}

#[test]
fn filecheck_assert_pass() {
    check_filecheck("main = () => { assert(1 == 1); 0 }", &["define i32 @main", "call void @__glyim_assert_fail"]);
}

#[test]
fn filecheck_call_fn() {
    check_filecheck("fn add(a, b) { a + b }\nmain = () => add(1, 2)", &["define i64 @add", "add i64", "define i32 @main", "call i64 @add"]);
}
