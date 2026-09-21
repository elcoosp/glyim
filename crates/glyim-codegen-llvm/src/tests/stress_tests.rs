use crate::LlvmBackend;
use glyim_codegen::CodegenBackend;
use std::path::Path;
use std::thread;

/// Per-process unique temp file path. Prevents concurrent test processes
/// (a second `cargo nextest` invocation, `cargo-mutants` workers, parallel
/// CI shards on the same host) from racing on a fixed `/tmp/glyim_test_*.o`
/// path — one process's `remove_file` was racing another's write+read, which
/// produced spurious `output.exists()` / `metadata()` failures.
fn tpath(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("glyim_test_{}_{}", std::process::id(), name))
}

#[test]
fn s08_t36_concurrent_generate_independent_backends() {
    let handles: Vec<_> = (0..4)
        .map(|i| {
            thread::spawn(move || {
                let backend = LlvmBackend::new();
                let output_path = tpath(&format!("glyim_concurrent_{}.o", i));
                let output = Path::new(&output_path);
                let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
                    glyim_core::CrateId::from_raw(0),
                    glyim_core::LocalDefId::from_raw(i * 1000),
                )));
                let bodies = vec![body];
                let result = backend.generate(&bodies, &output);
                assert!(result.is_ok(), "concurrent generate in thread {} failed", i);
                // Clean up
                std::fs::remove_file(&output).ok();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn s08_t37_concurrent_generate_function_independent() {
    let handles: Vec<_> = (0..4)
        .map(|i| {
            thread::spawn(move || {
                let backend = LlvmBackend::new();
                let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
                    glyim_core::CrateId::from_raw(0),
                    glyim_core::LocalDefId::from_raw(i * 2000),
                )));
                let result = backend.generate_function(&body);
                assert!(
                    result.is_ok(),
                    "concurrent generate_function in thread {} failed",
                    i
                );
                let bytes = result.unwrap();
                assert!(!bytes.is_empty());
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn s08_t38_rapid_backend_creation_and_drop() {
    // Create and drop backends rapidly, ensuring no resource leak.
    for i in 0..20 {
        let backend = LlvmBackend::new();
        let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(i),
        )));
        let _ = backend.generate_function(&body);
        // backend drops here
    }
}

#[test]
fn s08_t39_generate_to_file_then_overwrite() {
    let backend = LlvmBackend::new();
    let output = tpath("glyim_overwrite.o");
    let body = std::sync::Arc::new(glyim_mir::Body::dummy(glyim_core::DefId::new(
        glyim_core::CrateId::from_raw(0),
        glyim_core::LocalDefId::from_raw(999),
    )));
    let bodies = vec![body.clone()];
    backend.generate(&bodies, &output).expect("first write");
    let first_size = std::fs::metadata(&output).expect("file should exist").len();
    // Overwrite same file
    backend.generate(&bodies, &output).expect("overwrite");
    let second_size = std::fs::metadata(&output).expect("file should exist").len();
    // Size should be similar (identical object, same target).
    assert_eq!(
        first_size, second_size,
        "overwriting should produce consistent size"
    );
    std::fs::remove_file(&output).ok();
}
