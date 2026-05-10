use crate::ty::TyArena;

/// Fuel budget for Wasm execution.
#[derive(Clone, Debug)]
pub struct FuelBudget {
    pub max_instructions: u64,
    remaining: u64,
}

impl FuelBudget {
    pub fn new(max_instructions: u64) -> Self {
        Self {
            max_instructions,
            remaining: max_instructions,
        }
    }

    /// Consume fuel. Returns true if budget was exceeded.
    pub fn consume(&mut self, amount: u64) -> bool {
        if amount > self.remaining {
            self.remaining = 0;
            true
        } else {
            self.remaining -= amount;
            false
        }
    }

    pub fn remaining(&self) -> u64 {
        self.remaining
    }

    pub fn reset(&mut self) {
        self.remaining = self.max_instructions;
    }
}

impl Default for FuelBudget {
    fn default() -> Self {
        Self::new(1_000_000)
    }
}

/// Context provided to comptime blocks and macros for querying type information.
pub struct ComptimeContext<'a> {
    pub arena: &'a TyArena,
    /// Dependencies recorded during comptime evaluation for query invalidation.
    pub dependencies: Vec<crate::queries::Dependency>,
}

impl<'a> ComptimeContext<'a> {
    pub fn new(arena: &'a TyArena) -> Self {
        Self {
            arena,
            dependencies: Vec::new(),
        }
    }

    /// Check if a trait is implemented for a type.
    pub fn trait_is_implemented(&mut self, _trait_name: &str, _type_name: &str) -> bool {
        // Record dependency for invalidation
        self.dependencies
            .push(crate::queries::Dependency::TraitImpl(
                _trait_name.to_string(),
                _type_name.to_string(),
            ));
        false // Stub – real resolution would query the CHR store
    }

    /// Get the fields of a type.
    pub fn get_fields(&mut self, _type_name: &str) -> Vec<FieldInfo> {
        self.dependencies
            .push(crate::queries::Dependency::TypeFields(
                _type_name.to_string(),
            ));
        vec![]
    }

    pub fn dependencies(&self) -> &[crate::queries::Dependency] {
        &self.dependencies
    }
}

/// Information about a field, provided to comptime/macro code.
#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub name: String,
    pub type_name: String,
    pub offset: usize,
}

/// Result of executing a comptime block or macro.
#[derive(Clone, Debug)]
pub enum ComptimeResult {
    Success {
        items: Vec<glyim_hir::HirItem>,
        fuel_used: u64,
    },
    FuelExhausted {
        fuel_used: u64,
    },
    Error(String),
}
