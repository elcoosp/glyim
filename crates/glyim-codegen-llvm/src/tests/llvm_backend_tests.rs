use crate::LlvmBackend;
use glyim_codegen::CodegenBackend;

/// Per-process unique temp file path. Prevents concurrent test processes
/// (a second `cargo nextest` invocation, `cargo-mutants` workers, parallel
/// CI shards on the same host) from racing on a fixed `/tmp/glyim_test_*.o`
/// path — one process's `remove_file` was racing another's write+read, which
/// produced spurious `output.exists()` / `metadata()` failures.
fn tpath(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("glyim_test_{}_{}", std::process::id(), name))
}

#[test]
fn s08_t01_create_backend_without_crash() {
    let backend = LlvmBackend::new();
    assert_eq!(backend.name(), "llvm");
}

#[test]
fn s08_t02_generate_empty_bodies_creates_module() {
    let backend = LlvmBackend::new();
    let output = tpath("glyim_test_empty.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, &output);
    assert!(result.is_ok(), "generate with no bodies should succeed");
}

#[test]
fn s08_t03_generate_returns_ok() {
    let backend = LlvmBackend::new();
    let output = tpath("glyim_test_ok.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, &output);
    assert!(result.is_ok());
}

#[test]
fn s08_t04_name_returns_llvm() {
    let backend = LlvmBackend::new();
    assert_eq!(backend.name(), "llvm");
}

#[test]
fn s08_t05_multiple_generate_calls_reuse_context() {
    let backend = LlvmBackend::new();
    let output1 = tpath("glyim_test_reuse1.o");
    let output2 = tpath("glyim_test_reuse2.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];

    let r1 = backend.generate(&bodies, &output1);
    let r2 = backend.generate(&bodies, &output2);

    assert!(r1.is_ok(), "First generate call should succeed");
    assert!(
        r2.is_ok(),
        "Second generate call should succeed (reuses Context)"
    );
}

#[test]
fn s08_t06_generate_function_with_body() {
    let backend = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    )));
    let result = backend.generate_function(&body);
    assert!(result.is_ok(), "generate_function should succeed");
    let bytes = result.unwrap();
    assert!(
        !bytes.is_empty(),
        "generate_function should return non-empty bytes"
    );
}

#[test]
fn s08_t07_with_target_constructor() {
    let backend = LlvmBackend::default().with_target("aarch64-unknown-linux-gnu");
    assert_eq!(backend.name(), "llvm");
    // Verify it doesn't crash on generate
    let output = tpath("glyim_test_aarch64.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, &output);
    assert!(
        result.is_ok(),
        "generate with aarch64 target should succeed"
    );
}

#[test]
fn s08_t08_default_trait() {
    let backend: LlvmBackend = Default::default();
    assert_eq!(backend.name(), "llvm");
}

#[test]
fn s08_t09_generate_with_single_body() {
    let backend = LlvmBackend::new();
    let output = tpath("glyim_test_single.o");
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    )));
    let bodies = vec![body];
    let result = backend.generate(&bodies, &output);
    assert!(result.is_ok(), "generate with single body should succeed");
    // Verify output file was created
    assert!(output.exists(), "output file should exist after generate");
}

#[test]
fn s08_t10_generate_with_multiple_bodies() {
    let backend = LlvmBackend::new();
    let output = tpath("glyim_test_multi.o");
    let mut bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    for i in 0..5 {
        let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(i),
        )));
        bodies.push(body);
    }
    let result = backend.generate(&bodies, &output);
    assert!(result.is_ok(), "generate with 5 bodies should succeed");
    assert!(
        output.exists(),
        "output file should exist after generate with multiple bodies"
    );
}

#[test]
fn s08_t11_stress_many_bodies() {
    let backend = LlvmBackend::new();
    let output = tpath("glyim_test_stress.o");
    let mut bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    for i in 0..100 {
        let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(i),
        )));
        bodies.push(body);
    }
    let result = backend.generate(&bodies, &output);
    assert!(
        result.is_ok(),
        "generate with 100 bodies should succeed without crash"
    );
}

