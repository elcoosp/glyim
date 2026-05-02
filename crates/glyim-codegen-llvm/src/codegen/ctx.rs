use glyim_interner::Symbol;
use inkwell::values::{FunctionValue, PointerValue};
use std::collections::HashMap;
pub struct FunctionContext<'ctx> {
    pub(crate) vars: HashMap<Symbol, PointerValue<'ctx>>,
    pub(crate) fn_value: FunctionValue<'ctx>,
    pub(crate) ret_val_ptr: Option<PointerValue<'ctx>>,
    pub(crate) ret_bb: Option<inkwell::basic_block::BasicBlock<'ctx>>,
}
impl<'ctx> FunctionContext<'ctx> {}
