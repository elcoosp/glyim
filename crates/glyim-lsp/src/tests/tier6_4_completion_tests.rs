//! Tier 6.4: LSP-side receiver-type completion filtering.
//!
//! `s12_impl_methods_indexed_with_receiver_type` runs the real parser + HIR
//! pipeline to confirm `impl` methods are indexed with their receiver type.
//!
//! `s12_dot_completion_filters_by_receiver_type` drives the completion filter
//! through a self-consistent `CrateHir` whose spans live in the *same
//! coordinate space* as the source string (the receiver `x` of `x.` is typed
//! `i32`, matching the `impl i32` methods). This verifies the receiver-type
//! filter end-to-end without depending on the parser's own span emission.

use crate::completion::provide_completions;
use crate::database::{AnalysisDatabase, SourceMap};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::Name;
use glyim_core::primitives::IntTy;
use glyim_core::Visibility;
use glyim_hir::{
    Body, CrateHir, Expr, ExprId, FnItem, ImplItem, ImplMethod, Item, ItemId, ItemKind, Path,
    TypeRef,
};
use glyim_span::{ByteIdx, Span, SyntaxContext};
use glyim_type::{Ty, TyCtxMut, TyKind};
use glyim_typeck::TypeckResult;
use lsp_types::{
    CompletionContext, CompletionParams, CompletionResponse, CompletionTriggerKind, Position,
    TextDocumentIdentifier, TextDocumentPositionParams, Uri,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

fn completion_params(path: &PathBuf, line: u32, character: u32) -> CompletionParams {
    CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_for(path) },
            position: Position { line, character },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
            trigger_character: Some(".".to_string()),
        }),
    }
}

fn line_col_of(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[test]
fn s12_impl_methods_indexed_with_receiver_type() {
    let source = "struct Foo {}\nimpl Foo {\n    fn ping(&self) {}\n    fn pong(&self) {}\n}\n";
    let db = AnalysisDatabase::new();
    let path = PathBuf::from("/test/receiver.g");
    let file_id = db.file_map.write().get_or_create(&path);
    db.source_maps.write().insert(
        file_id,
        SourceMap::new(path.clone(), file_id, source.to_string()),
    );
    let crate_id = glyim_core::CrateId::from_raw(0);
    let parse_result = glyim_frontend::parse_to_syntax(source, file_id);
    let mut interner = glyim_core::Interner::new();
    let def_map = glyim_def_map::build_def_map(&parse_result.root, crate_id, interner.clone()).0;
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    let ty_ctx_mut = glyim_type::TyCtxMut::new(interner.clone());
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = glyim_solve::SimpleTraitSolver::new(&trait_ctx);
    let (ty_ctx, typeck_result) =
        glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.hirs.write().insert(file_id, hir);
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck_result));

    let guard = db.symbol_index.read();
    let methods: Vec<_> = guard
        .symbols_in_file(file_id)
        .into_iter()
        .filter(|s| s.name == "ping" || s.name == "pong")
        .collect();
    assert_eq!(methods.len(), 2, "expected both impl methods to be indexed");
    for m in &methods {
        assert_eq!(
            m.type_signature
                .as_ref()
                .and_then(|ts| ts.receiver_type.clone()),
            Some("Foo".to_string()),
            "impl method should carry receiver type `Foo`"
        );
    }
}

