use crate::optimize::check_optimization;

#[test]
fn optimize_checks_syntax_only() {
    let src = "main = () => 42";
    // If FileCheck not installed, the command will fail – we expect an error, not panic.
    let res = check_optimization(src);
    match res {
        Ok(failures) => {
            // If it passes, there should be no failures for a simple source without CHECK lines.
            // Actually FileCheck without CHECK lines passes vacuously.
            assert!(failures.is_empty());
        }
        Err(e) => {
            // FileCheck not found is acceptable
            assert!(e.contains("failed to run FileCheck") || e.contains("IO error"));
        }
    }
}
