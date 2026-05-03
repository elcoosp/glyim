use glyim_typeck::TypeChecker;
use proptest::prelude::*;

proptest! {
    /// A well‑typed program with a simple main should not produce type errors.
    #[test]
    fn simple_main_no_type_errors(value in any::<i64>()) {
        let source = format!("main = () => {}", value);
        let parse_out = glyim_parse::parse(&source);
        prop_assert!(parse_out.errors.is_empty());
        let mut interner = parse_out.interner;
        let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
        let mut tc = TypeChecker::new(interner);
        let result = tc.check(&hir);
        prop_assert!(result.is_ok(), "type errors: {:?}", result.err());
    }
}