#[test]
fn s12_dot_completion_filters_by_receiver_type() {
    let source = "\
fn unrelated() {}
impl i32 {
    fn ping(&self) {}
    fn other(&self) {}
}
fn main() {
    let x = 1 + 2;
    x.
}
";
    let dot_pos = source.find("x.").expect("`x.` present");
    let offset = dot_pos + "x.".len(); // cursor right after the `.`
    let (line, character) = line_col_of(source, offset);

    let db = AnalysisDatabase::new();
    let path = PathBuf::from("/test/receiver.g");
    let file_id = db.file_map.write().get_or_create(&path);
    db.source_maps.write().insert(
        file_id,
        SourceMap::new(path.clone(), file_id, source.to_string()),
    );

    let interner = glyim_core::Interner::new();
    let i32_name = interner.intern("i32");
    let x_name = interner.intern("x");
    let ping_name = interner.intern("ping");
    let other_name = interner.intern("other");
    let unrelated_name = interner.intern("unrelated");
    let main_name = interner.intern("main");

    // main body: receiver `x` (ExprId 0) + MethodCall { receiver: x } (ExprId 1).
    let receiver_id = ExprId::from_raw(0);
    let main_owner = LocalDefId::from_raw(0);
    let main_body_id = glyim_hir::BodyId::from_raw(0);
    let ctx = SyntaxContext::ROOT;
    let span_for = |lo: usize, hi: usize| {
        Span::new(
            file_id,
            ByteIdx::from_raw(lo as u32),
            ByteIdx::from_raw(hi as u32),
            ctx,
        )
    };
    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    exprs.push(Expr::Path(Path::from_single(x_name)));
    exprs.push(Expr::MethodCall {
        receiver: receiver_id,
        method: ping_name,
        args: vec![],
    });
    let mut expr_spans: IndexVec<ExprId, Span> = IndexVec::new();
    expr_spans.push(span_for(dot_pos, dot_pos + 1)); // receiver `x`
    expr_spans.push(span_for(dot_pos, dot_pos + 2)); // method-call span covers the `.`
    let body = Body {
        owner: main_owner,
        exprs,
        pats: IndexVec::new(),
        params: vec![],
        span: span_for(0, source.len()),
        expr_spans,
    };

    let mut bodies: IndexVec<glyim_hir::BodyId, Body> = IndexVec::new();
    bodies.push(body);
    let mut body_owners: IndexVec<glyim_hir::BodyId, LocalDefId> = IndexVec::new();
    body_owners.push(main_owner);

    // Items: one `impl i32`, `unrelated`, `main`. `build_from_hir` indexes the
    // impl methods with receiver `i32`, and the free functions with no receiver.
    let mut items: IndexVec<ItemId, Item> = IndexVec::new();
    let impl_item = Item {
        id: ItemId::from_raw(0),
        name: i32_name,
        kind: ItemKind::Impl(ImplItem {
            trait_ref: None,
            self_ty: TypeRef::Path(Path::from_single(i32_name)),
            methods: vec![
                ImplMethod {
                    name: ping_name,
                    body: None,
                    params: vec![],
                    generic_params: vec![],
                    return_ty: None,
                },
                ImplMethod {
                    name: other_name,
                    body: None,
                    params: vec![],
                    generic_params: vec![],
                    return_ty: None,
                },
            ],
            generic_params: vec![],
            where_clauses: vec![],
            associated_types: vec![],
        }),
        visibility: Visibility::Inherited,
        span: span_for(0, source.len()),
    };
    let fn_item = |name: Name| Item {
        id: ItemId::from_raw(0),
        name,
        kind: ItemKind::Fn(FnItem {
            params: vec![],
            return_ty: None,
            body: Some(main_body_id),
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params: vec![],
            where_clauses: vec![],
        abi: None,}),
        visibility: Visibility::Inherited,
        span: span_for(0, source.len()),
    };
    items.push(impl_item);
    items.push(fn_item(unrelated_name));
    items.push(fn_item(main_name));

    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };
    db.hirs.write().insert(file_id, hir.clone());
    db.symbol_index.write().build_from_hir(file_id, &hir, &interner);

    // Type the receiver `x` as `i32`.
    let mut ty_ctx_mut = TyCtxMut::new(interner.clone());
    let i32_ty: Ty = ty_ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ty_ctx = ty_ctx_mut.freeze();
    let mut type_map: HashMap<ExprId, Ty> = HashMap::new();
    type_map.insert(receiver_id, i32_ty);
    let mut expr_types: HashMap<LocalDefId, HashMap<ExprId, Ty>> = HashMap::new();
    expr_types.insert(main_owner, type_map);
    let typeck = TypeckResult {
        thir_bodies: vec![],
        diagnostics: vec![],
        const_values: std::collections::HashMap::new(),
        expr_types,
    };
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck));

    // Sanity: the receiver-aware `type_at_offset` resolves `i32`.
    let tc = db.ty_ctx(file_id);
    let resolved = db.type_at_offset(file_id, offset);
    assert_eq!(
        resolved.as_ref().map(|t| format!(
            "{}",
            glyim_type::PrintTy::new(*t, tc.as_ref().unwrap().as_ref())
        )),
        Some("i32".to_string()),
        "receiver `x` before the `.` should resolve to `i32`"
    );

    let params = completion_params(&path, line, character);
    let file_map_guard = db.file_map.read();
    let result = provide_completions(&db, &file_map_guard, &params);
    drop(file_map_guard);
    let items = match result {
        Some(CompletionResponse::List(list)) => list.items,
        other => panic!("expected a completion list, got {:?}", other),
    };
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"ping"),
        "expected `ping` (receiver i32) in completions, got {:?}",
        labels
    );
    assert!(
        labels.contains(&"other"),
        "expected `other` (receiver i32) in completions, got {:?}",
        labels
    );
    assert!(
        !labels.contains(&"unrelated"),
        "free function `unrelated` must be filtered out at a `.` call site, got {:?}",
        labels
    );
}

