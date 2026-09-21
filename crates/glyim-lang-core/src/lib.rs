//! Glyim Language Core Library
//!
//! This crate contains the standard library source files for the Glyim language.
//! The actual library code is in `.g` files under `lib/`, written in Glyim syntax.
//! This Rust crate provides access to those source files and testing infrastructure.

/// Returns the source code of a core library module by name.
pub fn core_source(name: &str) -> Option<&'static str> {
    match name {
        "option" => Some(include_str!("../lib/option.g")),
        "result" => Some(include_str!("../lib/result.g")),
        "iter" => Some(include_str!("../lib/iter.g")),
        "slice" => Some(include_str!("../lib/slice.g")),
        "str" => Some(include_str!("../lib/str.g")),
        "cell" => Some(include_str!("../lib/cell.g")),
        "mem" => Some(include_str!("../lib/mem.g")),
        "ptr" => Some(include_str!("../lib/ptr.g")),
        "ops" => Some(include_str!("../lib/ops.g")),
        "cmp" => Some(include_str!("../lib/cmp.g")),
        "marker" => Some(include_str!("../lib/marker.g")),
        "panic" => Some(include_str!("../lib/panic.g")),
        "hint" => Some(include_str!("../lib/hint.g")),
        "convert" => Some(include_str!("../lib/convert.g")),
        "default" => Some(include_str!("../lib/default.g")),
        "future" => Some(include_str!("../lib/future.g")),
        _ => None,
    }
}

/// Returns the names of all core library modules.
pub fn core_modules() -> &'static [&'static str] {
    &[
        "option", "result", "iter", "slice", "str", "cell", "mem", "ptr", "ops", "cmp", "marker",
        "panic", "hint", "convert", "default", "future",
    ]
}

/// Returns the combined source of all core library modules.
pub fn core_source_all() -> String {
    let mut out = String::new();
    for name in core_modules() {
        if let Some(src) = core_source(name) {
            out.push_str(&format!("// === module: {} ===\n", name));
            out.push_str(src);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests;
