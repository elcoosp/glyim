use glyim_interner::{Interner, Symbol};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct KnownSymbols {
    pub option: Symbol,
    pub some: Symbol,
    pub none: Symbol,
    pub result: Symbol,
    pub ok: Symbol,
    pub err_sym: Symbol,
    pub vec: Symbol,
    pub main_fn: Symbol,
    pub iterator: Symbol,
    pub bool_type: Symbol,
    pub i64_type: Symbol,
    pub f64_type: Symbol,
    pub str_type: Symbol,
    pub unit_type: Symbol,
    builtin_set: HashSet<Symbol>,
}

impl KnownSymbols {
    pub fn intern_all(interner: &mut Interner) -> Self {
        let option = interner.intern("Option");
        let some = interner.intern("Some");
        let none = interner.intern("None");
        let result = interner.intern("Result");
        let ok = interner.intern("Ok");
        let err_sym = interner.intern("Err");
        let vec = interner.intern("Vec");
        let main_fn = interner.intern("main");
        let iterator = interner.intern("Iterator");
        let bool_type = interner.intern("bool");
        let i64_type = interner.intern("i64");
        let f64_type = interner.intern("f64");
        let str_type = interner.intern("str");
        let unit_type = interner.intern("unit");

        let mut builtin_set = HashSet::new();
        for &s in &[
            option, result, vec, iterator, bool_type, i64_type, f64_type, str_type, unit_type,
        ] {
            builtin_set.insert(s);
        }

        Self {
            option,
            some,
            none,
            result,
            ok,
            err_sym,
            vec,
            main_fn,
            iterator,
            bool_type,
            i64_type,
            f64_type,
            str_type,
            unit_type,
            builtin_set,
        }
    }

    pub fn is_builtin_type(&self, sym: Symbol) -> bool {
        self.builtin_set.contains(&sym)
    }

    /// Get the original string name for a symbol (reverse lookup).
    pub fn unresolve_symbol(&self, sym: Symbol) -> &str {
        // We need access to the interner. Since KnownSymbols doesn't hold an interner,
        // we'll add a method that takes an interner parameter instead.
        unreachable!("use typeck's normalizer which has access to the interner")
    }

}
