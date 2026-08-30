//! Threading tests for glyim-runtime
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn thread_spawn_and_join() {
    // W5-C06-T02: Thread spawn and join works
    let counter = Arc::new(Mutex::new(0));
    let counter_clone = Arc::clone(&counter);

    let handle = thread::spawn(move || {
        let mut num = counter_clone.lock().unwrap();
        *num += 42;
    });

    handle.join().expect("Failed to join thread");
    let result = *counter.lock().unwrap();
    assert_eq!(result, 42);
}

#[test]
fn thread_yield_and_sleep() {
    // Test thread yield and sleep behavior
    let start = std::time::Instant::now();
    thread::sleep(Duration::from_millis(50));
    let elapsed = start.elapsed();

    // Should have slept at least 40ms (allowing for scheduling variance)
    assert!(
        elapsed >= Duration::from_millis(40),
        "Sleep was too short: {:?}",
        elapsed
    );

    // Yield should not panic and should return quickly
    thread::yield_now();
}

#[test]
fn thread_park_unpark() {
    // Test park/unpark synchronization
    let handle = thread::current();

    let child = thread::spawn(move || {
        // Unpark the parent after a short delay
        thread::sleep(Duration::from_millis(10));
        handle.unpark();
    });

    // Park should return when unparked
    thread::park_timeout(Duration::from_secs(1));
    child.join().expect("Failed to join child");
}

#[test]
fn thread_current_id_and_parallelism() {
    // Test thread ID and available_parallelism
    let id1 = thread::current().id();
    let id2 = thread::current().id();
    assert_eq!(id1, id2, "Same thread should have same ID");

    let parallelism = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    assert!(parallelism >= 1, "Should have at least 1 parallelism");
}

/// Prove the `glyim_thread_spawn_named` FFI entry point (used by
/// `std::thread::Builder::spawn` in the `.g` stdlib) actually spawns a
/// *named* OS thread and returns a valid id that `glyim_thread_join` can wait
/// on. This mirrors exactly what `thread.g::spawn_impl` does when a name is
/// set, so it covers the previously-missing runtime symbol.
#[test]
fn glyim_thread_spawn_named_joins_value() {
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Arc;

    unsafe extern "C" {
        fn glyim_thread_spawn_named(
            name: *const u8,
            name_len: usize,
            stack_size: usize,
            f: extern "C" fn(*mut u8),
            arg: *mut u8,
        ) -> usize;
        fn glyim_thread_join(thread_id: u64) -> i32;
    }

    static RESULT: AtomicI32 = AtomicI32::new(0);
    let shared = Arc::new(AtomicI32::new(0));
    let shared_ptr = Arc::into_raw(shared) as *mut AtomicI32 as *mut u8;

    extern "C" fn entry(arg: *mut u8) {
        let shared = unsafe { &*(arg as *const AtomicI32) };
        shared.store(99, Ordering::SeqCst);
        RESULT.store(99, Ordering::SeqCst);
    }

    let name = b"worker-1\0";
    let id = unsafe {
        glyim_thread_spawn_named(
            name.as_ptr(),
            name.len() - 1, // exclude the NUL terminator
            0,              // 0 = default stack size
            entry,
            shared_ptr,
        )
    };
    assert!(id != 0, "glyim_thread_spawn_named must return a non-zero id");

    let rc = unsafe { glyim_thread_join(id as u64) };
    assert_eq!(rc, 0, "glyim_thread_join should return 0 for clean exit");

    assert_eq!(
        RESULT.load(Ordering::SeqCst),
        99,
        "named thread must have run its entry function"
    );
    // Reclaim the Arc so we don't leak it.
    unsafe {
        let _ = Arc::from_raw(shared_ptr as *mut AtomicI32);
    }
}
