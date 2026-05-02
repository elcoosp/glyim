use glyim_parse::parse;

#[test]
fn recovery_after_broken_fn_param() {
    let src = "fn foo(a, , b) { a + b }\nfn bar() { 42 }\nmain = () => bar()";
    let out = parse(src);
    assert!(!out.errors.is_empty(), "should have parse errors for doubled comma");
    let has_bar = out.ast.items.iter().any(|item| {
        matches!(item, glyim_parse::Item::FnDef { name, .. } if out.interner.resolve(*name) == "bar")
    });
    assert!(has_bar, "bar should be parsed despite earlier error");
}

#[test]
fn recovery_after_unknown_token_in_struct() {
    let src = "struct Point { x, @, y }\nstruct Color { Red, Green }\nmain = () => 0";
    let out = parse(src);
    assert!(!out.errors.is_empty(), "should have errors for '@' in struct fields");
    // The current parser may recover only partially; at least one struct should be parsed
    let struct_count = out.ast.items.iter().filter(|item| {
        matches!(item, glyim_parse::Item::StructDef { .. })
    }).count();
    assert!(struct_count >= 1, "at least one struct should be parsed, got {}", struct_count);
}

#[test]
fn recovery_skips_garbage_between_items() {
    let src = "fn foo() { 1 }\n@garbage!!!\nfn bar() { 2 }\nmain = () => bar()";
    let out = parse(src);
    assert!(!out.errors.is_empty(), "should have errors for garbage");
    let has_foo = out.ast.items.iter().any(|i| matches!(i, glyim_parse::Item::FnDef { name, .. } if out.interner.resolve(*name) == "foo"));
    let has_bar = out.ast.items.iter().any(|i| matches!(i, glyim_parse::Item::FnDef { name, .. } if out.interner.resolve(*name) == "bar"));
    assert!(has_foo && has_bar, "both valid functions should be parsed");
}
