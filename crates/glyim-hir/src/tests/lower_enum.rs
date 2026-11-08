use crate::lower::lower_crate;
use crate::{ItemId, ItemKind};
use glyim_core::interner::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

#[test]
fn test_enum_simple() {
    let source = "enum Color { Red, Green, Blue }";
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    let root = parse_result.root;
    let mut interner = Interner::new();
    let hir = lower_crate(&root, &mut interner, &mut Vec::new());

    assert_eq!(hir.items.len(), 1);
    let item = &hir.items[ItemId::from_raw(0)];
    assert_eq!(interner.resolve(item.name), "Color");
    match &item.kind {
        ItemKind::Enum(e) => {
            assert_eq!(e.variants.len(), 3);
            assert_eq!(interner.resolve(e.variants[0].name), "Red");
            assert_eq!(interner.resolve(e.variants[1].name), "Green");
            assert_eq!(interner.resolve(e.variants[2].name), "Blue");
        }
        other => panic!("Expected Enum item, got {other:?}"),
    }
}
