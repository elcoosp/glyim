#![deny(unreachable_patterns)]

pub mod mangle_table;
pub mod mangling;
pub mod metadata;
pub mod queue;

pub use mangle_table::MangleTable;
pub use mangling::{ManglingError, mangle_name, mangle_method_name, type_to_short_string};
pub use metadata::{TypeMetadata, TypeStructure};
pub use queue::{ItemKind, WorkItem, WorkItemContext, WorkQueue};
