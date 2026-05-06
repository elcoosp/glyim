use glyim_compiler::pipeline::{semantic_source_hash, semantic_hash_of_source};

#[test]
fn semantic_source_hash_is_deterministic() {
    let source = "fn main() { 1 + 2 }";
    let h1 = semantic_source_hash(source);
    let h2 = semantic_source_hash(source);
    assert_eq!(h1, h2);
}

#[test]
fn semantic_source_hash_different_for_different_code() {
    let source_a = "fn main() { 1 + 2 }";
    let source_b = "fn main() { 3 + 4 }";
    let h_a = semantic_source_hash(source_a);
    let h_b = semantic_source_hash(source_b);
    assert_ne!(h_a, h_b);
}

#[test]
fn semantic_source_hash_returns_32_bytes() {
    let h = semantic_source_hash("42");
    assert_eq!(h.as_bytes().len(), 32);
}

#[test]
fn semantic_hash_of_source_local_rename_stable() {
    // Two sources that differ only in local variable names
    let source_x = "fn add(x) { x + 1 }";
    let source_y = "fn add(y) { y + 1 }";
    let h_x = semantic_hash_of_source(source_x);
    let h_y = semantic_hash_of_source(source_y);
    // After normalization, these should hash identically
    assert_eq!(h_x, h_y);
}

#[test]
fn semantic_hash_of_source_different_body_different_hash() {
    let source_a = "fn add(x) { x + 1 }";
    let source_b = "fn add(x) { x + 2 }";
    let h_a = semantic_hash_of_source(source_a);
    let h_b = semantic_hash_of_source(source_b);
    assert_ne!(h_a, h_b);
}

#[test]
fn semantic_hash_of_source_is_deterministic() {
    let source = "fn main() { 42 }";
    let h1 = semantic_hash_of_source(source);
    let h2 = semantic_hash_of_source(source);
    assert_eq!(h1, h2);
}
