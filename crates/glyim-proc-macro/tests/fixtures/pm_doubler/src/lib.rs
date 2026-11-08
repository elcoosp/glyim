//! End-to-end proc-macro fixture crate.
//!
//! This is a real, compilable proc-macro crate that exports the
//! `glyim_proc_macro_main` C-ABI entry point expected by
//! `glyim_proc_macro::load_cdylib`. It is intentionally dependency-free: it
//! re-declares the exact C-ABI types (`PmTokenStream`, `PmMacroFn`,
//! `PmRegisterFn`) so it can be compiled standalone by `rustc
//! --crate-type cdylib` during the round-trip test, with no workspace
//! dependency on `glyim_proc_macro` itself. The type layouts MUST stay in
//! sync with `crates/glyim-proc-macro/src/lib.rs` — that is the contract
//! `load_cdylib` relies on.
//!
//! Contract note: the host hands `expand` an output stream with `cap == 0`
//! (allocated by `pm_ts_alloc`). The dylib must populate `out` by allocating
//! its own buffer through the process-shared global allocator, because the
//! host later frees it via `pm_ts_free` (which calls `std::alloc::realloc`).
//! Using `std::alloc::alloc` here matches that free path exactly.

use std::alloc::{alloc, Layout};
use std::os::raw::c_char;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PmStr {
    pub ptr: *const u8,
    pub len: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PmToken {
    pub kind: u16,
    pub text: PmStr,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PmTokenStream {
    pub ptr: *mut PmToken,
    pub len: u32,
    pub cap: u32,
}

/// C-ABI signature of the host's register callback handed to the dylib.
/// MUST match `PmRegisterCallback` in `glyim_proc_macro` exactly:
/// `extern "C" fn(*mut RegistryHolder, *const c_char, PmMacroFn)`.
pub type PmRegisterFn = extern "C" fn(*mut std::ffi::c_void, *const c_char, PmMacroFn);

/// C-ABI signature of a registered macro entry point.
pub type PmMacroFn = extern "C" fn(*mut PmTokenStream, *mut PmTokenStream);

/// `glyim_proc_macro_main` — the single C-ABI entry point `load_cdylib`
/// looks up. Called once with a `register` callback; the dylib calls
/// `register` once per macro it provides.
///
/// Registers a single macro `reverse` that reverses the input token stream.
/// Reversal is observable (output != input for multi-token input) and needs
/// only `count` output slots, which this dylib allocates itself.
#[no_mangle]
pub extern "C" fn glyim_proc_macro_main(
    _reg: *mut std::ffi::c_void,
    register_cb: PmRegisterFn,
) {
    let name = std::ffi::CString::new("reverse").expect("macro name");
    let name_ptr = name.as_ptr();

    extern "C" fn expand(in_ts: *mut PmTokenStream, out_ts: *mut PmTokenStream) {
        unsafe {
            let input = &*in_ts;
            let count = input.len as usize;
            if count == 0 {
                (*out_ts).ptr = std::ptr::null_mut();
                (*out_ts).len = 0;
                (*out_ts).cap = 0;
                return;
            }
            let src = std::slice::from_raw_parts(input.ptr, count);
            // Allocate the output buffer through the process-shared global
            // allocator so the host's `pm_ts_free` (realloc) can free it.
            let layout = Layout::array::<PmToken>(count).expect("token layout");
            let dst = alloc(layout) as *mut PmToken;
            let dst_slice = std::slice::from_raw_parts_mut(dst, count);
            // Reverse the input into the output buffer.
            for i in 0..count {
                dst_slice[i] = src[count - 1 - i];
            }
            (*out_ts).ptr = dst;
            (*out_ts).len = count as u32;
            (*out_ts).cap = count as u32;
        }
    }

    register_cb(_reg, name_ptr, expand as PmMacroFn);
}

/// Keep `PmTokenStream` referenced so the layout is checked even if the macro
/// body above is the only user.
#[no_mangle]
pub extern "C" fn __pm_abi_marker(_ts: PmTokenStream) {}
