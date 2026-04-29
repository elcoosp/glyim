use glyim_interner::Symbol;
use inkwell::values::{FunctionValue, PointerValue};
use std::collections::HashMap;

pub struct FunctionContext<'ctx> {
    pub(crate) vars: HashMap<Symbol, PointerValue<'ctx>>,
    pub(crate) fn_value: FunctionValue<'ctx>,
}

impl<'ctx> FunctionContext<'ctx> {
    #[allow(dead_code)]
    pub fn new(fn_value: FunctionValue<'ctx>) -> Self {
        Self {
            vars: HashMap::new(),
            fn_value,
        }
    }
}
