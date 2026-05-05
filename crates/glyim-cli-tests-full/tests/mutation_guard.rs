/// Mutation guard: if addition is mutated to subtraction, this test fails.
#[test]
fn critical_addition_must_not_be_mutated() {
    let result = glyim_cli::pipeline::run_jit("main = () => 1 + 2").unwrap();
    assert_eq!(result, 3);
}
