use std::ffi::CString;
use std::ptr;

// LLJIT types/functions are in llvm_sys::orc2::lljit
use llvm_sys::orc2::lljit;

pub struct OrcSession {
    jit: lljit::LLVMOrcLLJITRef,
    target_triple: String,
}

impl OrcSession {
    pub fn new() -> Self {
        Self::with_target_triple(None)
    }

    pub fn with_target_triple(target_triple: Option<&str>) -> Self {
        let triple = target_triple
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                inkwell::targets::TargetMachine::get_default_triple().to_string()
            });

        let builder = unsafe { lljit::LLVMOrcCreateLLJITBuilder() };
        let mut jit: lljit::LLVMOrcLLJITRef = ptr::null_mut();
        let error = unsafe { lljit::LLVMOrcCreateLLJIT(&mut jit, builder) };

        if !error.is_null() {
            let msg = unsafe {
                let msg_ptr = llvm_sys::error::LLVMGetErrorMessage(error);
                let c_str = std::ffi::CStr::from_ptr(msg_ptr);
                let msg = c_str.to_string_lossy().to_string();
                llvm_sys::error::LLVMDisposeErrorMessage(msg_ptr);
                msg
            };
            panic!("Failed to create LLJIT: {}", msg);
        }

        Self { jit, target_triple: triple }
    }

    pub fn create_dylib(&mut self, name: &str) -> Result<OrcDylib, String> {
        let session = unsafe { lljit::LLVMOrcLLJITGetExecutionSession(self.jit) };
        let c_name = CString::new(name).map_err(|e| format!("invalid name: {e}"))?;
        let dylib_ref = unsafe {
            llvm_sys::orc2::LLVMOrcExecutionSessionCreateBareJITDylib(
                session,
                c_name.as_ptr(),
            )
        };
        if dylib_ref.is_null() {
            return Err(format!("failed to create JITDylib '{}'", name));
        }
        Ok(OrcDylib {
            dylib: dylib_ref,
            name: name.to_string(),
        })
    }

    pub fn raw_session(&self) -> llvm_sys::orc2::LLVMOrcExecutionSessionRef {
        unsafe { lljit::LLVMOrcLLJITGetExecutionSession(self.jit) }
    }
}

impl Drop for OrcSession {
    fn drop(&mut self) {
        if !self.jit.is_null() {
            unsafe { lljit::LLVMOrcDisposeLLJIT(self.jit); }
        }
    }
}

unsafe impl Send for OrcSession {}

pub struct OrcDylib {
    dylib: llvm_sys::orc2::LLVMOrcJITDylibRef,
    name: String,
}

impl OrcDylib {
    pub fn name(&self) -> &str { &self.name }
    pub fn raw_dylib(&self) -> llvm_sys::orc2::LLVMOrcJITDylibRef { self.dylib }
}

impl Default for OrcSession {
    fn default() -> Self { Self::new() }
}