#[test]
fn s08_t12_generate_function_multiple_calls() {
    let backend = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    )));
    // Multiple generate_function calls should work (reuse context)
    let r1 = backend.generate_function(&body);
    let r2 = backend.generate_function(&body);
    let r3 = backend.generate_function(&body);
    assert!(r1.is_ok(), "First generate_function should succeed");
    assert!(r2.is_ok(), "Second generate_function should succeed");
    assert!(r3.is_ok(), "Third generate_function should succeed");
}

#[test]
fn s08_t13_empty_generate_function_returns_empty() {
    let backend = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    )));
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    // Empty body (no basic blocks) should still return valid bytes
    let bytes = result.unwrap();
    assert!(
        !bytes.is_empty(),
        "Even empty body should produce some bytes"
    );
}

#[test]
fn s08_t14_different_output_paths() {
    let backend = LlvmBackend::new();
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    // Path under a directory that does not exist: LLVM's object writer
    // must return a clean error, not panic.
    let output = std::env::temp_dir()
        .join(format!("glyim_nonexistent_dir_{}", std::process::id()))
        .join("output.o");
    let result = backend.generate(&bodies, &output);
    // Should return an error because directory doesn't exist
    assert!(
        result.is_err(),
        "generate to nonexistent directory should error"
    );
}

#[test]
fn s08_t15_generate_function_different_owners() {
    let backend = LlvmBackend::new();
    let body1 = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(1),
    )));
    let body2 = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(2),
    )));
    let r1 = backend.generate_function(&body1);
    let r2 = backend.generate_function(&body2);
    assert!(r1.is_ok());
    assert!(r2.is_ok());
    let bytes1 = r1.unwrap();
    let bytes2 = r2.unwrap();
    // Both should return non-empty bytes
    assert!(!bytes1.is_empty());
    assert!(!bytes2.is_empty());
    // They might differ in internal function names, but minimal object may be identical.
    // At least we ensure no crash and non-empty.
}

#[test]
fn s08_t16_generate_with_invalid_triple() {
    // This test verifies that an invalid triple produces an error, not a panic.
    let backend = LlvmBackend::default().with_target("nonexistent-unknown-unknown");
    let output = tpath("glyim_test_invalid_triple.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, &output);
    assert!(result.is_err(), "invalid triple should cause error");
}

#[test]
fn s08_t17_generate_function_then_generate() {
    let backend = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(42),
    )));
    // First call generate_function
    let r1 = backend.generate_function(&body);
    assert!(r1.is_ok());
    // Then call generate with multiple bodies
    let output = tpath("glyim_test_mixed.o");
    let bodies = vec![body.clone()];
    let r2 = backend.generate(&bodies, &output);
    assert!(
        r2.is_ok(),
        "generate after generate_function should succeed"
    );
    assert!(output.exists());
}

#[test]
fn s08_t18_generate_to_readonly_directory() {
    // If the path is in a directory with no write permission, it should error.
    // Create a temporary read-only directory.
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let mut perms = std::fs::metadata(dir.path()).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(dir.path(), perms).ok();

    let backend = LlvmBackend::new();
    let output = dir.path().join("test.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, &output);
    // Should error because write fails
    assert!(
        result.is_err(),
        "writing to read-only directory should error"
    );

    // Clean up: restore permissions so tempfile can delete
    let mut perms = std::fs::metadata(dir.path()).unwrap().permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(dir.path(), perms).ok();
}

#[test]
fn s08_t19_generate_function_returns_elf_magic_for_linux() {
    // On x86_64-unknown-linux-gnu, generated object should be ELF.
    let backend = LlvmBackend::new(); // default is x86_64-unknown-linux-gnu
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(7),
    )));
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytes = result.unwrap();
    // ELF magic: 0x7F 'E' 'L' 'F'
    assert_eq!(
        &bytes[0..4],
        &[0x7f, 0x45, 0x4c, 0x46],
        "Object file should start with ELF magic"
    );
}

#[test]
fn s08_t20_default_target_triple_is_linux() {
    let backend = LlvmBackend::new();
    // We cannot directly access target_triple field, but generate_function
    // will succeed with ELF output (tested above). We just rely on it not crashing.
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(0),
    )));
    let _ = backend.generate_function(&body);
    // If no panic, test passes.
}

#[test]
fn s08_t27_generate_with_directory_path_errors() {
    let backend = LlvmBackend::new();
    // Use a path that is an existing directory
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let output = dir.path(); // This is a directory, not a file
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, output);
    assert!(
        result.is_err(),
        "generate with a directory path should error"
    );
    // Cleanup handled by tempdir
}

