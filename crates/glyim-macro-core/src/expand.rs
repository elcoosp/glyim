use crate::context::MacroContext;
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub enum MacroArg {
    Expr(String),
    Ty(String),
}

/// Interpret a macro body. For `@identity`, returns the first expression argument.
pub fn interpret_macro(
    _ctx: &dyn MacroContext,
    _type_args: &[Symbol],
    args: &[MacroArg],
) -> Option<String> {
    args.first().and_then(|a| match a {
        MacroArg::Expr(s) => Some(s.clone()),
        _ => None,
    })
}
