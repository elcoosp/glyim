#![no_main]
// Target 2: full-pipeline ICE detector. Every input must compile to
// either (a) success, or (b) a Vec<GlyimDiagnostic> — never a panic.
// This is the fuzz-side twin of the no-ice corpus (30 hand-written
// hostile fixtures) with unbounded creativity.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    if src.len() > 256 * 1024 {
        return;
    }
    let _ = std::fs::write("/tmp/glyim-fuzz-last-input.g", src);
    // compile_file_to_mir takes a path; write the input to a scratch file
    // so the pipeline sees a normal source file.
    let scratch = std::env::temp_dir().join("glyim-fuzz-compile-input.g");
    let _ = std::fs::write(&scratch, src);
    // The contract: Result<_, Vec<GlyimDiagnostic>>. A panic inside the
    // pipeline is the bug libFuzzer reports. Do not unwrap the result.
    let config = glyim_db::CrateConfig {
        name: "fuzz".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = glyim_db::Database::new(config);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        glyim_pipeline::compile_file_to_mir(&mut db, &scratch)
    }));
    if result.is_err() {
        // A panic escaped the pipeline: this is exactly the class of bug
        // the no-ice corpus hunts. Let libFuzzer record the crash.
        panic!(
            "pipeline panicked on input saved at {}",
            scratch.display()
        );
    }
});
