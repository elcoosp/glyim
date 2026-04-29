use crate::types::ExprId;
use glyim_interner::Interner;

/// Context shared across all lowering operations.
/// Encapsulates mutable state to avoid threading `&mut Interner` everywhere.
pub struct LoweringContext<'a> {
    pub interner: &'a mut Interner,
    next_id: u32,
}

impl<'a> LoweringContext<'a> {
    pub fn new(interner: &'a mut Interner) -> Self {
        Self {
            interner,
            next_id: 0,
        }
    }

    /// Generate a fresh expression ID
    pub fn fresh_id(&mut self) -> ExprId {
        let id = ExprId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// Intern a string and return the symbol
    pub fn intern(&mut self, s: &str) -> glyim_interner::Symbol {
        self.interner.intern(s)
    }

    /// Resolve a symbol to its string representation
    pub fn resolve(&self, sym: glyim_interner::Symbol) -> &str {
        self.interner.resolve(sym)
    }
}
