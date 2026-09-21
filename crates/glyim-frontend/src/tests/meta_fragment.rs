//! Regression tests for de-stubbing plan §2.3: `:meta` fragment grammar.
//!
//! `try_parse_fragment("meta", src)` must now validate a real meta-item
//! grammar (Word / NameValue / List) instead of accepting any non-empty
//! content. Well-formed meta items should be accepted; malformed input
//! (unbalanced list, missing path, trailing tokens) must be rejected.

use crate::try_parse_fragment;

fn accepts(src: &str) -> bool {
    try_parse_fragment("meta", src).is_some()
}

#[test]
fn meta_word_single_segment() {
    assert!(accepts("test"), "bare word meta `test` should be accepted");
}

#[test]
fn meta_word_path_segmented() {
    assert!(
        accepts("cfg(feature = \"x\")") || accepts("path::to::attr"),
        "segmented path word meta should be accepted"
    );
    // Explicitly: a two-segment path is a valid Word meta.
    assert!(
        accepts("foo::bar"),
        "segmented path `foo::bar` should be accepted"
    );
}

#[test]
fn meta_name_value_string() {
    assert!(
        accepts("doc = \"hello\""),
        "`doc = \"hello\"` name-value meta should be accepted"
    );
}

#[test]
fn meta_name_value_int() {
    assert!(
        accepts("count = 42"),
        "`count = 42` name-value meta should be accepted"
    );
}

#[test]
fn meta_list_empty() {
    assert!(
        accepts("attr()"),
        "empty list meta `attr()` should be accepted"
    );
}

#[test]
fn meta_list_with_inner_word() {
    assert!(
        accepts("attr(a, b)"),
        "list meta `attr(a, b)` should be accepted"
    );
}

#[test]
fn meta_list_with_inner_name_value() {
    assert!(
        accepts("attr(a = 1, b = \"x\")"),
        "list meta `attr(a = 1, b = \"x\")` should be accepted"
    );
}

#[test]
fn meta_list_nested() {
    assert!(
        accepts("outer(inner(x))"),
        "nested list meta `outer(inner(x))` should be accepted"
    );
}

#[test]
fn meta_rejects_unbalanced_list() {
    assert!(
        !accepts("attr("),
        "unbalanced open list `attr(` should be rejected"
    );
    assert!(
        !accepts("attr(]"),
        "list with wrong close `attr(]` should be rejected"
    );
}

#[test]
fn meta_rejects_missing_path() {
    assert!(
        !accepts("= foo"),
        "meta `= foo` (missing path) should be rejected"
    );
}

#[test]
fn meta_rejects_trailing_tokens() {
    assert!(
        !accepts("foo )"),
        "meta `foo )` (trailing rparen) should be rejected"
    );
    assert!(
        !accepts("foo bar"),
        "meta `foo bar` (two words) should be rejected"
    );
}

#[test]
fn meta_rejects_empty() {
    assert!(!accepts(""), "empty meta should be rejected");
}

#[test]
fn meta_rejects_name_value_missing_literal() {
    assert!(
        !accepts("foo ="),
        "name-value meta `foo =` (missing literal) should be rejected"
    );
    assert!(
        !accepts("foo = bar"),
        "name-value meta `foo = bar` (rhs is not a literal) should be rejected"
    );
}
