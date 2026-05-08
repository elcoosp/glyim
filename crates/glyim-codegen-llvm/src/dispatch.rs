use dashmap::DashMap;
use glyim_interner::Symbol;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct DispatchTable {
    pointers: DashMap<Symbol, AtomicUsize>,
}

impl DispatchTable {
    pub fn new() -> Self {
        Self { pointers: DashMap::new() }
    }

    pub fn get_address(&self, name: Symbol) -> usize {
        self.pointers.get(&name).map(|p| p.load(Ordering::Relaxed)).unwrap_or(0)
    }

    pub fn update(&self, name: Symbol, new_address: usize) {
        if let Some(ptr) = self.pointers.get(&name) {
            ptr.store(new_address, Ordering::Release);
        } else {
            self.pointers.insert(name, AtomicUsize::new(new_address));
        }
    }

    pub fn contains(&self, name: Symbol) -> bool {
        self.pointers.contains_key(&name)
    }

    pub fn remove(&self, name: Symbol) {
        self.pointers.remove(&name);
    }

    pub fn len(&self) -> usize {
        self.pointers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pointers.is_empty()
    }
}

impl Default for DispatchTable {
    fn default() -> Self {
        Self::new()
    }
}
