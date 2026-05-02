use proptest::prelude::*;
use glyim_parse::parse;

proptest! {
    /// Any valid integer literal should parse without errors and produce a binding.
    #[test]
    fn integer_literal_roundtrip(value in any::<i64>()) {
        let source = format!("main = () => {}", value);
        let out = parse(&source);
        prop_assert!(out.errors.is_empty(), "parse errors: {:?}", out.errors);
        prop_assert_eq!(out.ast.items.len(), 1);
    }

    /// Any valid string literal (without newlines) should parse without errors.
    #[test]
    fn string_literal_roundtrip(s in "\\PC*") {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        let source = format!("main = () => \"{}\"", escaped);
        let out = parse(&source);
        prop_assert!(out.errors.is_empty());
        prop_assert_eq!(out.ast.items.len(), 1);
    }
}
