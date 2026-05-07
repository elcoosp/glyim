use crate::types::TestDef;
use glyim_parse::Ast;
use glyim_interner::Interner;

pub fn collect_tests(
    ast: &Ast,
    interner: &Interner,
    filter: Option<&str>,
    include_ignored: bool,
) -> Vec<TestDef> {
    let mut tests = Vec::new();
    for item in &ast.items {
        if let glyim_parse::Item::FnDef { name, attrs, .. } = item {
            let mut is_test = false;
            let mut is_opt = false;
            let mut is_ignored = false;
            let mut should_panic = false;
            for attr in attrs {
                match attr.name.as_str() {
                    "test" => {
                        is_test = true;
                        for arg in &attr.args {
                            if arg.key == "should_panic" { should_panic = true; }
                        }
                    }
                    "optimize_check" => { is_test = true; is_opt = true; }
                    "ignore" => is_ignored = true,
                    _ => {}
                }
            }
            if !is_test { continue; }
            if is_ignored && !include_ignored { continue; }
            let test_name = interner.resolve(*name).to_string();
            if let Some(f) = filter
                && test_name != f { continue; }
            tests.push(TestDef {
                name: test_name,
                source_file: String::new(),
                ignored: is_ignored,
                should_panic,
                is_optimize_check: is_opt,
                tags: vec![],
            });
        }
    }
    tests
}
