//! Threading tests for glyim-runtime

use crate::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[test]
fn thread_spawn_and_join() {
    let flag = Arc::new(AtomicBool::new(false));
    let flag_ptr = Arc::into_raw(flag.clone()) as *mut u8;
    extern "C" fn thread_func(arg: *mut u8) {
        let flag = unsafe { Arc::from_raw(arg as *const AtomicBool) };
        flag.store(true, Ordering::SeqCst);
        std::mem::forget(flag);
    }
    let handle = unsafe { glyim_thread_spawn(thread_func, flag_ptr) };
    assert_ne!(handle, 0);
    let ret = unsafe { glyim_thread_join(handle) };
    assert_eq!(ret, 0);
    assert!(flag.load(Ordering::SeqCst));
}

#[test]
fn thread_yield_and_sleep() {
    unsafe {
        glyim_thread_yield();
        glyim_thread_sleep(0, 10_000_000);
    }
}

#[test]
fn thread_park_unpark() {
    let flag = Arc::new(AtomicBool::new(false));
    let flag_ptr = Arc::into_raw(flag.clone()) as *mut u8;
    extern "C" fn parked_thread(arg: *mut u8) {
        let flag = unsafe { Arc::from_raw(arg as *const AtomicBool) };
        std::thread::park();
        flag.store(true, Ordering::SeqCst);
        std::mem::forget(flag);
    }
    let handle = unsafe { glyim_thread_spawn(parked_thread, flag_ptr) };
    std::thread::sleep(Duration::from_millis(50));
    unsafe { glyim_thread_unpark(handle) };
    let ret = unsafe { glyim_thread_join(handle) };
    assert_eq!(ret, 0);
    assert!(flag.load(Ordering::SeqCst));
}

#[test]
fn thread_current_id() {
    let id = unsafe { glyim_thread_current_id() };
    assert!(id != 0);
}

#[test]
fn thread_available_parallelism() {
    let n = unsafe { glyim_thread_available_parallelism() };
    assert!(n >= 1);
}

/// RT-21 regression: `glyim_thread_current_id` must return an id in the SAME
/// space as `glyim_thread_spawn`'s handle and `glyim_thread_unpark`'s key.
/// It used to return `pthread_self()` (an address), which is never a store
/// key, so the async reactor's `unpark(thread_id)` calls silently no-op'd and
/// all socket I/O hung.
#[test]
fn thread_current_id_matches_spawn_id_and_is_unparkable() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // The spawned thread records `current_id()`; the parent compares it to the
    // handle `glyim_thread_spawn` returned.
    static OBSERVED_ID: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn record_id(_arg: *mut u8) {
        let id = unsafe { glyim_thread_current_id() };
        OBSERVED_ID.store(id, Ordering::SeqCst);
    }

    let handle = unsafe { glyim_thread_spawn(record_id, std::ptr::null_mut()) };
    assert_ne!(handle, 0, "spawn must return a non-zero id");
    let _ = unsafe { glyim_thread_join(handle) };

    let observed = OBSERVED_ID.load(Ordering::SeqCst);
    assert_eq!(
        observed, handle,
        "the spawned thread's current_id() must equal the handle spawn returned \
         (RT-21: same id space as unpark)"
    );
}

/// The current (main) thread must get a non-zero id that is stable across
/// calls, and `unpark` on it must not panic (it is registered).
#[test]
fn main_thread_current_id_is_nonzero_and_stable() {
    let id1 = unsafe { glyim_thread_current_id() };
    let id2 = unsafe { glyim_thread_current_id() };
    assert_ne!(id1, 0, "current_id must be non-zero");
    assert_eq!(id1, id2, "current_id must be stable within a thread");
    // Registered ⇒ unpark finds it (no-op here, but must not panic / miss).
    unsafe { glyim_thread_unpark(id1) };
}
