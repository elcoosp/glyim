//! TDD red test for module-scoped generic structs (Script 27).
//!
//! Repro of the bug from the stdlib probe: `struct Once<T>` at crate root
//! retains its `<T>`, but the same struct wrapped as `pub mod iter { pub
//! struct Once<T> { .. } }` loses the type parameter, producing 41 errors
//! like "generic type `Once` expects 0 type argument(s), found 1".

use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;
use crate::lower::lower_crate_raw;
use crate::ItemKind;

fn lower_first_struct_generics(src: &str) -> usize {
    let root = parse_to_syntax(src, FileId::BOGUS).root;
    let mut interner = Interner::new();
    let mut diags = Vec::new();
    let hir = lower_crate_raw(&root, &mut interner, &mut diags);

    // Walk all items (including module children) and find the first Struct.
    fn walk(hir: &crate::CrateHir, items: &[crate::ItemId]) -> Option<crate::Item> {
        for id in items {
            let item = hir.items.get(*id)?;
            match &item.kind {
                ItemKind::Struct(s) => return Some(item.clone()),
                ItemKind::Mod(m) => {
                    if let Some(found) = walk(hir, &m.children) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let roots: Vec<crate::ItemId> = hir.items.iter_enumerated().map(|(id, _)| id).collect();
    let item = walk(&hir, &roots).expect("expected at least one struct item");
    match &item.kind {
        ItemKind::Struct(s) => s.generic_params.len(),
        _ => unreachable!(),
    }
}

#[test]
fn crate_root_generic_struct_retains_type_param() {
    let n = lower_first_struct_generics("struct Foo<T> { x: T }\n");
    assert_eq!(n, 1, "crate-root struct must retain its 1 generic param");
}

#[test]
fn module_wrapped_generic_struct_retains_type_param() {
    // The bug: `pub mod m { pub struct Foo<T> { .. } }` — HIR drops `<T>`.
    let n = lower_first_struct_generics(
        "pub mod m {\n    pub struct Foo<T> { x: T }\n}\n",
    );
    assert_eq!(
        n, 1,
        "module-wrapped struct must retain its 1 generic param (Script 27 red)"
    );
}
