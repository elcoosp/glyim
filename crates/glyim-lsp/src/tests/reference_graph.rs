use crate::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

#[test]
fn test_insert_and_find() {
    let mut graph = ReferenceGraph::new();
    let file_id = FileId::from_raw(1);

    let ref1 = Reference {
        file_id,
        span: Span::new(
            file_id,
            ByteIdx::from_raw(0),
            ByteIdx::from_raw(3),
            SyntaxContext::ROOT,
        ),
        is_definition: true,
        kind: ReferenceKind::Call,
        access: AccessKind::Read,

        def_id: None,

        owner_item_id: None,
            is_item_level: false,
            };

    graph.insert_test_reference("my_func", ref1);

    let refs = graph.find_references("my_func");
    assert_eq!(refs.len(), 1);
    assert!(refs[0].is_definition);
    assert_eq!(refs[0].kind, ReferenceKind::Call);
}

#[test]
fn test_empty_graph() {
    let graph = ReferenceGraph::new();
    assert!(graph.find_references("non_existent").is_empty());
}
