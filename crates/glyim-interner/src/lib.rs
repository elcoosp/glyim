//! String interning for Glyim compiler identifiers.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Symbol(u32);

#[derive(Debug, Clone)]
pub struct Interner {
    strings: Vec<String>,
    map: HashMap<String, u32>,
}

impl Interner {
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
            map: HashMap::new(),
        }
    }
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&id) = self.map.get(s) {
            return Symbol(id);
        }
        let id = self.strings.len() as u32;
        self.strings.push(s.to_owned());
        self.map.insert(s.to_owned(), id);
        Symbol(id)
    }
    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.0 as usize]
    }

    /// Returns None if the symbol index is out of bounds.
    pub fn try_resolve(&self, sym: Symbol) -> Option<&str> {
        self.strings.get(sym.0 as usize).map(|s| s.as_str())
    }
    /// Returns Some(Symbol) if `index` is a valid symbol index.
    pub fn get_symbol(&self, index: u32) -> Option<Symbol> {
        if index < self.strings.len() as u32 {
            Some(Symbol(index))
        } else {
            None
        }
    }
    pub fn len(&self) -> usize {
        self.strings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
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
}
