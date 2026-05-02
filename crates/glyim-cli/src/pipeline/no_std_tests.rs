use super::*;

#[cfg(test)]
mod no_std_tests {
    use super::*;
    #[test]
    fn detect_no_std_simple() {
        assert!(detect_no_std("no_std\nfn main() { 0 }"));
    }
    #[test]
    fn detect_no_std_at_start() {
        assert!(detect_no_std("no_std\nfn main() { 0 }"));
    }
    #[test]
    fn detect_no_std_false_when_absent() {
        assert!(!detect_no_std("fn main() { 0 }"));
    }
    #[test]
    fn detect_no_std_false_in_string() {
        assert!(!detect_no_std(r#"fn main() { "no_std" }"#));
    }
    #[test]
    fn detect_no_std_false_as_part_of_ident() {
        assert!(!detect_no_std("fn no_std_helper() { 0 }"));
    }
    #[test]
    fn detect_no_std_false_as_field_name() {
        assert!(!detect_no_std("struct S { no_std: bool }"));
    }
    #[test]
    fn detect_no_std_with_trailing_whitespace() {
        assert!(detect_no_std("no_std   \nfn main() { 0 }"));
    }
    #[test]
    fn detect_no_std_false_empty() {
        assert!(!detect_no_std(""));
    }
    #[test]
    fn detect_no_std_false_only_whitespace() {
        assert!(!detect_no_std("  \n  \n"));
    }
    #[test]
    fn detect_no_std_after_other_code() {
        assert!(detect_no_std("fn foo() { 0 }\nno_std\nfn bar() { 0 }"));
    }
    #[test]
    fn detect_no_std_known_limitation_comment() {
        assert!(!detect_no_std("// no_std\nfn main() { 0 }"));
    }
}