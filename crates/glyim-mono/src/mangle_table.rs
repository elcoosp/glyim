use crate::mangling;
use glyim_hir::types::HirType;
use glyim_interner::{Interner, Symbol};

#[derive(Debug, Default)]
pub struct MangleTable {
    seen: Vec<bool>,
}

impl MangleTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mangle(
        &mut self,
        base: Symbol,
        args: &[HirType],
        interner: &mut Interner,
    ) -> Result<Symbol, crate::mangling::ManglingError> {
        let mangled = mangling::mangle_name(interner, base, args)?;
        self.mark_seen(mangled);
        Ok(mangled)
    }

    pub fn contains(&self, sym: Symbol) -> bool {
        let idx = sym.raw() as usize;
        idx < self.seen.len() && self.seen[idx]
    }

    pub fn mark_seen(&mut self, sym: Symbol) {
        let idx = sym.raw() as usize;
        if idx >= self.seen.len() {
            self.seen.resize(idx + 64, false);
        }
        self.seen[idx] = true;
    }

    pub fn mangle_fn(
        &mut self,
        base: Symbol,
        args: &[HirType],
        interner: &mut Interner,
    ) -> Result<Symbol, crate::mangling::ManglingError> {
        self.mangle(base, args, interner)
    }
}
