use glyim_interner::Interner;
use glyim_parse::{Ast, Item};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFunction {
    pub name: String,
    pub ignored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestResult {
    Passed,
    Failed,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRunSummary {
    pub results: Vec<(String, TestResult)>,
}

impl TestRunSummary {
    pub fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|(_, r)| *r == TestResult::Passed)
            .count()
    }
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|(_, r)| *r == TestResult::Failed)
            .count()
    }
    pub fn ignored(&self) -> usize {
        self.results
            .iter()
            .filter(|(_, r)| *r == TestResult::Ignored)
            .count()
    }
    pub fn total(&self) -> usize {
        self.results.len()
    }
    pub fn exit_code(&self) -> i32 {
        if self.failed() > 0 {
            1
        } else {
            0
        }
    }
    pub fn format_summary(&self) -> String {
        let passed = self.passed();
        let failed = self.failed();
        let ignored = self.ignored();
        format!(
            "\ntest result: {}. {} passed; {} failed; {} ignored",
            if failed == 0 { "ok" } else { "FAILED" },
            passed,
            failed,
            ignored
        )
    }
}

pub fn collect_test_functions(
    ast: &Ast,
    interner: &Interner,
    filter_name: Option<&str>,
    include_ignored: bool,
) -> Vec<TestFunction> {
    let mut tests = Vec::new();
    for item in &ast.items {
        if let Item::FnDef { attrs, name, .. } = item {
            let is_test = attrs.iter().any(|a| a.name == "test");
            if !is_test {
                continue;
            }
            let is_ignored = attrs.iter().any(|a| a.name == "ignore");
            if is_ignored && !include_ignored {
                continue;
            }
            let resolved = interner.resolve(*name).to_string();
            if let Some(filter) = filter_name {
                if resolved != *filter {
                    continue;
                }
            }
            tests.push(TestFunction {
                name: resolved,
                ignored: is_ignored,
            });
        }
    }
    tests
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_parse::parse;

    #[test]
    fn collect_single_test() {
        let out = parse("#[test]\nfn a() { 0 }");
        let tests = collect_test_functions(&out.ast, &out.interner, None, false);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "a");
    }

    #[test]
    fn collect_ignores_by_default() {
        let out = parse("#[test]\n#[ignore]\nfn a() { 0 }");
        let tests = collect_test_functions(&out.ast, &out.interner, None, false);
        assert!(tests.is_empty());
    }

    #[test]
    fn collect_includes_ignored_when_flagged() {
        let out = parse("#[test]\n#[ignore]\nfn a() { 0 }");
        let tests = collect_test_functions(&out.ast, &out.interner, None, true);
        assert_eq!(tests.len(), 1);
        assert!(tests[0].ignored);
    }

    #[test]
    fn summary_format() {
        let s = TestRunSummary {
            results: vec![
                ("a".into(), TestResult::Passed),
                ("b".into(), TestResult::Failed),
                ("c".into(), TestResult::Ignored),
            ],
        };
        let fmt = s.format_summary();
        assert!(fmt.contains("1 passed"));
        assert!(fmt.contains("1 failed"));
        assert!(fmt.contains("1 ignored"));
    }
}
