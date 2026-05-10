use crate::ty::{Ty, TyKind, TyArena};
use glyim_interner::Interner;
use glyim_diag::Span;

#[test]
fn ty_kind_primitives_exist() {
    assert!(matches!(TyKind::Int, TyKind::Int));
    assert!(matches!(TyKind::Float, TyKind::Float));
    assert!(matches!(TyKind::Bool, TyKind::Bool));
    assert!(matches!(TyKind::Str, TyKind::Str));
    assert!(matches!(TyKind::Unit, TyKind::Unit));
    assert!(matches!(TyKind::Never, TyKind::Never));
    assert!(matches!(TyKind::Error, TyKind::Error));
    assert!(matches!(TyKind::Infer, TyKind::Infer));
}

#[test]
fn ty_kind_nominal() {
    let mut interner = Interner::new();
    let sym = interner.intern("MyStruct");
    let named = TyKind::Named(sym);
    assert!(matches!(named, TyKind::Named(s) if s == sym));
}

#[test]
fn ty_kind_generic() {
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let inner = Ty(0);
    let app = TyKind::App(vec_sym, vec![inner]);
    if let TyKind::App(s, args) = app {
        assert_eq!(s, vec_sym);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], Ty(0));
    } else {
        panic!("Expected App");
    }
}

#[test]
fn ty_kind_function() {
    let param = Ty(0);
    let ret = Ty(1);
    let fn_kind = TyKind::Fn(vec![param], ret);
    if let TyKind::Fn(params, r) = fn_kind {
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], Ty(0));
        assert_eq!(r, Ty(1));
    } else {
        panic!("Expected Fn");
    }
}

#[test]
fn ty_kind_raw_ptr() {
    let inner = Ty(0);
    let ptr = TyKind::RawPtr(inner);
    assert!(matches!(ptr, TyKind::RawPtr(t) if t == Ty(0)));
}

#[test]
fn ty_is_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<Ty>();
}

#[test]
fn ty_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<Ty>();
}

#[test]
fn ty_arena_alloc_int() {
    let mut arena = TyArena::new();
    let ty = arena.alloc(TyKind::Int);
    assert!(matches!(arena.get(ty), TyKind::Int));
}

#[test]
fn ty_arena_alloc_named() {
    let mut arena = TyArena::new();
    let mut interner = Interner::new();
    let sym = interner.intern("Foo");
    let ty = arena.alloc(TyKind::Named(sym));
    assert!(matches!(arena.get(ty), TyKind::Named(s) if *s == sym));
}

#[test]
fn ty_arena_alloc_app() {
    let mut arena = TyArena::new();
    let inner = arena.alloc(TyKind::Int);
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let ty = arena.alloc(TyKind::App(vec_sym, vec![inner]));
    if let TyKind::App(s, args) = arena.get(ty) {
        assert_eq!(*s, vec_sym);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], inner);
    } else {
        panic!("Expected App");
    }
}

#[test]
fn ty_arena_fresh_infer() {
    let mut arena = TyArena::new();
    let span = Span::new(10, 20);
    let ty = arena.fresh_infer(span);
    assert!(matches!(arena.get(ty), TyKind::Infer));
    assert_eq!(arena.get_infer_span(ty), Some(span));
}

#[test]
fn ty_arena_infer_span_none_for_concrete() {
    let mut arena = TyArena::new();
    let ty = arena.alloc(TyKind::Int);
    assert_eq!(arena.get_infer_span(ty), None);
}

#[test]
fn ty_arena_multiple_allocs() {
    let mut arena = TyArena::new();
    let t0 = arena.alloc(TyKind::Int);
    let t1 = arena.alloc(TyKind::Bool);
    let t2 = arena.fresh_infer(Span::new(0, 1));
    assert!(matches!(arena.get(t0), TyKind::Int));
    assert!(matches!(arena.get(t1), TyKind::Bool));
    assert!(matches!(arena.get(t2), TyKind::Infer));
    assert_ne!(t0, t1);
    assert_ne!(t1, t2);
    assert_ne!(t0, t2);
}

#[test]
fn ty_arena_format_int() {
    let s = format!("{:?}", TyKind::Int);
    assert!(s.contains("Int"));
}
