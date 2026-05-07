use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct Symbol(u32);

impl Symbol {
    pub fn from_raw(id: u32) -> Self {
        Symbol(id)
    }
    pub fn raw(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone)]
struct Entry {
    string: String,
    ref_count: u32,
}

#[derive(Debug, Clone)]
pub struct Interner {
    entries: Vec<Entry>,
    map: HashMap<String, u32>,
}

impl Interner {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            map: HashMap::new(),
        }
    }

    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&id) = self.map.get(s) {
            return Symbol(id);
        }
        let id = self.entries.len() as u32;
        self.entries.push(Entry {
            string: s.to_owned(),
            ref_count: 0,
        });
        self.map.insert(s.to_owned(), id);
        Symbol(id)
    }

    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.entries[sym.0 as usize].string
    }

    pub fn try_resolve(&self, sym: Symbol) -> Option<&str> {
        self.entries.get(sym.0 as usize).map(|e| e.string.as_str())
    }

    pub fn get_symbol(&self, index: u32) -> Option<Symbol> {
        if index < self.entries.len() as u32 {
            Some(Symbol(index))
        } else {
            None
        }
    }

    pub fn resolve_symbol(&self, s: &str) -> Option<Symbol> {
        self.map.get(s).map(|&id| Symbol(id))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn all_symbols(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.string.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Increment the reference count for a symbol (used during reachability marking).
    pub fn increment_ref(&mut self, sym: Symbol) {
        if let Some(entry) = self.entries.get_mut(sym.0 as usize) {
            entry.ref_count += 1;
        }
    }

    /// Reset all reference counts to zero before a new marking pass.
    pub fn reset_ref_counts(&mut self) {
        for entry in &mut self.entries {
            entry.ref_count = 0;
        }
    }

    /// Compact the internter: remove all symbols whose reference count is zero,
    /// then remap any old `Symbol` values to their new indices.
    ///
    /// Returns a mapping from old `Symbol` to new `Symbol` that can be used
    /// to update all data structures that hold symbols.
    pub fn compact(&mut self) -> HashMap<Symbol, Symbol> {
        let mut old_to_new = HashMap::new();
        let mut new_entries = Vec::new();
        let mut new_map = HashMap::new();

        // Rebuild entries array, keeping only ref_count > 0
        for (old_idx, entry) in self.entries.iter().enumerate() {
            if entry.ref_count > 0 {
                let new_idx = new_entries.len() as u32;
                old_to_new.insert(Symbol(old_idx as u32), Symbol(new_idx));
                new_entries.push(Entry {
                    string: entry.string.clone(),
                    ref_count: 0, // reset for next compilation
                });
                new_map.insert(entry.string.clone(), new_idx);
            }
        }

        // Replace internals
        self.entries = new_entries;
        self.map = new_map;

        old_to_new
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_deduplicates_equal_strings() {
        let mut i = Interner::new();
        assert_eq!(i.intern("hello"), i.intern("hello"));
    }

    #[test]
    fn intern_different_strings_produce_different_symbols() {
        let mut i = Interner::new();
        assert_ne!(i.intern("hello"), i.intern("world"));
    }

    #[test]
    fn resolve_returns_original_string() {
        let mut i = Interner::new();
        let s = i.intern("hello");
        assert_eq!(i.resolve(s), "hello");
    }

    #[test]
    fn resolve_multiple_strings() {
        let mut i = Interner::new();
        let h = i.intern("hello");
        let w = i.intern("world");
        assert_eq!(i.resolve(h), "hello");
        assert_eq!(i.resolve(w), "world");
    }

    #[test]
    fn intern_empty_string() {
        let mut i = Interner::new();
        let s = i.intern("");
        assert_eq!(i.resolve(s), "");
    }

    #[test]
    fn intern_unicode() {
        let mut i = Interner::new();
        let s = i.intern("日本語");
        assert_eq!(i.resolve(s), "日本語");
    }

    #[test]
    fn len_tracks_unique_strings() {
        let mut i = Interner::new();
        assert_eq!(i.len(), 0);
        i.intern("a");
        assert_eq!(i.len(), 1);
        i.intern("a");
        assert_eq!(i.len(), 1);
        i.intern("b");
        assert_eq!(i.len(), 2);
    }

    #[test]
    fn default_creates_empty_interner() {
        assert!(Interner::default().is_empty());
    }

    #[test]
    fn compact_removes_unreferenced() {
        let mut interner = Interner::new();
        let a = interner.intern("a");
        let b = interner.intern("b");
        let c = interner.intern("c");

        // Mark 'a' and 'c' as referenced
        interner.increment_ref(a);
        interner.increment_ref(c);
        // 'b' has zero refs

        let remap = interner.compact();

        // After compact, len should be 2 (a and c)
        assert_eq!(interner.len(), 2);

        // Old symbols should map to new ones
        let new_a = remap[&a];
        let new_c = remap[&c];
        assert!(!remap.contains_key(&b));

        // New symbols resolve correctly
        assert_eq!(interner.resolve(new_a), "a");
        assert_eq!(interner.resolve(new_c), "c");
    }

    #[test]
    fn compact_preserves_order() {
        let mut interner = Interner::new();
        let a = interner.intern("a");
        let b = interner.intern("b");
        let c = interner.intern("c");
        interner.increment_ref(a);
        interner.increment_ref(b);
        interner.increment_ref(c);
        interner.compact();
        // All should still be there, in order
        assert_eq!(interner.len(), 3);
        assert_eq!(interner.resolve(Symbol(0)), "a");
        assert_eq!(interner.resolve(Symbol(1)), "b");
        assert_eq!(interner.resolve(Symbol(2)), "c");
    }
}
