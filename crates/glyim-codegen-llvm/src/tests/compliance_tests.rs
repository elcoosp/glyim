use crate::LlvmBackend;
use glyim_codegen::CodegenBackend;
use std::io::Read;
use std::path::Path;

/// Per-process unique temp file path. Prevents concurrent test processes
/// (a second `cargo nextest` invocation, `cargo-mutants` workers, parallel
/// CI shards on the same host) from racing on a fixed `/tmp/glyim_test_*.o`
/// path — one process's `remove_file` was racing another's write+read, which
/// produced spurious `output.exists()` / `metadata()` failures.
fn tpath(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("glyim_test_{}_{}", std::process::id(), name))
}


#[test]
fn s08_t40_codegen_backend_name_static() {
    let backend: &dyn CodegenBackend = &LlvmBackend::new();
    let name = backend.name();
    assert_eq!(name, "llvm");
    // Ensure name is a static string (same pointer across calls)
    assert!(std::ptr::eq(name, backend.name()));
}

#[test]
fn s08_t41_generate_returns_error_for_invalid_path() {
    let backend = LlvmBackend::new();
    // Use a path with invalid characters (on Unix, /tmp works fine; use a deeply nested nonexistent dir)
    let output = Path::new("/tmp/nonexistent/nested/deeply/output.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, &output);
    assert!(result.is_err());
    // The error should be an internal error (target machine file write failure)
    let err = result.unwrap_err();
    assert!(!err.is_empty());
    for diag in &err {
        // Check that it's an internal error
        assert_eq!(diag.code.category, glyim_diag::ErrorCategory::Internal);
    }
}

#[test]
fn s08_t42_generate_function_error_for_invalid_triple() {
    let backend = LlvmBackend::default().with_target("nonexistent-triple-unknown");
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(9999),
    )));
    let result = backend.generate_function(&body);
    assert!(result.is_err());
    let errs = result.unwrap_err();
    assert!(!errs.is_empty());
}

#[test]
fn s08_t43_generate_function_produces_deterministic_output_across_backends() {
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(8888),
    )));
    let b1 = LlvmBackend::new();
    let b2 = LlvmBackend::new();
    let r1 = b1.generate_function(&body).unwrap();
    let r2 = b2.generate_function(&body).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn s08_t44_generate_with_empty_bodies_produces_valid_object() {
    let backend = LlvmBackend::new();
    let output = tpath("glyim_empty_module.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    backend.generate(&bodies, &output).unwrap();
    // Verify ELF magic if x86_64-unknown-linux-gnu (default)
    let mut buf = [0u8; 4];
    std::fs::File::open(&output)
        .unwrap()
        .read_exact(&mut buf)
        .unwrap();
    assert_eq!(&buf, &[0x7f, 0x45, 0x4c, 0x46]);
    std::fs::remove_file(&output).ok();
}

#[test]
fn s08_t45_generate_function_memory_buffer_not_leaked() {
    // The generate_function method creates a new module each time;
    // calling many times should not leak.
    let backend = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(7777),
    )));
    for _ in 0..200 {
        let _ = backend.generate_function(&body);
    }
}

#[test]
fn s08_t46_with_target_reuses_initialization() {
    // Calling with_target multiple times should not re-initialize LLVM targets unnecessarily,
    // but at least it should not crash.
    let b1 = LlvmBackend::default().with_target("x86_64-unknown-linux-gnu");
    let b2 = LlvmBackend::default().with_target("aarch64-unknown-linux-gnu");
    assert_eq!(b1.name(), "llvm");
    assert_eq!(b2.name(), "llvm");
    // Both should be usable
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(4444),
    )));
    b1.generate_function(&body).unwrap();
    b2.generate_function(&body).unwrap();
}

#[test]
fn s08_t47_generate_function_returns_vec_not_slice() {
    // The Vec<u8> returned must own its data.
    let backend = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(5555),
    )));
    let vec = backend.generate_function(&body).unwrap();
    let ptr = vec.as_ptr();
    drop(backend);
    // The vec must still be valid after dropping backend (context ownership)
    let _ = vec[0];
    // Re-access pointer (should be fine, vec owns)
    let _ = unsafe { *ptr };
}
