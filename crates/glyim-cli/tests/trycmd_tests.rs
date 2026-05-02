#[test]
fn cli_trycmd() {
    let t = trycmd::TestCases::new();
    t.case("tests/cmd/*.trycmd");
}
