/// A token proving an error was emitted. Infectious, but silently handled.
/// Unification with Error always succeeds, preventing cascading errors.
#[derive(Clone, Copy, Debug)]
pub struct ErrorGuaranteed(pub(crate) ());

use crate::ty::{Ty, TyKind, TyArena};
use glyim_diag::Span;

pub struct UnificationTable {
    parents: Vec<Ty>,
    ranks: Vec<u8>,
}

impl UnificationTable {
    pub fn new() -> Self {
        Self {
            parents: Vec::new(),
            ranks: Vec::new(),
        }
    }

    pub fn new_var(&mut self, arena: &mut TyArena, span: Span) -> Ty {
        let ty = arena.fresh_infer(span);
        self.parents.push(ty);
        self.ranks.push(0);
        ty
    }

    pub fn find(&self, arena: &TyArena, ty: Ty) -> Ty {
        match arena.get(ty) {
            TyKind::Infer | TyKind::Error if self.parents[ty.0] == ty => ty,
            TyKind::Infer => self.find(arena, self.parents[ty.0]),
            _ => ty,
        }
    }
}

impl Default for UnificationTable {
    fn default() -> Self {
        Self::new()
    }
}
