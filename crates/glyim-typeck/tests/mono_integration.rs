use glyim_parse::parse;
use glyim_hir::lower;
use glyim_hir::monomorphize::monomorphize;
use glyim_hir::item::HirItem;
use glyim_typeck::TypeChecker;
use glyim_interner::Interner;

fn typecheck_and_monomorphize(
    source: &str,
) -> (glyim_hir::monomorphize::MonoResult, Interner) {
    let parse_out = parse(source);
    assert!(parse_out.errors.is_empty());
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner.clone());
    tc.check(&hir).expect("type check should succeed");
    let expr_types = tc.expr_types;
    let call_type_args = tc.call_type_args;
    let result = monomorphize(&hir, &mut interner, &expr_types, &call_type_args);
    (result, interner)
}

#[test]
fn mono_specializes_id_to_concrete() {
    let (result, interner) = typecheck_and_monomorphize(
        "fn id<T>(x: T) -> T { x }\nmain = () => id(42)",
    );
    let has_id_i64 = result.hir.items.iter().any(|item| {
        if let HirItem::Fn(f) = item {
            interner.resolve(f.name) == "id__i64"
        } else {
            false
        }
    });
    assert!(has_id_i64, "monomorphized HIR should contain 'id__i64'");
}

#[test]
fn mono_specializes_generic_struct_literal() {
    let (result, interner) = typecheck_and_monomorphize(
        "struct Container<T> { value: T }\nmain = () => {\n    let c: Container<i64> = Container { value: 42 };\n    c.value\n}",
    );
    let has_container_i64 = result.hir.items.iter().any(|item| {
        if let HirItem::Struct(s) = item {
            interner.resolve(s.name) == "Container__i64"
        } else {
            false
        }
    });
    assert!(has_container_i64, "monomorphized HIR should contain 'Container__i64' struct");
}

#[test]
fn mono_rewrites_call_graph() {
    let (result, interner) = typecheck_and_monomorphize(
        "fn a<T>(x: T) -> T { b(x) }\nfn b<U>(x: U) -> U { x }\nmain = () => a(42)",
    );
    let has_a = result.hir.items.iter().any(|item| {
        if let HirItem::Fn(f) = item {
            interner.resolve(f.name) == "a__i64"
        } else {
            false
        }
    });
    let has_b = result.hir.items.iter().any(|item| {
        if let HirItem::Fn(f) = item {
            interner.resolve(f.name) == "b__i64"
        } else {
            false
        }
    });
    assert!(has_a, "monomorphized HIR should contain 'a__i64'");
    assert!(has_b, "monomorphized HIR should contain 'b__i64'");
}
