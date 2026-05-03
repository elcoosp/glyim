use std::process::Command;
use std::sync::Once;

static START_SERVER: Once = Once::new();
static mut SERVER_PID: Option<u32> = None;

/// Start the CAS server once per test run.
fn ensure_server() {
    unsafe {
        START_SERVER.call_once(|| {
            let server_path = format!(
                "{}/../../target/release/glyim-cas-server",
                env!("CARGO_MANIFEST_DIR")
            );
            let child = Command::new(&server_path)
                .spawn()
                .expect("failed to start CAS server");
            std::thread::sleep(std::time::Duration::from_secs(1));
            SERVER_PID = Some(child.id());
            std::mem::forget(child);
        });
    }
}

#[test]
fn two_machines_share_macro_cache() {
    ensure_server();

    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let source = "@identity(main = () => 42)";
    let main_g_a = dir_a.path().join("main.g");
    let main_g_b = dir_b.path().join("main.g");
    std::fs::write(&main_g_a, source).unwrap();
    std::fs::write(&main_g_b, source).unwrap();

    let glyim = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/release/glyim");

    let status_a = Command::new(glyim)
        .arg("run")
        .arg(&main_g_a)
        .status()
        .expect("run on machine A");
    let status_b = Command::new(glyim)
        .arg("run")
        .arg(&main_g_b)
        .status()
        .expect("run on machine B");

    let code_a = status_a.code().unwrap_or(-1);
    let code_b = status_b.code().unwrap_or(-1);

    assert_eq!(code_a, 42, "machine A should return 42");
    assert_eq!(code_b, 42, "machine B should return 42 (cache hit)");
}
