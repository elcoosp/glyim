use crate::lower::lower_crate;
use crate::{ItemId, ItemKind};
use glyim_core::interner::Interner;
use glyim_core::primitives::StructKind;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

#[test]
fn test_struct_record() {
    let source = "struct Point { x: i32, y: i32 }";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let root = parse_result.root;
    let mut interner = Interner::new();
    let hir = lower_crate(&root, &mut interner, &mut Vec::new());

    assert_eq!(hir.items.len(), 1);
    let item = &hir.items[ItemId::from_raw(0)];
    assert_eq!(interner.resolve(item.name), "Point");
    match &item.kind {
        ItemKind::Struct(s) => {
            assert_eq!(s.kind, StructKind::Record);
            assert_eq!(s.fields.len(), 2);
            assert_eq!(interner.resolve(s.fields[0].name), "x");
            assert_eq!(interner.resolve(s.fields[1].name), "y");
            assert!(s.generic_params.is_empty());
        }
        other => panic!("Expected Struct item, got {other:?}"),
    }
}

#[test]
fn test_struct_unit() {
    let source = "struct Unit;";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let root = parse_result.root;
    let mut interner = Interner::new();
    let hir = lower_crate(&root, &mut interner, &mut Vec::new());

    assert_eq!(hir.items.len(), 1);
    let item = &hir.items[ItemId::from_raw(0)];
    assert_eq!(interner.resolve(item.name), "Unit");
    match &item.kind {
        ItemKind::Struct(s) => {
            assert_eq!(s.kind, StructKind::Unit);
            assert!(s.fields.is_empty());
        }
        other => panic!("Expected Struct item, got {other:?}"),
    }
}
