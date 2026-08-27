//! Native threads for the Glyim standard library.
//!
//! This module contains primitives for spawning and managing threads.

/// Holds the eventual result of a spawned thread. Allocated once by `spawn`,
/// written once by the trampoline running on the new OS thread, and read once
/// by `join` on the joining thread. `glyim_thread_join` blocks until the OS
/// thread has exited, which is the synchronization point that makes reading
/// `result` in `join` safe without extra locking (mirrors `std::thread`).
struct ResultSlot<T> {
    result: Option<Result<T, String>>,
}

/// A handle to a thread.
struct JoinHandle<T> {
    thread_id: ThreadId,
    // Raw pointer to a leaked `Box<ResultSlot<T>>`. Reclaimed exactly once,
    // in `join`.
    slot: *mut u8,
    _marker: PhantomData<T>,
}

impl<T> JoinHandle<T> {
    /// Wait for the associated thread to finish and retrieve its result.
    fn join(self) -> Result<T, Box<dyn Any + Send>> {
        extern "C" {
            fn glyim_thread_join(thread_id: u64) -> i32;
        }
        let rc = unsafe { glyim_thread_join(self.thread_id.to_u64()) };

        // SAFETY: `self.slot` was produced by `Box::into_raw(Box::new(
        // ResultSlot { .. }))` in `spawn`/`Builder::spawn` and has not been
        // converted back to a `Box` since. `glyim_thread_join` only returns
        // after the OS thread's entry function has returned, and the
        // trampoline writes `result` as the very last thing it does before
        // returning — so this read happens-after that write.
        let slot_box: Box<ResultSlot<T>> = unsafe { Box::from_raw(self.slot as *mut ResultSlot<T>) };
        let slot = *slot_box; // move out, drop the box

        if rc != 0 {
            return Result::Err(Box::new("thread panicked".to_string()));
        }
        match slot.result {
            Option::Some(Result::Ok(v)) => Result::Ok(v),
            Option::Some(Result::Err(msg)) => Result::Err(Box::new(msg)),
            Option::None => Result::Err(Box::new("thread exited without producing a result".to_string())),
        }
    }

    /// Return the thread ID of this handle.
    fn thread_id(&self) -> ThreadId {
        self.thread_id
    }
}

/// A unique identifier for a running thread.
struct ThreadId {
    id: u64,
}

impl ThreadId {
    /// Create a new `ThreadId` from a raw value.
    fn from_u64(id: u64) -> ThreadId {
        ThreadId { id }
    }

    /// Convert to a raw u64 value.
    fn to_u64(&self) -> u64 {
        self.id
    }
}

/// A handle to a thread.
struct Thread {
    id: ThreadId,
    name: Option<String>,
}

impl Thread {
    /// Get the unique identifier for this thread.
    fn id(&self) -> ThreadId {
        self.id
    }

    /// Get the name of this thread.
    fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_str())
    }
}

/// Payload boxed once and handed across the FFI boundary as a single
/// `*mut u8`. The trampoline below is monomorphized per `<F, T>` instantiation
/// by the compiler, so taking its address as an `extern "C" fn(*mut u8)` is a
/// concrete, ABI-stable function pointer — not a generic one — by the time
/// codegen runs.
struct SpawnPayload<F, T> {
    f: Option<F>,
    slot: *mut u8, // *mut ResultSlot<T>, boxed and leaked by the spawning side
}

extern "C" fn thread_trampoline<F, T>(arg: *mut u8)
where
    F: FnOnce() -> T,
{
    // SAFETY: `arg` was produced by `Box::into_raw(Box::new(SpawnPayload { .. }))`
    // in `spawn`/`Builder::spawn`, is passed to exactly one OS thread, and this
    // trampoline runs exactly once for it.
    let payload: Box<SpawnPayload<F, T>> = unsafe { Box::from_raw(arg as *mut SpawnPayload<F, T>) };
    let mut payload = *payload;
    let f = payload.f.take().expect("thread trampoline invoked with no closure");

    // Glyim has no unwinding exceptions, so running `f()` directly is safe:
    // a panic aborts the whole process (mirroring `std`'s `catch_unwind`
    // contract at the boundary — the spawned thread's panic terminates the
    // program, and `join` reports it via the non-zero return code below).
    let result = f();

    // SAFETY: same pointer/lifetime contract as in `JoinHandle::join` above;
    // the slot is still alive because `join` has not run yet (it can't: this
    // OS thread hasn't exited).
    let mut slot: Box<ResultSlot<T>> = unsafe { Box::from_raw(payload.slot as *mut ResultSlot<T>) };
    slot.result = Option::Some(Result::Ok(result));
    // Leak it back out: `join` reclaims ownership via `Box::from_raw`.
    Box::into_raw(slot);
}

