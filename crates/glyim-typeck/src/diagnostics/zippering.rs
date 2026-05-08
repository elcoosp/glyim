use crate::ty::{Ty, TyKind, TyArena};

/// Lockstep structural traversal to find the exact point of divergence.
pub fn zip_diff(
    arena: &TyArena,
    t1: Ty,
    t2: Ty,
    path: String,
) -> Option<String> {
    match (arena.get(t1), arena.get(t2)) {
        (TyKind::App(s1, a1), TyKind::App(s2, a2)) if s1 == s2 && a1.len() == a2.len() => {
            a1.iter().zip(a2).enumerate().find_map(|(i, (a, b))| {
                zip_diff(arena, *a, *b, format!("{path}.T{i}"))
            })
        }
        _ if t1 != t2 => Some(format!("Diverged at {path}")),
        _ => None,
    }
}
