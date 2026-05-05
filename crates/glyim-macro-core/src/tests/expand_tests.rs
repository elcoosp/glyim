use glyim_interner::Symbol;
use crate::context::{Field, MacroContext};
use crate::expand::{MacroArg, interpret_macro};

struct TestCtx;
impl MacroContext for TestCtx {
    fn trait_is_implemented(&self, _: Symbol, _: Symbol) -> bool {
        false
    }
    fn get_fields(&self, _: Symbol) -> Vec<Field> {
        vec![]
    }
    fn get_type_params(&self, _: Symbol) -> Vec<Symbol> {
        vec![]
    }
}

#[test]
fn identity_works() {
    let result = interpret_macro(&TestCtx, &[], &[MacroArg::Expr("42".into())]);
    assert_eq!(result.unwrap(), "42");
}

#[test]
fn identity_wrong_arg_count() {
    let result = interpret_macro(&TestCtx, &[], &[]);
    assert!(result.is_none());
}
