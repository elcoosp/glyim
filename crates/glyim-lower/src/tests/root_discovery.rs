use glyim_core::arena::IndexVec;
use glyim_core::interner::Interner;
use glyim_core::primitives::Visibility;
use glyim_hir::{BodyId, CrateHir, FnItem, Item, ItemId, ItemKind, Path, StaticItem, TypeRef};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::{GlyimLang, SyntaxKind, SyntaxNode};
use glyim_type::TyCtxMut;
use rowan::GreenNodeBuilder;
use rowan::Language;

use crate::discovery::discover_mono_roots;
use crate::mono::MonoItem;

fn make_name(interner: &mut Interner, s: &str) -> glyim_core::interner::Name {
    interner.intern(s)
}

fn make_hir(items: Vec<Item>) -> CrateHir {
    CrateHir {
        items: {
            let mut iv = IndexVec::new();
            for item in items {
                iv.push(item);
            }
            iv
        },
        bodies: IndexVec::new(),
        body_owners: IndexVec::new(),
        interner: Default::default(),
    }
}

fn dummy_span() -> Span {
    Span::new(
        FileId::BOGUS,
        ByteIdx::ZERO,
        ByteIdx::from_raw(1),
        SyntaxContext::ROOT,
    )
}

fn fn_item(interner: &mut Interner, id: u32, name: &str, body: Option<BodyId>) -> Item {
    Item {
        id: ItemId::from_raw(id),
        name: make_name(interner, name),
        kind: ItemKind::Fn(FnItem {
            params: vec![],
            return_ty: None,
            body,
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params: vec![],
            where_clauses: vec![],
            abi: None,
        }),
        visibility: Visibility::Public,
        span: dummy_span(),
    }
}

fn static_item(interner: &mut Interner, id: u32, name: &str) -> Item {
    Item {
        id: ItemId::from_raw(id),
        name: make_name(interner, name),
        kind: ItemKind::Static(StaticItem {
            ty: TypeRef::Path(Path::from_single(make_name(interner, "i32"))),
            body: None,
            root_expr: None,
            is_mut: false,
        }),
        visibility: Visibility::Public,
        span: dummy_span(),
    }
}

fn empty_syntax_root() -> SyntaxNode {
    let mut builder = GreenNodeBuilder::new();
    let kind = GlyimLang::kind_to_raw(SyntaxKind::SourceFile);
    builder.start_node(kind);
    builder.finish_node();
    let green = builder.finish();
    SyntaxNode::new_root(green)
}

fn make_ctx(interner: Interner) -> TyCtxMut {
    TyCtxMut::new(interner)
}

fn build_attr_syntax(attr_name: &str) -> (SyntaxNode, Span) {
    let mut builder = GreenNodeBuilder::new();
    let source_file_kind = GlyimLang::kind_to_raw(SyntaxKind::SourceFile);
    builder.start_node(source_file_kind);

    let hash_kind = GlyimLang::kind_to_raw(SyntaxKind::Hash);
    let lbracket_kind = GlyimLang::kind_to_raw(SyntaxKind::LBracket);
    let ident_kind = GlyimLang::kind_to_raw(SyntaxKind::Ident);
    let rbracket_kind = GlyimLang::kind_to_raw(SyntaxKind::RBracket);
    let kw_fn_kind = GlyimLang::kind_to_raw(SyntaxKind::KwFn);
    let lparen_kind = GlyimLang::kind_to_raw(SyntaxKind::LParen);
    let rparen_kind = GlyimLang::kind_to_raw(SyntaxKind::RParen);
    let lbrace_kind = GlyimLang::kind_to_raw(SyntaxKind::LBrace);
    let rbrace_kind = GlyimLang::kind_to_raw(SyntaxKind::RBrace);

    builder.token(hash_kind, "#");
    builder.token(lbracket_kind, "[");
    builder.token(ident_kind, attr_name);
    builder.token(rbracket_kind, "]");
    builder.token(kw_fn_kind, "fn");
    builder.token(ident_kind, "dummy");
    builder.token(lparen_kind, "(");
    builder.token(rparen_kind, ")");
    builder.token(lbrace_kind, "{");
    builder.token(rbrace_kind, "}");

    builder.finish_node();
    let green = builder.finish();
    let root = SyntaxNode::new_root(green);

    let first_tok = root.first_token().unwrap();
    let last_tok = root.last_token().unwrap();
    let start: u32 = first_tok.text_range().start().into();
    let end: u32 = last_tok.text_range().end().into();
    let span = Span::new(
        FileId::BOGUS,
        ByteIdx::from_raw(start),
        ByteIdx::from_raw(end),
        SyntaxContext::ROOT,
    );

    (root, span)
}

