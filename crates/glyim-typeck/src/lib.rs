#![deny(unreachable_patterns)]

pub mod errors;
pub mod naming;
pub mod symbols;

pub use errors::{TypeError, UnifyError};
pub use naming::format_type_for_error;
pub use symbols::KnownSymbols;
