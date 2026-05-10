use crate::ty::{Ty, TyArena, TyKind};

/// Lockstep structural traversal to find the exact point of divergence.
/// Returns `None` if the types are structurally identical.
/// Returns `Some(path)` describing where they diverge.
pub fn zip_diff(arena: &TyArena, t1: Ty, t2: Ty, path: String) -> Option<String> {
    let k1 = arena.get(t1);
    let k2 = arena.get(t2);

    match (k1, k2) {
        (TyKind::Int, TyKind::Int) => None,
        (TyKind::Bool, TyKind::Bool) => None,
        (TyKind::Float, TyKind::Float) => None,
        (TyKind::Str, TyKind::Str) => None,
        (TyKind::Unit, TyKind::Unit) => None,
        (TyKind::Never, TyKind::Never) => None,
        (TyKind::Error, TyKind::Error) => None,

        (TyKind::Named(s1), TyKind::Named(s2)) if s1 == s2 => None,

        (TyKind::App(s1, a1), TyKind::App(s2, a2)) if s1 == s2 && a1.len() == a2.len() => a1
            .iter()
            .zip(a2.iter())
            .enumerate()
            .find_map(|(i, (a, b))| zip_diff(arena, *a, *b, format!("{path}.T{i}"))),

        (TyKind::Fn(p1, r1), TyKind::Fn(p2, r2)) if p1.len() == p2.len() => {
            for (i, (pa, pb)) in p1.iter().zip(p2.iter()).enumerate() {
                if let Some(diff) = zip_diff(arena, *pa, *pb, format!("{path}.param{i}")) {
                    return Some(diff);
                }
            }
            zip_diff(arena, *r1, *r2, format!("{path}.ret"))
        }

        (TyKind::RawPtr(i1), TyKind::RawPtr(i2)) => zip_diff(arena, *i1, *i2, format!("{path}.*")),

        (TyKind::Infer, _) | (_, TyKind::Infer) => None,
        (TyKind::Error, _) | (_, TyKind::Error) => None,

        _ => Some(format!(
            "Diverged at {path}: expected {:?}, found {:?}",
            k1, k2
        )),
    }
}
