use glyim_interner::Symbol;
use inkwell::values::{FunctionValue, PointerValue};
use std::collections::HashMap;
pub struct FunctionContext<'ctx> {
    pub(crate) vars: HashMap<Symbol, PointerValue<'ctx>>,
    pub(crate) fn_value: FunctionValue<'ctx>,
    pub(crate) ret_val_ptr: Option<PointerValue<'ctx>>,
    pub(crate) ret_bb: Option<inkwell::basic_block::BasicBlock<'ctx>>,
}
impl<'ctx> FunctionContext<'ctx> {
    pub fn new(fn_value: FunctionValue<'ctx>) -> Self { Self { vars: HashMap::new(), fn_value, ret_val_ptr: None, ret_bb: None } }
}
