//! Tests for expression lowering.

#![allow(unused_imports, dead_code)]

use crate::{Body, Expr, ExprId, Pat, TypeRef};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::Interner;
use glyim_span::Span;
use glyim_syntax::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

use crate::lower::lower_expr::lower_expr as lower_expr_fn;
use crate::lower::{is_expr_node, lower_expr};
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

fn parse_expr(src: &str) -> SyntaxNode {
    let full_src = format!("fn main() {{ {} }}", src);
    let parse = parse_to_syntax(&full_src, FileId::from_raw(1));
    let fn_def = parse
        .root
        .children()
        .find(|n| n.kind() == SyntaxKind::FnDef)
        .expect("FnDef not found");
    let block = fn_def
        .children()
        .find(|n| n.kind() == SyntaxKind::Block)
        .expect("Block not found");
    block
        .children()
        .find(is_expr_node)
        .or_else(|| {
            block
                .children()
                .find(|n| n.kind() == SyntaxKind::ExprStmt)
                .and_then(|stmt| stmt.children().find(is_expr_node))
        })
        .expect("expr node not found")
        .clone()
}

fn make_body() -> Body {
    Body {
        owner: LocalDefId::from_raw(0),
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    }
}

#[test]
fn test_lower_closure_expr() {
    let node = parse_expr("|x: i32| x + 1");
    let mut interner = Interner::default();
    let mut body = make_body();
    let eid = lower_expr_fn(
        &node,
        &mut interner,
        &mut body,
        &mut Vec::new(),
        &HashMap::new(),
    )
    .unwrap();
    match &body.exprs[eid] {
        Expr::Closure {
            params,
            body: _,
            is_move: _,
        } => {
            assert_eq!(params.len(), 1);
            let pat = &body.pats[params[0]];
            match pat {
                Pat::Binding { name, .. } => assert_eq!(interner.resolve(*name), "x"),
                Pat::Path(path) => assert_eq!(path.as_name(), Some(interner.intern("x"))),
                Pat::Wild => {}
                _ => panic!("unexpected pattern {:?}", pat),
            }
        }
        _ => panic!("expected Closure expr"),
    }
}

#[test]
fn test_lower_struct_expr() {
    let node = parse_expr("Point { x, y: 20 }");
    let mut interner = Interner::default();
    let mut body = make_body();
    let eid = lower_expr_fn(
        &node,
        &mut interner,
        &mut body,
        &mut Vec::new(),
        &HashMap::new(),
    )
    .unwrap();
    match &body.exprs[eid] {
        Expr::Struct {
            path,
            fields,
            spread,
        } => {
            if !fields.is_empty() {
                assert_eq!(path.as_name(), Some(interner.intern("Point")));
                assert_eq!(fields.len(), 2);
                let names: Vec<&str> = fields.iter().map(|(n, _)| interner.resolve(*n)).collect();
                assert!(names.contains(&"x") && names.contains(&"y"));
                assert!(spread.is_none());
            }
        }
        _ => panic!("expected Struct expr"),
    }
}

#[test]
fn test_lower_range_expr() {
    let node = parse_expr("1..10");
    let mut interner = Interner::default();
    let mut body = make_body();
    let eid = lower_expr_fn(
        &node,
        &mut interner,
        &mut body,
        &mut Vec::new(),
        &HashMap::new(),
    )
    .unwrap();
    match &body.exprs[eid] {
        Expr::Range {
            start,
            end,
            inclusive,
        } => {
            assert!(start.is_some());
            assert!(end.is_some());
            assert!(!inclusive);
        }
        _ => panic!("expected Range expr"),
    }
}

#[test]
fn test_lower_index_expr() {}

#[test]
fn test_lower_cast_expr() {
    let node = parse_expr("x as i32");
    let mut interner = Interner::default();
    let mut body = make_body();
    let eid = lower_expr_fn(
        &node,
        &mut interner,
        &mut body,
        &mut Vec::new(),
        &HashMap::new(),
    )
    .unwrap();
    match &body.exprs[eid] {
        Expr::Cast { expr: _, ty } => match ty {
            TypeRef::Path(p) => assert_eq!(p.as_name(), Some(interner.intern("i32"))),
            _ => panic!("expected Path type"),
        },
        _ => panic!("expected Cast expr"),
    }
}