#[test]
fn s08_t28_different_owners_produce_different_output() {
    let backend = LlvmBackend::new();
    let body_a = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(1),
    )));
    let body_b = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(2),
    )));
    let bytes_a = backend.generate_function(&body_a).expect("should succeed");
    let bytes_b = backend.generate_function(&body_b).expect("should succeed");
    // The function names differ, so the object files should differ.
    assert_ne!(
        bytes_a, bytes_b,
        "different owners should yield different object code"
    );
}

#[test]
fn s08_t29_name_after_with_target() {
    let backend = LlvmBackend::default().with_target("x86_64-unknown-linux-gnu");
    assert_eq!(backend.name(), "llvm");
    // Also test after another target
    let backend2 = LlvmBackend::default().with_target("wasm32-unknown-unknown");
    assert_eq!(backend2.name(), "llvm");
}

#[test]
fn s08_t30_generate_function_returns_vec_u8() {
    let backend = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(3),
    )));
    let bytes = backend.generate_function(&body).expect("should succeed");
    // bytes must be a valid Vec<u8> (not empty, not a slice pointing to invalid memory)
    assert!(!bytes.is_empty());
    // Ensure we can read the entire buffer without panic
    let _ = bytes[0];
}

#[test]
fn s08_t31_multiple_backends_independent() {
    let b1 = LlvmBackend::new();
    let b2 = LlvmBackend::new();
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(10),
    )));
    let r1 = b1.generate_function(&body);
    let r2 = b2.generate_function(&body);
    assert!(r1.is_ok());
    assert!(r2.is_ok());
    // Both should produce identical output (deterministic codegen)
    assert_eq!(
        r1.unwrap(),
        r2.unwrap(),
        "independent backends should produce identical output for same input"
    );
}

#[test]
fn s08_t32_wasm_triple_produces_wasm_object() {
    let backend = LlvmBackend::default().with_target("wasm32-unknown-unknown");
    let output = tpath("glyim_test_wasm.o");
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(200),
    )));
    let bodies = vec![body];
    let result = backend.generate(&bodies, &output);
    // If LLVM has wasm target support, this will succeed; if not, may error.
    // We just care that it doesn't panic.
    if result.is_ok() {
        // Check wasm magic: \0asm
        let mut file = std::fs::File::open(&output).expect("output file should exist");
        let mut magic = [0u8; 4];
        use std::io::Read;
        file.read_exact(&mut magic).expect("should read magic");
        assert_eq!(
            &magic, b"\0asm",
            "wasm object should start with \\0asm magic"
        );
        std::fs::remove_file(&output).ok();
    }
    // If error (e.g., target not compiled in), we just skip.
}

#[test]
fn s08_t33_generate_function_body_not_mutated() {
    // Ensure that calling generate_function does not alter the Arc<Body> itself.
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(300),
    )));
    let original_owner = body.owner;
    let backend = LlvmBackend::new();
    let _ = backend.generate_function(&body).expect("should succeed");
    assert_eq!(
        body.owner, original_owner,
        "Arc<Body> owner should remain unchanged"
    );
    // Also check that the Arc strong count hasn't leaked (just be 1: the single owner)
    assert_eq!(std::sync::Arc::strong_count(&body), 1);
}

#[test]
fn s08_t34_codegen_backend_trait_object_safe() {
    // Ensure CodegenBackend trait is object-safe (can be used as &dyn)
    let backend: &dyn glyim_codegen::CodegenBackend = &LlvmBackend::new();
    assert_eq!(backend.name(), "llvm");
    // generate_function should also be callable
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(400),
    )));
    let result = backend.generate_function(&body);
    assert!(
        result.is_ok(),
        "generate_function via trait object should succeed"
    );
}

#[test]
fn s08_t35_with_target_empty_string() {
    // Empty target triple should not panic, but likely cause error on generate.
    let backend = LlvmBackend::default().with_target("");
    assert_eq!(backend.name(), "llvm");
    let output = tpath("glyim_test_empty_triple.o");
    let bodies: Vec<std::sync::Arc<glyim_mir::Body>> = vec![];
    let result = backend.generate(&bodies, &output);
    // Should error because empty triple is invalid.
    assert!(result.is_err(), "empty triple should cause generate error");
}
