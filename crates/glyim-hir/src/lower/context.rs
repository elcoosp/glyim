use crate::decl_table::DeclTable;
use crate::types::ExprId;
use glyim_interner::Interner;
use glyim_interner::Symbol;
use std::collections::HashSet;

/// Context shared across all lowering operations.
/// Encapsulates mutable state to avoid threading `&mut Interner` everywhere.
pub struct LoweringContext<'a> {
    pub interner: &'a mut Interner,
    next_id: u32,
    pub struct_names: HashSet<Symbol>,
    pub decl_table: Option<&'a crate::decl_table::DeclTable>,
}

impl<'a> LoweringContext<'a> {
    pub fn new(interner: &'a mut Interner) -> Self {
        Self {
            interner,
            next_id: 0,
            struct_names: HashSet::new(),
            decl_table: None,
        }
    }

    /// Create a context that also holds a pre-built declaration table.
    pub fn with_decl_table(
        interner: &'a mut Interner,
        decl_table: &'a crate::decl_table::DeclTable,
    ) -> Self {
        let mut ctx = Self::new(interner);
        ctx.decl_table = Some(decl_table);
        ctx
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
