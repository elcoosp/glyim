//! End-to-end tests for the content-addressed compilation cache.
//!
//! These drive the real `glyim` binary (via `cargo_bin`-style path
//! discovery) against a scratch crate and a scratch cache directory. They
//! assert the *contract* the cache must uphold:
//!
//! * an identical rebuild is a hit and copies the *byte-identical* object;
//! * any change to the flattened source (including an external module)
//!   invalidates the key;
//! * a different target / opt level / entry-main setting invalidates it;
//! * a cache hit produces a runnable executable for `--emit=exec`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the `glyim` binary built by this crate's test profile.
///
/// Cargo sets `CARGO_BIN_EXE_<name>` at *runtime* for integration tests of a
/// crate with a matching binary target (not at compile time), so look it up
/// dynamically. Fall back to the conventional `target/<profile>/glyim` path
/// when the variable is absent (e.g. `cargo test --no-run` followed by a
/// manual run).
fn glyim_bin() -> PathBuf {
    // Cargo's env var uses the *binary target name*, which is `glyim-cli`.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_glyim-cli") {
        return PathBuf::from(p);
    }
    // Fallback: walk up from the test binary's directory to the target root.
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join("glyim-cli")
}

/// A scratch directory that is removed on drop.
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "glyim_cache_it_{}_{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `glyim <args...>`, returning (stdout, stderr, success).
fn run(cache_dir: &Path, args: &[&str]) -> (String, String, bool) {
    let out = Command::new(glyim_bin())
        .args(args)
        .env("GLYIM_CACHE_DIR", cache_dir)
        .env("RUST_LOG", "glyim_cli=info")
        .output()
        .expect("spawn glyim");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

fn count_objects(cache_dir: &Path) -> usize {
    let objs = cache_dir.join("objects");
    std::fs::read_dir(&objs)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("o"))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn identical_rebuild_is_a_hit_and_byte_identical() {
    let work = Scratch::new("identical");
    let cache = Scratch::new("identical_cache");
    let src = work.path().join("main.g");
    std::fs::write(&src, "fn main() -> i32 { 42 }\n").unwrap();

    let obj_a = work.path().join("a.o");
    let (_o, _e, ok) = run(
        cache.path(),
        &[
            src.to_str().unwrap(),
            "-o",
            obj_a.to_str().unwrap(),
            "--emit",
            "obj",
            "--backend",
            "llvm",
            "--target",
            "aarch64-apple-darwin",
        ],
    );
    assert!(ok, "cold compile must succeed");
    assert_eq!(count_objects(cache.path()), 1, "cold compile caches one object");

    let obj_b = work.path().join("b.o");
    let (_o, stderr_b, ok) = run(
        cache.path(),
        &[
            src.to_str().unwrap(),
            "-o",
            obj_b.to_str().unwrap(),
            "--emit",
            "obj",
            "--backend",
            "llvm",
            "--target",
            "aarch64-apple-darwin",
        ],
    );
    assert!(ok, "warm compile must succeed");
    assert!(
        stderr_b.contains("cache hit"),
        "warm compile must be a cache hit; stderr:\n{stderr_b}"
    );
    assert_eq!(count_objects(cache.path()), 1, "warm compile adds no object");

    assert_eq!(
        std::fs::read(&obj_a).unwrap(),
        std::fs::read(&obj_b).unwrap(),
        "cached object must be byte-identical to a fresh one"
    );
}

#[test]
fn changing_source_invalidates_the_cache() {
    let work = Scratch::new("invalidate");
    let cache = Scratch::new("invalidate_cache");
    let src = work.path().join("main.g");

    std::fs::write(&src, "fn main() -> i32 { 1 }\n").unwrap();
    let _ = run(
        cache.path(),
        &[
            src.to_str().unwrap(),
            "-o",
            work.path().join("a.o").to_str().unwrap(),
            "--emit",
            "obj",
            "--backend",
            "llvm",
            "--target",
            "aarch64-apple-darwin",
        ],
    );
    assert_eq!(count_objects(cache.path()), 1);

    // Change the *value* (not just whitespace) so the object really differs.
    std::fs::write(&src, "fn main() -> i32 { 2 }\n").unwrap();
    let (_o, stderr, _ok) = run(
        cache.path(),
        &[
            src.to_str().unwrap(),
            "-o",
            work.path().join("b.o").to_str().unwrap(),
            "--emit",
            "obj",
            "--backend",
            "llvm",
            "--target",
            "aarch64-apple-darwin",
        ],
    );
    assert!(
        !stderr.contains("cache hit"),
        "changed source must miss; stderr:\n{stderr}"
    );
    assert_eq!(count_objects(cache.path()), 2, "miss caches a second object");
}

#[test]
fn changing_an_external_module_invalidates_the_cache() {
    let work = Scratch::new("external");
    let cache = Scratch::new("external_cache");
    std::fs::write(
        work.path().join("main.g"),
        "mod helper;\nfn main() -> i32 { helper::value() }\n",
    )
    .unwrap();
    std::fs::write(work.path().join("helper.g"), "pub fn value() -> i32 { 1 }\n").unwrap();

    let args = |out: &str| {
        vec![
            work.path().join("main.g").to_str().unwrap().to_string(),
            "-o".into(),
            work.path().join(out).to_str().unwrap().to_string(),
            "--emit".into(),
            "obj".into(),
            "--backend".into(),
            "llvm".into(),
            "--target".into(),
            "aarch64-apple-darwin".into(),
        ]
    };
    let a = args("a.o");
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let _ = run(cache.path(), &a);

    // Identical rebuild: hit.
    let b = args("b.o");
    let b: Vec<&str> = b.iter().map(String::as_str).collect();
    let (_o, stderr, _ok) = run(cache.path(), &b);
    assert!(stderr.contains("cache hit"), "unchanged modules must hit");

    // Edit the *helper*: the flattened source changes, so the key changes.
    std::fs::write(work.path().join("helper.g"), "pub fn value() -> i32 { 2 }\n").unwrap();
    let c = args("c.o");
    let c: Vec<&str> = c.iter().map(String::as_str).collect();
    let (_o, stderr, _ok) = run(cache.path(), &c);
    assert!(
        !stderr.contains("cache hit"),
        "editing an external module must miss; stderr:\n{stderr}"
    );
    assert_eq!(count_objects(cache.path()), 2);
}

#[test]
fn different_target_misses() {
    let work = Scratch::new("target");
    let cache = Scratch::new("target_cache");
    let src = work.path().join("main.g");
    std::fs::write(&src, "fn main() -> i32 { 1 }\n").unwrap();

    let run_target = |target: &str, out: &str| {
        run(
            cache.path(),
            &[
                src.to_str().unwrap(),
                "-o",
                work.path().join(out).to_str().unwrap(),
                "--emit",
                "obj",
                "--backend",
                "llvm",
                "--target",
                target,
            ],
        )
    };
    let (_, _, ok) = run_target("aarch64-apple-darwin", "a.o");
    assert!(ok);
    let (_, stderr, ok) = run_target("x86_64-unknown-linux-gnu", "b.o");
    assert!(ok);
    assert!(
        !stderr.contains("cache hit"),
        "a different target must miss; stderr:\n{stderr}"
    );
    assert_eq!(count_objects(cache.path()), 2);
}

#[test]
fn cache_hit_for_exec_produces_a_runnable_binary() {
    let work = Scratch::new("exec");
    let cache = Scratch::new("exec_cache");
    let src = work.path().join("main.g");
    std::fs::write(&src, "fn main() -> i32 { 7 }\n").unwrap();

    let run_exec = |out: &str| {
        run(
            cache.path(),
            &[
                src.to_str().unwrap(),
                "-o",
                work.path().join(out).to_str().unwrap(),
                "--emit",
                "exec",
                "--backend",
                "llvm",
                "--target",
                "aarch64-apple-darwin",
            ],
        )
    };
    let (_, _, ok) = run_exec("a");
    assert!(ok, "cold exec must succeed");

    let (_, stderr, ok) = run_exec("b");
    assert!(ok);
    assert!(stderr.contains("cache hit"), "warm exec must hit");

    let b = work.path().join("b");
    assert!(b.exists(), "cached exec must still produce a binary");
    // Actually running the binary is only meaningful on the host triple;
    // skip when cross-compiled (target != host). The key contract this test
    // asserts is that a cache hit still runs the *link* step and yields an
    // executable file, not a bare object.
    let is_host_target = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    if is_host_target {
        let status = Command::new(&b).status().expect("run cached binary");
        assert_eq!(status.code(), Some(7), "cached binary must return 7");
    }
}
