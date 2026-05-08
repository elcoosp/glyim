use crate::dispatch::DispatchTable;
use glyim_interner::Symbol;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct StagingArea {
    items: Vec<Symbol>,
}

pub struct DoubleBufferedJIT {
    dispatch: Arc<DispatchTable>,
    staging: StagingArea,
}

impl DoubleBufferedJIT {
    pub fn new(dispatch: Arc<DispatchTable>) -> Self {
        Self { dispatch, staging: StagingArea::default() }
    }

    pub fn stage_item(&mut self, sym: Symbol) {
        self.staging.items.push(sym);
    }

    pub fn staged_count(&self) -> usize {
        self.staging.items.len()
    }

    pub fn staged_items(&self) -> &[Symbol] {
        &self.staging.items
    }

    pub fn commit(&mut self) {
        // For now, just clear staging; full compilation in integration chunk
        self.staging.items.clear();
    }

    pub fn dispatch(&self) -> &DispatchTable {
        &self.dispatch
    }
}
