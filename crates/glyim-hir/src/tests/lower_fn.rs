use crate::lower::lower_crate;
use crate::{ItemId, ItemKind};
use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

#[test]
fn test_fn_with_params() {
    let source = "fn add(x: i32, y: i32) -> i32 { x + y }";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let hir = lower_crate(&parse_result.root, &mut interner, &mut Vec::new());
    assert_eq!(hir.items.len(), 1);
    let item = &hir.items[ItemId::from_raw(0)];
    assert_eq!(interner.resolve(item.name), "add");
    match &item.kind {
        ItemKind::Fn(fn_item) => {
            assert_eq!(fn_item.params.len(), 2);
            assert_eq!(interner.resolve(fn_item.params[0].name), "x");
            assert_eq!(interner.resolve(fn_item.params[1].name), "y");
            assert!(fn_item.return_ty.is_some());
            assert!(fn_item.body.is_some());
        }
        _ => panic!(),
    }
}

#[test]
fn test_extern_c_fn_gets_c_abi() {
    // `extern "C" fn` is a single FFI function declaration (unstub-5 Phase 4):
    // it must lower to a `FnItem` whose `abi` is `Some("C")`.
    let source = "extern \"C\" fn add(a: i32, b: i32) -> i32 { a + b }";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let hir = lower_crate(&parse_result.root, &mut interner, &mut Vec::new());
    assert_eq!(hir.items.len(), 1);
    let item = &hir.items[ItemId::from_raw(0)];
    assert_eq!(interner.resolve(item.name), "add");
    match &item.kind {
        ItemKind::Fn(fn_item) => {
            assert_eq!(fn_item.params.len(), 2);
            assert!(fn_item.body.is_some());
            let abi = fn_item.abi.expect("extern \"C\" fn must carry an abi name");
            assert_eq!(interner.resolve(abi), "C");
        }
        _ => panic!("expected ItemKind::Fn"),
    }
}

#[test]
fn test_bare_extern_fn_defaults_to_c_abi() {
    // A bare `extern fn` (no ABI string) also denotes the C calling
    // convention (unstub-5 Phase 4).
    let source = "extern fn sub(a: i32, b: i32) -> i32 { a - b }";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let mut interner = Interner::new();
    let hir = lower_crate(&parse_result.root, &mut interner, &mut Vec::new());
    let item = &hir.items[ItemId::from_raw(0)];
    match &item.kind {
        ItemKind::Fn(fn_item) => {
            let abi = fn_item.abi.expect("bare extern fn must carry an abi name");
            assert_eq!(interner.resolve(abi), "C");
        }
        _ => panic!("expected ItemKind::Fn"),
    }
}


#[test]
fn unsafe_fn_sets_is_unsafe() {
    // §3.5.25: `is_unsafe` was hardcoded `false` — the parser emits
    // `KwUnsafe` as a *sibling* of the `FnDef` node, so the child-token
    // scan in `lower_fn` never saw it. This asserts the field is real.
    use crate::ItemKind;
    use glyim_core::interner::Interner;
    use glyim_frontend::parse_to_syntax;
    use glyim_span::FileId;

    let src = "unsafe fn f() {}";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(src, file_id);
    let mut interner = Interner::new();
    let hir = crate::lower::lower_crate(&parse_result.root, &mut interner, &mut Vec::new());

    let item = &hir.items[crate::ItemId::from_raw(0)];
    match &item.kind {
        ItemKind::Fn(f) => assert!(
            f.is_unsafe,
            "unsafe fn must set is_unsafe = true; parser emits KwUnsafe as a \
             sibling of FnDef, and the fix walks prev_sibling_or_token"
        ),
        other => panic!("expected Fn item, got {:?}", other),
    }
}

#[test]
fn plain_fn_is_not_unsafe() {
    use crate::ItemKind;
    use glyim_core::interner::Interner;
    use glyim_frontend::parse_to_syntax;
    use glyim_span::FileId;

    let src = "fn f() {}";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(src, file_id);
    let mut interner = Interner::new();
    let hir = crate::lower::lower_crate(&parse_result.root, &mut interner, &mut Vec::new());

    let item = &hir.items[crate::ItemId::from_raw(0)];
    match &item.kind {
        ItemKind::Fn(f) => assert!(!f.is_unsafe, "plain fn must not set is_unsafe"),
        other => panic!("expected Fn item, got {:?}", other),
    }
}
