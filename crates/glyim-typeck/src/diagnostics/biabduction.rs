use crate::diagnostics::AutoFix;
use crate::ty::TyArena;
use crate::ty::Ty;

pub fn bi_abductive_synthesis(
    _arena: &TyArena,
    expected: Ty,
    found: Ty,
) -> Option<AutoFix> {
    let _ = (expected, found);
    None
}
