use crate::symbol_index::{DefinitionLocation, SymbolIndex, SymbolInfo, SymbolKind, TypeSignature};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};

#[test]
fn test_lookup_by_name_returns_correct_symbol() {
    let mut index = SymbolIndex::new();
    let file_id = FileId::from_raw(1);
    let span = Span::new(
        file_id,
        ByteIdx::ZERO,
        ByteIdx::from_raw(10),
        SyntaxContext::ROOT,
    );
    let def_loc = DefinitionLocation { file_id, span };
    let info = SymbolInfo {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        definition: def_loc,
        type_signature: Some(TypeSignature {
            params: vec![("x".to_string(), "i32".to_string())],
            receiver_type: None,
            return_type: Some("i32".to_string()),
            generic_params: vec![],
        }),
        is_pub: true,
        documentation: None,
    };
    index.insert_test_symbol(file_id, info.clone());

    let found = index.lookup_by_name("foo");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "foo");
    assert_eq!(found[0].kind, SymbolKind::Function);
    assert!(found[0].type_signature.is_some());

    let not_found = index.lookup_by_name("bar");
    assert_eq!(not_found.len(), 0);
}

#[test]
fn test_query_prefix_returns_matching_symbols() {
    let mut index = SymbolIndex::new();
    let file_id = FileId::from_raw(1);
    let span = Span::new(
        file_id,
        ByteIdx::ZERO,
        ByteIdx::from_raw(10),
        SyntaxContext::ROOT,
    );
    let def_loc = DefinitionLocation { file_id, span };

    let foo = SymbolInfo {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        definition: def_loc.clone(),
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    let foobar = SymbolInfo {
        name: "foobar".to_string(),
        kind: SymbolKind::Function,
        definition: def_loc.clone(),
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    let bar = SymbolInfo {
        name: "bar".to_string(),
        kind: SymbolKind::Function,
        definition: def_loc,
        type_signature: None,
        is_pub: true,
        documentation: None,
    };

    index.insert_test_symbol(file_id, foo);
    index.insert_test_symbol(file_id, foobar);
    index.insert_test_symbol(file_id, bar);

    let matches = index.query("foo", 10);
    assert_eq!(matches.len(), 2);
    // Collect names and sort to avoid order dependency
    let mut names: Vec<&str> = matches.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["foo", "foobar"]);

    let matches_limit = index.query("foo", 1);
    // The limit of 1 may return either "foo" or "foobar", but we only care that length is 1
    assert_eq!(matches_limit.len(), 1);
    // Optional: verify it's one of the two
    assert!(matches_limit[0].name == "foo" || matches_limit[0].name == "foobar");
}

#[test]
fn test_query_fallback_contains() {
    let mut index = SymbolIndex::new();
    let file_id = FileId::from_raw(1);
    let span = Span::new(
        file_id,
        ByteIdx::ZERO,
        ByteIdx::from_raw(10),
        SyntaxContext::ROOT,
    );
    let def_loc = DefinitionLocation { file_id, span };
    let abc = SymbolInfo {
        name: "abc".to_string(),
        kind: SymbolKind::Function,
        definition: def_loc,
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    index.insert_test_symbol(file_id, abc);
    let matches = index.query("bc", 10);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "abc");
}
