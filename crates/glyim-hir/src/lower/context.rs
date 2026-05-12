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
    type_param_stack: Vec<Vec<Symbol>>,
}

impl<'a> LoweringContext<'a> {
    pub fn new(interner: &'a mut Interner) -> Self {
        Self {
            interner,
            next_id: 0,
            struct_names: HashSet::new(),
            decl_table: None,
            type_param_stack: Vec::new(),
        }
    }

    /// Create a context that also holds a pre-built declaration table.
    pub fn with_decl_table(
        interner: &'a mut Interner,
        decl_table: &'a crate::decl_table::DeclTable,
    ) -> Self {
        let mut ctx = Self {
            interner,
            next_id: 0,
            struct_names: HashSet::new(),
            decl_table: Some(decl_table),
            type_param_stack: Vec::new(),
        };
        ctx
    }

    /// Generate a fresh expression ID
    pub fn fresh_id(&mut self) -> ExprId {
        let id = ExprId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// Push a set of type parameter symbols into scope.
    pub fn push_type_params(&mut self, params: &[Symbol]) {
        eprintln!(
            "[push_type_params] params={:?}",
            params.iter().map(|s| self.resolve(*s)).collect::<Vec<_>>()
        );
        self.type_param_stack.push(params.to_vec());
    }

    /// Pop the most recently pushed type parameter scope.
    pub fn pop_type_params(&mut self) {
        eprintln!(
            "[pop_type_params] stack_depth_after={}",
            self.type_param_stack.len().saturating_sub(1)
        );
        self.type_param_stack.pop();
    }

    /// Check whether a symbol is an active type parameter.
    pub fn is_type_param(&self, sym: Symbol) -> bool {
        let top = self.type_param_stack.last();
        let is_param = top.map_or(false, |params| params.contains(&sym));
        eprintln!(
            "[is_type_param] sym={} resolved={} top_stack={:?} is_param={}",
            sym.raw(),
            self.resolve(sym),
            top.map(|p| p.iter().map(|&s| self.resolve(s)).collect::<Vec<_>>()),
            is_param
        );
        is_param
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