fn spawn_impl<F, T>(f: F, name: Option<String>, stack_size: Option<usize>) -> Result<JoinHandle<T>>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    extern "C" {
        fn glyim_thread_spawn(f: extern "C" fn(*mut u8), arg: *mut u8) -> usize;
        fn glyim_thread_spawn_named(
            name: *const u8,
            name_len: usize,
            stack_size: usize,
            f: extern "C" fn(*mut u8),
            arg: *mut u8,
        ) -> usize;
    }

    let slot_ptr = Box::into_raw(Box::new(ResultSlot::<T> { result: Option::None })) as *mut u8;
    let payload_ptr = Box::into_raw(Box::new(SpawnPayload::<F, T> {
        f: Option::Some(f),
        slot: slot_ptr,
    })) as *mut u8;

    let id = match &name {
        Option::Some(n) => unsafe {
            glyim_thread_spawn_named(
                n.as_ptr(),
                n.len(),
                stack_size.unwrap_or(DEFAULT_STACK_SIZE),
                thread_trampoline::<F, T>,
                payload_ptr,
            )
        },
        Option::None => unsafe { glyim_thread_spawn(thread_trampoline::<F, T>, payload_ptr) },
    };

    if id == 0 {
        // Spawn failed before the trampoline could run: reclaim both boxes
        // ourselves so nothing leaks.
        unsafe {
            let _ = Box::from_raw(payload_ptr as *mut SpawnPayload<F, T>);
            let _ = Box::from_raw(slot_ptr as *mut ResultSlot<T>);
        }
        return Result::Err("failed to spawn thread".into());
    }

    Result::Ok(JoinHandle {
        thread_id: ThreadId::from_u64(id as u64),
        slot: slot_ptr,
        _marker: PhantomData,
    })
}

/// Spawn a new thread, returning a `JoinHandle` for it.
///
/// The closure `f` is the function to execute in the new thread.
fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    spawn_impl(f, Option::None, Option::None).expect("failed to spawn thread")
}

/// Get a handle to the current thread.
fn current() -> Thread {
    extern "C" {
        fn glyim_thread_current_id() -> u64;
    }
    let id = unsafe { glyim_thread_current_id() };
    Thread {
        id: ThreadId::from_u64(id),
        name: Option::None,
    }
}

/// Get the current thread's unique identifier.
fn current_id() -> ThreadId {
    current().id()
}

/// Cooperatively gives up a timeslice to the OS scheduler.
fn yield_now() {
    extern "C" {
        fn glyim_thread_yield();
    }
    unsafe { glyim_thread_yield() }
}

/// Put the current thread to sleep for at least the specified amount of time.
fn sleep(dur: Duration) {
    extern "C" {
        fn glyim_thread_sleep(secs: u64, nanos: u32);
    }
    unsafe { glyim_thread_sleep(dur.as_secs(), dur.subsec_nanos()) }
}

/// Block the current thread until the specified duration has elapsed.
fn park_timeout(dur: Duration) {
    extern "C" {
        fn glyim_thread_park_timeout(secs: u64, nanos: u32);
    }
    unsafe { glyim_thread_park_timeout(dur.as_secs(), dur.subsec_nanos()) }
}

/// Block the current thread unless or until the token is available.
fn park() {
    extern "C" {
        fn glyim_thread_park();
    }
    unsafe { glyim_thread_park() }
}

/// Atomically makes the token available if it is not already.
fn unpark(thread: &Thread) {
    extern "C" {
        fn glyim_thread_unpark(thread_id: u64);
    }
    unsafe { glyim_thread_unpark(thread.id.to_u64()) }
}

/// Determine whether to give up a timeslice based on a hint.
fn hint_spin_loop() {
    extern "C" {
        fn glyim_thread_spin_loop_hint();
    }
    unsafe { glyim_thread_spin_loop_hint() }
}

/// The maximum number of threads that can be spawned.
const MAX_THREADS: usize = 65536;

/// The default stack size for new threads.
const DEFAULT_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Builder for configuring thread spawning.
struct Builder {
    name: Option<String>,
    stack_size: Option<usize>,
}

impl Builder {
    /// Create a new thread builder with default settings.
    fn new() -> Builder {
        Builder {
            name: Option::None,
            stack_size: Option::None,
        }
    }

    /// Set the name for the new thread.
    fn name(mut self, name: String) -> Builder {
        self.name = Option::Some(name);
        self
    }

    /// Set the stack size for the new thread.
    fn stack_size(mut self, size: usize) -> Builder {
        self.stack_size = Option::Some(size);
        self
    }

    /// Spawn a new thread with the configured settings.
    fn spawn<F, T>(self, f: F) -> Result<JoinHandle<T>>
    where
        F: FnOnce() -> T,
        F: Send + 'static,
        T: Send + 'static,
    {
        spawn_impl(f, self.name, self.stack_size)
    }
}

/// The number of logical cores available.
fn available_parallelism() -> Result<usize> {
    extern "C" {
        fn glyim_thread_available_parallelism() -> usize;
    }
    let n = unsafe { glyim_thread_available_parallelism() };
    if n == 0 {
        Result::Err("could not determine available parallelism".into())
    } else {
        Result::Ok(n)
    }
}
