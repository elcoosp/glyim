#![no_main]

use libfuzzer_sys::fuzz_target;
use glyim_typeck::ty::{TyKind, TyArena};
use glyim_typeck::unify::UnificationTable;
use glyim_diag::Span;

fn generate_ty(arena: &mut TyArena, choice: u8) -> glyim_typeck::ty::Ty {
    match choice % 8 {
        0 => arena.alloc(TyKind::Int),
        1 => arena.alloc(TyKind::Bool),
        2 => arena.alloc(TyKind::Float),
        3 => arena.alloc(TyKind::Str),
        4 => arena.alloc(TyKind::Unit),
        5 => arena.fresh_infer(Span::new(0, 1)),
        6 => arena.alloc(TyKind::Error),
        _ => arena.alloc(TyKind::Never),
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let mut arena = TyArena::new();
    let mut table = UnificationTable::new();

    let a = generate_ty(&mut arena, data[0]);
    let b = generate_ty(&mut arena, data[1]);

    // Must not panic
    let _ = table.unify(&mut arena, a, b, Span::new(0, 1), &mut |_| {});
});
