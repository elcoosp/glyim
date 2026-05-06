use crate::dispatch::DispatchTable;
use dashmap::DashMap;
use glyim_interner::Symbol;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExecutionTier {
    Tier0,
    Tier1,
}

pub struct TieredCompiler {
    dispatch: Arc<DispatchTable>,
    execution_counts: DashMap<Symbol, u64>,
    tiers: DashMap<Symbol, ExecutionTier>,
    promotion_threshold: u64,
}

impl TieredCompiler {
    pub fn new(dispatch: Arc<DispatchTable>, promotion_threshold: u64) -> Self {
        Self { dispatch, execution_counts: DashMap::new(), tiers: DashMap::new(), promotion_threshold }
    }

    pub fn record_execution(&self, sym: Symbol) {
        let mut count_mut = self.execution_counts.entry(sym).or_insert(0);
        *count_mut.value_mut() += 1;
        if *count_mut.value() >= self.promotion_threshold {
            self.tiers.insert(sym, ExecutionTier::Tier1);
        }
    }

    pub fn execution_tier(&self, sym: Symbol) -> ExecutionTier {
        self.tiers.get(&sym).map(|t| *t.value()).unwrap_or(ExecutionTier::Tier0)
    }

    pub fn execution_count(&self, sym: Symbol) -> u64 {
        self.execution_counts.get(&sym).map(|c| *c.value()).unwrap_or(0)
    }

    pub fn promote(&self, sym: Symbol) {
        self.tiers.insert(sym, ExecutionTier::Tier1);
    }

    pub fn promote_all(&self) -> Vec<Symbol> {
        let mut promoted = Vec::new();
        for entry in self.execution_counts.iter() {
            let sym = *entry.key();
            if self.execution_tier(sym) == ExecutionTier::Tier0 {
                self.tiers.insert(sym, ExecutionTier::Tier1);
                promoted.push(sym);
            }
        }
        promoted
    }

    pub fn reset_tier(&self, sym: Symbol) {
        self.tiers.insert(sym, ExecutionTier::Tier0);
        self.execution_counts.insert(sym, 0);
    }

    pub fn dispatch(&self) -> &DispatchTable {
        &self.dispatch
    }
}
