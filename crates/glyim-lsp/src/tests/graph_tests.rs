use crate::reference_graph::ReferenceGraph;
use glyim_core::{Interner, Visibility};
use glyim_hir::{CrateHir, FnItem, Item, ItemId, ItemKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

#[test]
fn build_from_hir_records_definitions_and_references() {
    let interner = Interner::new();
    let file_id = FileId::from_raw(1);
    let span = Span::new(
        file_id,
        ByteIdx::from_raw(0),
        ByteIdx::from_raw(10),
        SyntaxContext::ROOT,
    );
    let fn_name = interner.intern("my_function");
    let fn_item = FnItem {
        params: vec![],
        return_ty: None,
        body: None,
        is_unsafe: false,
        is_async: false,
        is_const: false,
        generic_params: vec![],
        where_clauses: vec![],
        abi: None,
        attrs: Vec::new(),
    };
    let item = Item {
        id: ItemId::from_raw(0),
        name: fn_name,
        kind: ItemKind::Fn(fn_item),
        visibility: Visibility::Public,
        span,
    };
    let hir = CrateHir {
        items: glyim_core::IndexVec::from_raw(vec![item]),
        bodies: glyim_core::IndexVec::new(),
        body_owners: glyim_core::IndexVec::new(),
        interner: Default::default(),
    };
    let mut graph = ReferenceGraph::new();
    // `build_from_hir` records top-level item names as definitions, so after
    // building the graph `my_function` must be present (as a definition).
    graph.build_from_hir(file_id, &hir, &interner);
    let name_str = interner.resolve(fn_name).to_string();
    let refs = graph.find_references(&name_str);
    assert!(
        !refs.is_empty(),
        "build_from_hir must record the top-level fn as a definition"
    );
    assert!(
        refs.iter().any(|r| r.is_definition),
        "the recorded reference must be a definition"
    );
}