/// Tier 6.4 (Plan §8.2, report #27): trait-impl method completion.
///
/// `build_from_hir` indexes *every* `ItemKind::Impl`, including `impl Trait for
/// T` (where `trait_ref` is `Some`). Such methods must be offered at a `.`
/// call site whose receiver resolves to `T`, exactly like inherent-impl
/// methods. This test proves the `trait_ref: Some` indexing path participates
/// in receiver-type completion rather than being skipped.
#[test]
fn s12_trait_impl_methods_completed() {
    let source = "\
trait Show { fn show(&self); }
impl Show for i32 {
    fn show(&self) {}
}
fn main() {
    let x = 1 + 2;
    x.
}
";
    let dot_pos = source.find("x.").expect("`x.` present");
    let offset = dot_pos + "x.".len();
    let (line, character) = line_col_of(source, offset);

    let db = AnalysisDatabase::new();
    let path = PathBuf::from("/test/trait_impl.g");
    let file_id = db.file_map.write().get_or_create(&path);
    db.source_maps.write().insert(
        file_id,
        SourceMap::new(path.clone(), file_id, source.to_string()),
    );

    let interner = glyim_core::Interner::new();
    let i32_name = interner.intern("i32");
    let show_name = interner.intern("Show");
    let method_name = interner.intern("show");
    let main_name = interner.intern("main");

    let receiver_id = ExprId::from_raw(0);
    let main_owner = LocalDefId::from_raw(0);

    let ctx = SyntaxContext::ROOT;
    let span_for = |lo: usize, hi: usize| {
        Span::new(
            file_id,
            ByteIdx::from_raw(lo as u32),
            ByteIdx::from_raw(hi as u32),
            ctx,
        )
    };
    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    exprs.push(Expr::Path(Path::from_single(i32_name)));
    exprs.push(Expr::MethodCall {
        receiver: receiver_id,
        method: method_name,
        args: vec![],
    });
    let mut expr_spans: IndexVec<ExprId, Span> = IndexVec::new();
    expr_spans.push(span_for(dot_pos, dot_pos + 1));
    expr_spans.push(span_for(dot_pos, dot_pos + 2));
    let body = Body {
        owner: main_owner,
        exprs,
        pats: IndexVec::new(),
        params: vec![],
        span: span_for(0, source.len()),
        expr_spans,
    };
    let mut bodies: IndexVec<glyim_hir::BodyId, Body> = IndexVec::new();
    bodies.push(body);
    let mut body_owners: IndexVec<glyim_hir::BodyId, LocalDefId> = IndexVec::new();
    body_owners.push(main_owner);

    let mut items: IndexVec<ItemId, Item> = IndexVec::new();
    let trait_impl_item = Item {
        id: ItemId::from_raw(0),
        name: i32_name,
        kind: ItemKind::Impl(ImplItem {
            trait_ref: Some(Path::from_single(show_name)),
            self_ty: TypeRef::Path(Path::from_single(i32_name)),
            methods: vec![ImplMethod {
                name: method_name,
                body: None,
                params: vec![],
                generic_params: vec![],
                return_ty: None,
            }],
            generic_params: vec![],
            where_clauses: vec![],
            associated_types: vec![],
        }),
        visibility: Visibility::Inherited,
        span: span_for(0, source.len()),
    };
    items.push(trait_impl_item);
    items.push(Item {
        id: ItemId::from_raw(1),
        name: main_name,
        kind: ItemKind::Fn(FnItem {
            params: vec![],
            return_ty: None,
            body: Some(glyim_hir::BodyId::from_raw(0)),
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params: vec![],
            where_clauses: vec![],
            abi: None,
        }),
        visibility: Visibility::Inherited,
        span: span_for(0, source.len()),
    });

    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };
    db.hirs.write().insert(file_id, hir.clone());
    db.symbol_index.write().build_from_hir(file_id, &hir, &interner);

    // Type the receiver `x` as `i32`.
    let mut ty_ctx_mut = TyCtxMut::new(interner.clone());
    let i32_ty: Ty = ty_ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32));
    let ty_ctx = ty_ctx_mut.freeze();
    let mut type_map: HashMap<ExprId, Ty> = HashMap::new();
    type_map.insert(receiver_id, i32_ty);
    let mut expr_types: HashMap<LocalDefId, HashMap<ExprId, Ty>> = HashMap::new();
    expr_types.insert(main_owner, type_map);
    let typeck = TypeckResult {
        thir_bodies: vec![],
        diagnostics: vec![],
        const_values: std::collections::HashMap::new(),
        expr_types,
    };
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck));

    let params = completion_params(&path, line, character);
    let file_map_guard = db.file_map.read();
    let result = provide_completions(&db, &file_map_guard, &params);
    drop(file_map_guard);
    let items = match result {
        Some(CompletionResponse::List(list)) => list.items,
        other => panic!("expected a completion list, got {:?}", other),
    };
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"show"),
        "expected trait-impl method `show` (receiver i32) in completions, got {:?}",
        labels
    );
}
