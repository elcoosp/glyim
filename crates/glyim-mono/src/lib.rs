#![deny(unreachable_patterns)]

pub mod concretize;
pub mod driver;
pub mod mangle_table;
pub mod mangling;
pub mod metadata;
pub mod queue;

pub use concretize::{
    ConcretizeError, ConcretizeErrorKind, build_subst, concretize_and_register,
    has_unresolved_type_param, substitute_and_concretize,
};
pub use driver::{FailedItem, MonoDriver, MonoMetrics, MonoResult};
pub use mangle_table::MangleTable;
pub use mangling::{ManglingError, mangle_method_name, mangle_name, type_to_short_string};
pub use metadata::{TypeMetadata, TypeStructure};
pub use queue::{ItemKind, WorkItem, WorkItemContext, WorkQueue};

#[cfg(test)]
mod tests;
