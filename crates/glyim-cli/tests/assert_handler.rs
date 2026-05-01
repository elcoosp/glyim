//! Longjmp-based abort handler for JIT tests.
//! Uses raw FFI to setjmp/longjmp to avoid libc crate API issues.

use std::sync::atomic::{AtomicBool, Ordering};

// jmp_buf is platform-dependent. 64 words is enough for all common platforms.
const JMP_BUF_WORDS: usize = 64;
static mut JMP_BUF: [usize; JMP_BUF_WORDS] = [0; JMP_BUF_WORDS];
static BUF_INIT: AtomicBool = AtomicBool::new(false);

extern "C" {
    fn setjmp(buf: *mut usize) -> libc::c_int;
    fn longjmp(buf: *mut usize, val: libc::c_int) -> !;
}

/// Call this before running pipeline::run to catch assertion failures.
/// Returns true after a longjmp (assert triggered), false on first entry.
pub fn setup_assert_catcher() -> bool {
    BUF_INIT.store(true, Ordering::SeqCst);
    let ret = unsafe { setjmp(JMP_BUF.as_mut_ptr()) };
    ret != 0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glyim_assert_fail_test_impl(_msg: *const u8, _len: i64) {
    if BUF_INIT.load(Ordering::SeqCst) {
        unsafe { longjmp(JMP_BUF.as_mut_ptr(), 1) };
    } else {
        std::process::abort();
    }
}
