use crate::diagnostics::AutoFix;
use crate::ty::{Ty, TyArena, TyKind};
use glyim_interner::Interner;

/// Ask "What wrapper makes this compile?"
/// Takes an interner to resolve symbol names.
pub fn bi_abductive_synthesis(
    arena: &TyArena,
    interner: &Interner,
    expected: Ty,
    found: Ty,
) -> Option<AutoFix> {
    let expected_kind = arena.get(expected);

    match expected_kind {
        TyKind::App(sym, args) => {
            let name = interner.resolve(*sym);
            if name == "Option" && args.len() == 1 {
                let inner = args[0];
                if types_match(arena, inner, found) {
                    return Some(AutoFix::WrapWithOptions((0..0).into()));
                }
            }
            if name == "Result" && args.len() >= 1 {
                let inner = args[0];
                if types_match(arena, inner, found) {
                    return Some(AutoFix::WrapWithOk((0..0).into()));
                }
            }
            None
        }
        _ => None,
    }
}

fn types_match(arena: &TyArena, a: Ty, b: Ty) -> bool {
    match (arena.get(a), arena.get(b)) {
        (TyKind::Int, TyKind::Int) => true,
        (TyKind::Bool, TyKind::Bool) => true,
        (TyKind::Float, TyKind::Float) => true,
        (TyKind::Str, TyKind::Str) => true,
        (TyKind::Unit, TyKind::Unit) => true,
        (TyKind::Never, TyKind::Never) => true,
        (TyKind::Error, _) | (_, TyKind::Error) => true,
        (TyKind::Infer, _) | (_, TyKind::Infer) => true,
        (TyKind::Named(s1), TyKind::Named(s2)) => s1 == s2,
        (TyKind::App(s1, a1), TyKind::App(s2, a2)) => {
            s1 == s2
                && a1.len() == a2.len()
                && a1.iter().zip(a2).all(|(&x, &y)| types_match(arena, x, y))
        }
        (TyKind::RawPtr(i1), TyKind::RawPtr(i2)) => types_match(arena, *i1, *i2),
        (TyKind::Fn(p1, r1), TyKind::Fn(p2, r2)) => {
            p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(&a, &b)| types_match(arena, a, b))
                && types_match(arena, *r1, *r2)
        }
        _ => false,
    }
}
