#![allow(deprecated)]
pub mod cross;
pub mod docgen;
pub mod lockfile_integration;
pub mod macro_expand;
pub mod manifest;
pub mod pipeline;
pub mod queries;

pub use pipeline::{
    BuildMode, PipelineConfig, PipelineError, build, build_with_mode, check,
    extract_types_from_result, generate_doc, init, print_ir, run, run_doctests, run_jit,
    run_with_mode,
};
