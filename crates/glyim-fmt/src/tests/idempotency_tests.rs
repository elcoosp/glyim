use crate::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]
    #[test]
    fn prop_idempotent_on_random(source in "\\PC{0,80}") {
        let config = FormatConfig::default();
        let first = format_source(&source, &config).expect("format failed");
        let second = format_source(&first, &config).expect("reformat failed");
        prop_assert_eq!(first, second, "Source: {}", source);
    }
}
