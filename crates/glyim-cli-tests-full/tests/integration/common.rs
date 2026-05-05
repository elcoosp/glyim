#[allow(unused_imports)]
pub use glyim_cli::pipeline;
pub use glyim_cli::pipeline::BuildMode;
pub use std::path::PathBuf;
pub use std::sync::Mutex;

unsafe extern "C" {
    pub fn setjmp(buf: *mut usize) -> i32;
    pub fn longjmp(buf: *mut usize, val: i32) -> !;
}

pub static JMP_BUF: Mutex<[usize; 64]> = Mutex::new([0; 64]);
pub static ASSERT_FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub unsafe extern "C" fn assert_handler_impl(_msg: *const u8, _len: i64) {
    ASSERT_FIRED.store(true, std::sync::atomic::Ordering::SeqCst);
    unsafe { longjmp(JMP_BUF.lock().unwrap().as_mut_ptr(), 1) };
}

pub unsafe extern "C" fn abort_handler_impl() {
    ASSERT_FIRED.store(true, std::sync::atomic::Ordering::SeqCst);
    unsafe { longjmp(JMP_BUF.lock().unwrap().as_mut_ptr(), 1) };
}

pub fn run_with_abort_catcher<F: FnOnce() -> i32>(f: F) -> i32 {
    let ret = unsafe { setjmp(JMP_BUF.lock().unwrap().as_mut_ptr()) };
    if ret != 0 { return 1; }
    f()
}

pub fn temp_g(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}
