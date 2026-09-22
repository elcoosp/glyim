//! Asserts the assembled stdlib actually contains the user-facing types the
//! prelude promises (`println`, `Vec`, `Option`, `Result`, `String`, `Box`)
//! and that each is reachable as a `pub mod` block or (for `str`/`slice`,
//! which the assembler emits flat so `impl str { .. }` binds to the
//! primitive) as a real impl block.

#[test]
fn assembled_stdlib_contains_user_facing_types() {
    let src = glyim_lang_std::std_source_assembled();

    let expected: &[(&str, &str)] = &[
        ("println", "a println function or macro"),
        ("Vec", "the growable array type"),
        ("Option", "the option type"),
        ("Result", "the result type"),
        ("String", "the owned string type"),
        ("Box", "the heap pointer type"),
        ("Iterator", "the iterator trait"),
    ];

    let mut missing = Vec::new();
    for (name, what) in expected {
        if !src.contains(name) {
            missing.push(format!("{name} ({what})"));
        }
    }
    assert!(
        missing.is_empty(),
        "assembled stdlib is missing {} user-facing item(s): {}",
        missing.len(),
        missing.join("; ")
    );
}

#[test]
fn assembled_stdlib_declares_prelude_modules() {
    let src = glyim_lang_std::std_source_assembled();
    let mut declared = std::collections::HashSet::new();
    for line in src.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("pub mod ") {
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                declared.insert(name.to_string());
            }
        }
    }

    // Modules the assembler wraps as `pub mod X { ... }`.
    let required = [
        "cmp", "option", "result", "iter", "ops", "default", "mem", "ptr", "cell",
        "marker", "convert", "hint", "vec", "boxed", "rc", "string", "raw_vec", "alloc",
    ];
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|m| !declared.contains(*m))
        .collect();
    assert!(
        missing.is_empty(),
        "assembled stdlib is missing pub-mod blocks for: {missing:?}; declared: {:?}",
        declared
    );
}

#[test]
fn assembled_stdlib_emits_str_and_slice_flat() {
    // The assembler deliberately emits `str.g` / `slice.g` outside a
    // `pub mod` wrapper so `impl str { .. }` and `impl<T> [T] { .. }` bind
    // to the primitive types, not a local module named `str`/`slice`.
    // Assert the *content* is present so a regression that drops these
    // modules entirely is caught.
    let src = glyim_lang_std::std_source_assembled();
    assert!(
        src.contains("impl str"),
        "assembled stdlib must contain `impl str` (from str.g, emitted flat)"
    );
    assert!(
        src.contains("impl<T> [T]") || src.contains("impl [T]"),
        "assembled stdlib must contain `impl<T> [T]` or `impl [T]` (from slice.g, emitted flat)"
    );
}
