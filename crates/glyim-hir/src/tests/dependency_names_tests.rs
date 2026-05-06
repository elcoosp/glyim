use crate::dependency_names::NameDependencyTable;
use glyim_interner::Interner;

#[test]
fn empty_table_has_no_dependencies() {
    let table = NameDependencyTable::new();
    let mut i = Interner::new();
    let foo = i.intern("foo");
    assert!(table.definitions_for_sym(foo).is_empty());
    assert!(table.references_for_sym(foo).is_empty());
}

#[test]
fn record_definition() {
    let mut i = Interner::new();
    let mut table = NameDependencyTable::new();
    let fn_name = i.intern("my_fn");
    let struct_name = i.intern("MyStruct");
    table.add_definition(fn_name, struct_name);
    let defs = table.definitions_for_sym(fn_name);
    assert_eq!(defs.len(), 1);
    assert!(defs.contains(&struct_name));
}

#[test]
fn record_reference() {
    let mut i = Interner::new();
    let mut table = NameDependencyTable::new();
    let fn_name = i.intern("caller");
    let callee = i.intern("callee");
    table.add_reference(fn_name, callee);
    let refs = table.references_for_sym(fn_name);
    assert_eq!(refs.len(), 1);
    assert!(refs.contains(&callee));
}

#[test]
fn transitive_dependents_single_hop() {
    let mut i = Interner::new();
    let mut table = NameDependencyTable::new();
    let a = i.intern("a");
    let b = i.intern("b");
    let c = i.intern("c");
    table.add_reference(a, b);
    table.add_reference(b, c);
    let deps = table.transitive_dependents(&[c]);
    assert!(deps.contains(&b));
    assert!(deps.contains(&a));
}

#[test]
fn transitive_dependents_diamond() {
    let mut i = Interner::new();
    let mut table = NameDependencyTable::new();
    let a = i.intern("a");
    let b = i.intern("b");
    let c = i.intern("c");
    let d = i.intern("d");
    table.add_reference(a, b);
    table.add_reference(a, c);
    table.add_reference(b, d);
    table.add_reference(c, d);
    let deps = table.transitive_dependents(&[d]);
    assert!(deps.contains(&b));
    assert!(deps.contains(&c));
    assert!(deps.contains(&a));
    assert_eq!(deps.len(), 3);
}

#[test]
fn transitive_dependents_unrelated_not_affected() {
    let mut i = Interner::new();
    let mut table = NameDependencyTable::new();
    let a = i.intern("a");
    let unrelated = i.intern("unrelated");
    table.add_reference(a, i.intern("b"));
    let deps = table.transitive_dependents(&[unrelated]);
    assert!(!deps.contains(&a));
}

#[test]
fn multiple_definitions() {
    let mut i = Interner::new();
    let mut table = NameDependencyTable::new();
    let fn_name = i.intern("my_fn");
    let s1 = i.intern("Struct1");
    let s2 = i.intern("Struct2");
    table.add_definition(fn_name, s1);
    table.add_definition(fn_name, s2);
    let defs = table.definitions_for_sym(fn_name);
    assert_eq!(defs.len(), 2);
}

#[test]
fn direct_dependents() {
    let mut i = Interner::new();
    let mut table = NameDependencyTable::new();
    let callee = i.intern("callee");
    let caller1 = i.intern("caller1");
    let caller2 = i.intern("caller2");
    table.add_reference(caller1, callee);
    table.add_reference(caller2, callee);
    let deps = table.direct_dependents(callee);
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&caller1));
    assert!(deps.contains(&caller2));
}
