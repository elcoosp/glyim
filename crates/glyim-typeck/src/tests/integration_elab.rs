use crate::TypeCheckOutput;
use glyim_interner::Interner;

#[test]
fn type_check_output_exists() {
    let mut interner = Interner::new();
    let output = TypeCheckOutput {
        expr_types: vec![],
        call_type_args: std::collections::HashMap::new(),
        interner: interner.clone(),
        reflect_metadata: vec![],
        generated_items: vec![],
    };
    assert!(output.expr_types.is_empty());
}
