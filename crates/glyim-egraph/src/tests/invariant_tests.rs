use crate::invariant::InvariantCertificate;
use glyim_diag::Span;
use glyim_hir::node::{HirExpr, HirFn};
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;

#[test]
fn certificate_deterministic() {
    let mut interner = Interner::new();
    let name = interner.intern("add");
    let hir_fn = HirFn {
        doc: None,
        name,
        type_params: vec![],
        params: vec![(interner.intern("x"), HirType::Int), (interner.intern("y"), HirType::Int)],
        param_mutability: vec![false, false],
        ret: Some(HirType::Int),
        body: HirExpr::IntLit { id: ExprId::new(0), value: 42, span: Span::new(0, 0) },
        span: Span::new(0, 0),
        is_pub: false,
        is_macro_generated: false,
        is_extern_backed: false,
    };
    let types = vec![HirType::Int];
    let cert1 = InvariantCertificate::compute(&hir_fn, &interner, &types);
    let cert2 = InvariantCertificate::compute(&hir_fn, &interner, &types);
    assert_eq!(cert1, cert2);
}

#[test]
fn certificate_different_functions_different_hash() {
    let mut interner = Interner::new();
    let fn_a = HirFn {
        doc: None,
        name: interner.intern("a"),
        type_params: vec![],
        params: vec![(interner.intern("x"), HirType::Int)],
        param_mutability: vec![false],
        ret: Some(HirType::Int),
        body: HirExpr::IntLit { id: ExprId::new(0), value: 1, span: Span::new(0, 0) },
        span: Span::new(0, 0),
        is_pub: false, is_macro_generated: false, is_extern_backed: false,
    };
    let fn_b = HirFn {
        name: interner.intern("b"),
        body: HirExpr::IntLit { id: ExprId::new(0), value: 2, span: Span::new(0, 0) },
        ..fn_a.clone()
    };
    let types = vec![HirType::Int];
    let cert_a = InvariantCertificate::compute(&fn_a, &interner, &types);
    let cert_b = InvariantCertificate::compute(&fn_b, &interner, &types);
    assert_ne!(cert_a.signature_hash, cert_b.signature_hash);
}

#[test]
fn certificate_serialization_roundtrip() {
    let mut interner = Interner::new();
    let hir_fn = HirFn {
        doc: None, name: interner.intern("test"), type_params: vec![],
        params: vec![(interner.intern("x"), HirType::Int)],
        param_mutability: vec![false], ret: Some(HirType::Int),
        body: HirExpr::IntLit { id: ExprId::new(0), value: 99, span: Span::new(0, 0) },
        span: Span::new(0, 0), is_pub: false, is_macro_generated: false, is_extern_backed: false,
    };
    let cert = InvariantCertificate::compute(&hir_fn, &interner, &[]);
    let bytes = cert.to_bytes();
    let restored = InvariantCertificate::from_bytes(&bytes).unwrap();
    assert_eq!(cert, restored);
}

#[test]
fn certificate_content_hash_is_deterministic() {
    let mut interner = Interner::new();
    let hir_fn = HirFn {
        doc: None, name: interner.intern("hash_test"), type_params: vec![],
        params: vec![(interner.intern("x"), HirType::Int)],
        param_mutability: vec![false], ret: Some(HirType::Int),
        body: HirExpr::IntLit { id: ExprId::new(0), value: 42, span: Span::new(0, 0) },
        span: Span::new(0, 0), is_pub: false, is_macro_generated: false, is_extern_backed: false,
    };
    let cert = InvariantCertificate::compute(&hir_fn, &interner, &[]);
    let h1 = cert.content_hash();
    let h2 = cert.content_hash();
    assert_eq!(h1, h2);
}
