use glyim_parse::{parse, ExprKind, Item};

fn unwrap_main_expr(out: &glyim_parse::ParseOutput) -> &ExprKind {
    if let Item::Binding { value, .. } = &out.ast.items[0] {
        if let ExprKind::Lambda { body, .. } = &value.kind {
            return &body.kind;
        }
        return &value.kind;
    }
    panic!("expected binding");
}

#[test]
fn parse_bool_true() {
    let out = parse("main = () => true");
    assert!(out.errors.is_empty());
    assert_eq!(*unwrap_main_expr(&out), ExprKind::BoolLit(true));
}

#[test]
fn parse_bool_false() {
    let out = parse("main = () => false");
    assert!(out.errors.is_empty());
    assert_eq!(*unwrap_main_expr(&out), ExprKind::BoolLit(false));
}

#[test]
fn parse_float_literal() {
    let out = parse("main = () => 3.14");
    assert!(out.errors.is_empty());
    assert!(matches!(unwrap_main_expr(&out), ExprKind::FloatLit(f) if (f - 3.14).abs() < 0.001));
}

#[test]
fn parse_struct_definition() {
    let out = parse("struct Point { x, y }");
    assert!(out.errors.is_empty());
    assert!(matches!(&out.ast.items[0], Item::StructDef { .. }));
}

#[test]
fn parse_enum_definition() {
    let out = parse("enum Color { Red, Green, Blue }");
    assert!(out.errors.is_empty());
    assert!(matches!(&out.ast.items[0], Item::EnumDef { .. }));
}

#[test]
fn parse_enum_with_fields() {
    let out = parse("enum Shape { Circle(f64), Rect { w, h } }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_struct_literal() {
    let out = parse("struct Point { x, y }\nmain = () => { Point { x: 1, y: 2 } }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_field_access() {
    let out = parse("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.x }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_match_expression() {
    let out = parse("main = () => match 42 { 1 => 10, _ => 20 }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_match_with_enum_patterns() {
    let src = "enum Color { Red, Green }\nmain = () => { let c = Color::Green; match c { Color::Red => 1, Color::Green => 2 } }";
    let out = parse(src);
    assert!(out.errors.is_empty());
}

#[test]
fn parse_some_pattern() {
    let src = "main = () => { let m = Some(42); match m { Some(v) => v, None => 0 } }";
    let out = parse(src);
    assert!(out.errors.is_empty());
}

#[test]
fn parse_ok_pattern() {
    let src = "main = () => { let r = Ok(42); match r { Ok(v) => v, Err(_) => 0 } }";
    let out = parse(src);
    assert!(out.errors.is_empty());
}

#[test]
fn parse_question_mark() {
    let src = "main = () => { let r = Ok(42)?; r }";
    let out = parse(src);
    assert!(out.errors.is_empty());
}

#[test]
fn parse_at_macro_call() {
    let src = "@identity fn id(expr: Expr) -> Expr { return expr }\nmain = () => @identity(99)";
    let out = parse(src);
    assert!(out.errors.is_empty());
}

#[test]
fn parse_return_expression() {
    let out = parse("fn main() { return 42 }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_as_cast() {
    let out = parse("main = () => 42 as f64");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_extern_block() {
    let src = "extern { fn write(fd: i64, buf: *const u8, len: i64) -> i64; }";
    let out = parse(src);
    assert!(out.errors.is_empty());
}

#[test]
fn parse_match_arm_with_guard() {
    let out = parse("main = () => { match 1 { v if v > 0 => 1, _ => 0 } }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_enum_variant_construction() {
    let src = "enum Color { Red, Green }\nmain = () => { let c = Color::Green; c }";
    let out = parse(src);
    assert!(out.errors.is_empty());
}

#[test]
fn parse_none_literal() {
    let out = parse("main = () => None");
    assert!(out.errors.is_empty());
    assert_eq!(*unwrap_main_expr(&out), ExprKind::NoneExpr);
}

#[test]
fn parse_some_literal() {
    let out = parse("main = () => Some(42)");
    assert!(out.errors.is_empty());
    assert!(matches!(unwrap_main_expr(&out), ExprKind::SomeExpr(_)));
}

#[test]
fn parse_ok_literal() {
    let out = parse("main = () => Ok(42)");
    assert!(out.errors.is_empty());
    assert!(matches!(unwrap_main_expr(&out), ExprKind::OkExpr(_)));
}

#[test]
fn parse_err_literal() {
    let out = parse("main = () => Err(0)");
    assert!(out.errors.is_empty());
    assert!(matches!(unwrap_main_expr(&out), ExprKind::ErrExpr(_)));
}

#[test] #[ignore]
fn parse_unit_literal() {
    let out = parse("main = () => ()");
    assert!(out.errors.is_empty());
}

#[test] #[ignore]
fn parse_raw_pointer() {
    let out = parse("main = () => { let p = *const i64; p }");
    assert!(out.errors.is_empty());
}

#[test] #[ignore]
fn parse_let_with_type_annotation() {
    let out = parse("main = () => { let x: f64 = 3.14; 1 }");
    assert!(out.errors.is_empty());
}