#[test]
fn t01_main_function_detected() {
    let mut interner = Interner::default();
    let item = fn_item(&mut interner, 0, "main", None);
    let hir = make_hir(vec![item]);
    let root = empty_syntax_root();
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], MonoItem::Fn { .. }));
    assert!(diags.is_empty());
}

#[test]
fn t02_no_entry_points_warning() {
    let mut interner = Interner::default();
    let item = fn_item(&mut interner, 0, "not_main", None);
    let hir = make_hir(vec![item]);
    let root = empty_syntax_root();
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert!(items.is_empty());
    assert!(!diags.is_empty());
}

#[test]
fn t03_multiple_entry_points() {
    let mut interner = Interner::default();
    let main_item = fn_item(&mut interner, 0, "main", None);
    let other_item = fn_item(&mut interner, 1, "other", None);
    let hir = make_hir(vec![main_item, other_item]);
    let root = empty_syntax_root();
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert_eq!(items.len(), 1);
    assert!(diags.is_empty());
}

#[test]
fn t04_static_without_used_not_detected() {
    let mut interner = Interner::default();
    let item = static_item(&mut interner, 0, "MY_STATIC");
    let hir = make_hir(vec![item]);
    let root = empty_syntax_root();
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert!(items.is_empty());
    assert!(!diags.is_empty());
}

#[test]
fn t05_fn_not_main_not_detected() {
    let mut interner = Interner::default();
    let item = fn_item(&mut interner, 0, "helper", None);
    let hir = make_hir(vec![item]);
    let root = empty_syntax_root();
    let mut ctx = make_ctx(interner);
    let (items, _) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert!(items.is_empty());
}

#[test]
fn t06_no_mangle_function_detected() {
    let mut interner = Interner::default();
    let (root, span) = build_attr_syntax("no_mangle");
    let item = Item {
        id: ItemId::from_raw(0),
        name: make_name(&mut interner, "custom_name"),
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
        span,
    };
    let hir = make_hir(vec![item]);
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert_eq!(items.len(), 1);
    assert!(diags.is_empty());
}

#[test]
fn t07_start_function_detected() {
    let mut interner = Interner::default();
    let (root, span) = build_attr_syntax("start");
    let item = Item {
        id: ItemId::from_raw(0),
        name: make_name(&mut interner, "any_name"),
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
        span,
    };
    let hir = make_hir(vec![item]);
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert_eq!(items.len(), 1);
    assert!(diags.is_empty());
}

#[test]
fn t08_used_static_detected() {
    let mut interner = Interner::default();
    let (root, span) = build_attr_syntax("used");
    let item = Item {
        id: ItemId::from_raw(0),
        name: make_name(&mut interner, "MY_STATIC"),
        kind: ItemKind::Static(StaticItem {
            ty: TypeRef::Path(Path::from_single(make_name(&mut interner, "i32"))),
            body: None,
            root_expr: None,
            is_mut: false,
        }),
        visibility: Visibility::Public,
        span,
    };
    let hir = make_hir(vec![item]);
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], MonoItem::Static { .. }));
    assert!(diags.is_empty());
}

#[test]
fn t09_unknown_attribute_not_detected() {
    let mut interner = Interner::default();
    let (root, span) = build_attr_syntax("unknown");
    let item = Item {
        id: ItemId::from_raw(0),
        name: make_name(&mut interner, "f"),
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
        span,
    };
    let hir = make_hir(vec![item]);
    let mut ctx = make_ctx(interner);
    let (items, diags) = discover_mono_roots(&root, &hir, None, &mut ctx);
    assert!(items.is_empty());
    assert!(!diags.is_empty());
}
