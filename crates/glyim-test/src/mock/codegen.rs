use glyim_codegen::CodegenBackend;
use glyim_diag::CompResult;
use glyim_mir::Body;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Debug)]
/// CodegenCall.
pub struct CodegenCall {
    /// Struct.
    pub body_count: usize,
    /// Struct.
    pub output_path: std::path::PathBuf,
}

/// MockCodegen.
pub struct MockCodegen {
    calls: Mutex<Vec<CodegenCall>>,
    function_calls: AtomicUsize,
}

impl MockCodegen {
    /// new.
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            function_calls: AtomicUsize::new(0),
        }
    }
    /// calls.
    pub fn calls(&self) -> Vec<CodegenCall> {
        self.calls.lock().clone()
    }
    /// function_call_count.
    pub fn function_call_count(&self) -> usize {
        self.function_calls.load(Ordering::Relaxed)
    }
}

impl Default for MockCodegen {
    fn default() -> Self {
        Self::new()
    }
}

impl CodegenBackend for MockCodegen {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn generate(&self, bodies: &[Arc<Body>], output: &Path) -> CompResult<()> {
        self.calls.lock().push(CodegenCall {
            body_count: bodies.len(),
            output_path: output.to_path_buf(),
        });
        Ok(())
    }
    fn generate_function(&self, _body: &Arc<Body>) -> CompResult<Vec<u8>> {
        self.function_calls.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }
}
