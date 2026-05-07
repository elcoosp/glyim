use crate::*;
use proptest::prelude::*;

/// Helper to format and check idempotency
fn check_idempotent(source: &str) {
    let config = FormatConfig::default();
    let first = format_source(source, &config).expect("format failed");
    let second = format_source(&first, &config).expect("reformat failed");
    assert_eq!(first, second, "Formatter is not idempotent!\nOriginal:\n---\n{}\n---\nFirst format:\n---\n{}\n---\nSecond format:\n---\n{}\n---",
        source, first, second);
}

proptest! {
    /// Random strings should not crash and should be idempotent.
    #[test]
    fn prop_idempotent_on_random(source in "\\PC{0,200}") {
        // We just test that formatting twice gives same result; we don't care if first format changes.
        let config = FormatConfig::default();
        let first = match format_source(&source, &config) {
            Ok(s) => s,
            Err(_) => return Ok(()), // ignore if formatting fails for random strings
        };
        let second = format_source(&first, &config).unwrap();
        prop_assert_eq!(first, second, "Source: {}", source);
    }
}
