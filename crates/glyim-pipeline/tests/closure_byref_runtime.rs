//! HIR-31: `ByRef`/`ByRef(Mut)` closure captures must **alias** the enclosing
//! binding, not copy it.
//!
//! `let mut c = 0; let mut f = || { c = c + 1; }; f(); f();` must observe
//! `c == 2`. Before the fix the environment field held a by-value copy, so `c`
//! never changed. This test compiles through the real pipeline (typeck ->
//! THIR -> MIR) and executes `main` in the in-process interpreter.

use std::io::Write;
use std::sync::Arc;

use glyim_db::Database;
use glyim_mir_interp::{InterpValue, Interpreter};
use glyim_pipeline::compile_file_to_mir;

fn compile_and_run(src: &str) -> Result<InterpValue, String> {
    static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call_id = CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let unique_tag = format!("{}_{}", std::process::id(), call_id);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("glyim_closure_byref_{}.g", unique_tag));
    {
        let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        f.write_all(src.as_bytes()).map_err(|e| e.to_string())?;
    }

    let config = glyim_db::CrateConfig {
        name: "test_crate".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    let mir = compile_file_to_mir(&mut db, &path).map_err(|diags| {
        diags
            .iter()
            .map(|d| format!("{:?}: {}", d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let ty_ctx = mir.ty_ctx;
    let bodies: Vec<Arc<glyim_mir::Body>> = mir.bodies.values().cloned().collect();

    let main_local = glyim_pipeline::Pipeline::entry_main_local_id(&mut db, &path)
        .ok_or_else(|| "could not resolve main entry".to_string())?;
    let main_id = glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_raw(0),
        glyim_core::def_id::LocalDefId::from_raw(main_local),
    );
    let main_body = mir
        .bodies
        .get(&main_id)
        .ok_or_else(|| "main body not found".to_string())?;

    let mut interp = Interpreter::new(ty_ctx.as_ref());
    for b in &bodies {
        interp.add_function(b.owner, (**b).clone());
    }
    interp
        .run_body(main_body)
        .map_err(|e| format!("interpreter error: {}", e))?;
    interp
        .get_return_value()
        .ok_or_else(|| "no return value".to_string())
}

#[test]
fn byref_mut_capture_aliases_enclosing_local() {
    // c starts 0; two `f()` calls each `c = c + 1`; `c` must be 2.
    let src = r#"
fn main() -> i32 {
    let mut c = 0;
    let mut f = || { c = c + 1; };
    f();
    f();
    c
}
"#;
    let ret = compile_and_run(src).expect("program compiles and runs");
    assert_eq!(
        ret,
        InterpValue::Int(2),
        "by-ref `&mut` capture must alias: two increments must leave c == 2"
    );
}

#[test]
fn byref_shared_capture_reads_live_value() {
    // The closure reads `c` after `c` is mutated outside it.
    let src = r#"
fn main() -> i32 {
    let mut c = 5;
    let g = || { c };
    c = 42;
    g()
}
"#;
    let ret = compile_and_run(src).expect("program compiles and runs");
    assert_eq!(
        ret,
        InterpValue::Int(42),
        "shared by-ref capture must observe the enclosing local's current value"
    );
}
