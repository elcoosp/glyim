use crate::{DefinitionLocation, SymbolIndex, SymbolInfo, SymbolKind};
use glyim_core::{IndexVec, Interner, Visibility};
use glyim_hir::*;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

fn create_dummy_hir(interner: &Interner) -> CrateHir {
    let mut items = IndexVec::new();
    let key = interner.intern("main");
    let name = key;
    let fn_item = Item {
        id: ItemId::from_raw(0),
        name,
        kind: ItemKind::Fn(FnItem {
            params: vec![],
            return_ty: None,
            body: None,
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params: vec![],
            where_clauses: vec![],
            abi: None,
        }),
        visibility: Visibility::Public,
        span: Span::new(
            FileId::from_raw(1),
            ByteIdx::ZERO,
            ByteIdx::from_raw(10),
            SyntaxContext::ROOT,
        ),
    };
    items.push(fn_item);
    CrateHir {
        items,
        bodies: IndexVec::new(),
        body_owners: IndexVec::new(),
        interner: Default::default(),
    }
}

#[test]
fn build_from_hir_adds_function_symbol() {
    let mut index = SymbolIndex::new();
    let file_id = FileId::from_raw(1);
    let interner = Interner::default();
    let hir = create_dummy_hir(&interner);
    index.build_from_hir(file_id, &hir, &interner);
    let symbols = index.symbols_in_file(file_id);
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "main");
    assert_eq!(symbols[0].kind, SymbolKind::Function);
}

#[test]
fn multi_file_symbols() {
    let mut index = SymbolIndex::new();
    let file1 = FileId::from_raw(10);
    let file2 = FileId::from_raw(20);

    let span1 = Span::new(
        file1,
        ByteIdx::ZERO,
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );
    let span2 = Span::new(
        file2,
        ByteIdx::ZERO,
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );

    let sym1 = SymbolInfo {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation {
            file_id: file1,
            span: span1,
        },
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    let sym2 = SymbolInfo {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation {
            file_id: file2,
            span: span2,
        },
        type_signature: None,
        is_pub: true,
        documentation: None,
    };

    index.insert_test_symbol(file1, sym1.clone());
    index.insert_test_symbol(file2, sym2.clone());

    let matches = index.lookup_by_name("foo");
    assert_eq!(matches.len(), 2);

    let file1_symbols = index.symbols_in_file(file1);
    assert_eq!(file1_symbols.len(), 1);
    assert_eq!(file1_symbols[0].definition.file_id, file1);
}

#[test]
fn query_prefix_and_contains() {
    let mut index = SymbolIndex::new();
    let file = FileId::from_raw(1);
    let span = Span::new(
        file,
        ByteIdx::ZERO,
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );

    // Add symbols including case variations
    let symbols = vec!["foo", "foobar", "bar", "baz", "FooBar"];
    for name in symbols {
        let info = SymbolInfo {
            name: name.to_string(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation {
                file_id: file,
                span,
            },
            type_signature: None,
            is_pub: true,
            documentation: None,
        };
        index.insert_test_symbol(file, info);
    }

    // Prefix query "foo" should match "foo" and "foobar" (case-sensitive)
    let prefix_matches = index.query("foo", 10);
    assert_eq!(prefix_matches.len(), 2);
    assert!(prefix_matches.iter().any(|s| s.name == "foo"));
    assert!(prefix_matches.iter().any(|s| s.name == "foobar"));

    // Contains query "bar" (prefix then contains) should match "foobar" and "bar"
    let contains_matches = index.query("bar", 10);
    assert!(contains_matches.iter().any(|s| s.name == "foobar"));
    assert!(contains_matches.iter().any(|s| s.name == "bar"));
}
