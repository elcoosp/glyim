//! Glyim Alloc Library – source access only.
//!
//! The actual implementation is in `.g` files. This crate only provides
//! access to those sources.

/// Returns the source code of an alloc library module by name.
pub fn alloc_source(name: &str) -> Option<&'static str> {
    match name {
        "alloc" => Some(include_str!("../lib/alloc.g")),
        "boxed" => Some(include_str!("../lib/boxed.g")),
        "vec" => Some(include_str!("../lib/vec.g")),
        "string" => Some(include_str!("../lib/string.g")),
        "rc" => Some(include_str!("../lib/rc.g")),
        "raw_vec" => Some(include_str!("../lib/raw_vec.g")),
        _ => None,
    }
}

/// alloc_modules.
pub fn alloc_modules() -> &'static [&'static str] {
    &["alloc", "boxed", "vec", "string", "rc", "raw_vec"]
}

/// alloc_source_all.
pub fn alloc_source_all() -> String {
    let mut out = String::new();
    for name in alloc_modules() {
        if let Some(src) = alloc_source(name) {
            out.push_str(&format!("// === module: {} ===\n", name));
            out.push_str(src);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests;
