use glyim_interner::{Interner, Symbol};

const MANGLE_SEPARATOR: &str = "__";

pub fn demangle_for_display(interner: &Interner, sym: Symbol) -> String {
    let s = interner.resolve(sym).to_string();

    // Check for method mangling: Type_method__args
    if let Some(pos) = s.find(MANGLE_SEPARATOR) {
        let base = &s[..pos];
        let args = &s[pos + MANGLE_SEPARATOR.len()..];

        // Try to parse type arguments
        if !args.is_empty() {
            let type_args: Vec<&str> = args.split(MANGLE_SEPARATOR).collect();
            if type_args.len() == 1 && type_args[0].is_empty() {
                return base.to_string();
            }
            return format!("{}<{}>", base, type_args.join(", "));
        }

        return base.to_string();
    }

    s.to_string()
}

pub fn is_method(_interner: &Interner, sym: Symbol) -> bool {
    let s = _interner.resolve(sym);
    s.contains(MANGLE_SEPARATOR)
}

pub fn method_base_type<'a>(interner: &'a Interner, sym: Symbol) -> Option<&'a str> {
    let s = interner.resolve(sym);
    let pos = s.find(MANGLE_SEPARATOR)?;
    Some(&s[..pos])
}
