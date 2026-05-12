use crate::TypeChecker;
use glyim_hir::lower;
use glyim_hir::types::{HirType, TypeVar};
use glyim_interner::Interner;
use glyim_parse::parse;

fn typecheck_source(source: &str) -> TypeChecker {
    let parse_out = parse(source);
    assert!(parse_out.errors.is_empty(), "Parse errors: {:?}", parse_out.errors);
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    let _ = tc.check(&hir);
    tc
}

#[test]
fn test_generic_impl_field_access_infers_type() {
    // Target scenario: accessing 'self.buckets' in an impl<K,V>
    // Expected: The type should resolve to something like Vec<Entry<K,V>>
    // (represented as Generic(Vec, [...]) internally), NOT Error.
    let source = r#"
    struct Entry<K, V> { key: K, value: V }
    struct HashMap<K, V> { buckets: Vec<Entry<K, V>> }

    impl<K, V> HashMap<K, V> {
        fn get_buckets(self: HashMap<K, V>) -> Vec<Entry<K, V>> {
            self.buckets
        }
    }

    main = () => {
        let m = HashMap { buckets: Vec::new() };
        m.get_buckets()
    }
    "#;

    let tc = typecheck_source(source);

    // Find the expression ID for 'self.buckets' inside get_buckets
    // In a real implementation we might need to map IDs or just check
    // the final FnTypes map for 'HashMap_get_buckets'.
    let get_buckets_sym = tc.interner.intern("HashMap_get_buckets");
    let fn_types = tc.fn_types_map.get(&get_buckets_sym);

    assert!(fn_types.is_some(), "Function should be in type map");

    // The current bug: self.buckets resolves to Error because K and V are unknown.
    // We want to verify this behavior exists before we fix it.
    // This checks if any expression in the function body is Error.
    let has_error = fn_types.unwrap().expr_types.values().any(|t| matches!(t, HirType::Error));
    assert!(has_error, "Expected current implementation to fail with Error type");
}
