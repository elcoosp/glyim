use crate::compile_to_ir;

#[test]
fn zero_as_struct_produces_valid_ir() {
    let source = "struct Point { x, y }\nmain = () => { let p = 0 as Point; p.x }";
    let ir = compile_to_ir(source).unwrap();
    assert!(
        ir.contains("__glyim_alloc"),
        "expected allocation call for zero-as-struct, got:\n{ir}"
    );
    assert!(
        ir.contains("store i64 0"),
        "expected zero stores, got:\n{ir}"
    );
}
