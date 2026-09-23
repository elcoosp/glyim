//! Synchronization primitives for the Glyim standard library.
//!
//! This module contains primitives for synchronizing access to shared data
//! across multiple threads.

/// A mutual exclusion primitive useful for protecting shared data.
struct Mutex<T> {
    inner: UnsafeCell<MutexInner>,
    _marker: PhantomData<T>,
}

/// Inner state for the mutex.
struct MutexInner {
    locked: AtomicBool,
    _padding: [u8; 63],
}

impl<T> Mutex<T> {
    /// Create a new mutex in an unlocked state ready for use.
    fn new(t: T) -> Mutex<T> {
        Mutex {
            inner: UnsafeCell::new(MutexInner {
                locked: AtomicBool::new(false),
                _padding: [0u8; 63],
            }),
            _marker: PhantomData,
        }
    }

    /// Acquire the mutex, blocking the current thread until it is able to do so.
    fn lock(&self) -> MutexGuard<T> {
        extern "C" {
            fn glyim_mutex_lock(mutex: *const u8) -> i32;
        }
        while self.inner().locked.compare_exchange(false, true, Ordering::Acquire).is_err() {
            // Spin and yield
            thread::yield_now();
        }
        MutexGuard { mutex: self }
    }

    /// Attempt to acquire the mutex without blocking.
    fn try_lock(&self) -> Option<MutexGuard<T>> {
        if self.inner().locked.compare_exchange(false, true, Ordering::Acquire).is_ok() {
            Option::Some(MutexGuard { mutex: self })
        } else {
            Option::None
        }
    }

    /// Returns a mutable reference to the underlying data.
    fn get_mut(&mut self) -> &mut T {
        self.inner().locked.store(false, Ordering::Relaxed);
        // SAFETY: we have &mut self, so no other references exist
        unsafe { &mut *(self.inner.get() as *mut T) }
    }

    /// Consume the mutex, returning the underlying data.
    fn into_inner(self) -> T {
        // SAFETY: the mutex is consumed, so no other references exist
        unsafe { self.inner.into_inner() as T }
    }

    fn inner(&self) -> &MutexInner {
        // SAFETY: the inner is always valid
        unsafe { &*self.inner.get() }
    }
}

/// An RAII implementation of a "scoped lock" of a mutex.
struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.inner().locked.store(false, Ordering::Release);
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: we hold the lock
        unsafe { &*(self.mutex.inner.get() as *const T) }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: we hold the lock
        unsafe { &mut *(self.mutex.inner.get() as *mut T) }
    }
}

/// A reader-writer lock, allowing multiple readers or a single writer.
struct RwLock<T> {
    inner: UnsafeCell<RwLockInner>,
    _marker: PhantomData<T>,
}

struct RwLockInner {
    read_count: AtomicUsize,
    write_locked: AtomicBool,
}

impl<T> RwLock<T> {
    /// Create a new RwLock in an unlocked state.
    fn new(t: T) -> RwLock<T> {
        RwLock {
            inner: UnsafeCell::new(RwLockInner {
                read_count: AtomicUsize::new(0),
                write_locked: AtomicBool::new(false),
            }),
            _marker: PhantomData,
        }
    }

    /// Lock this rwlock with shared read access.
    fn read(&self) -> RwLockReadGuard<T> {
        while self.inner().write_locked.load(Ordering::Acquire) {
            thread::yield_now();
        }
        self.inner().read_count.fetch_add(1, Ordering::Acquire);
        RwLockReadGuard { lock: self }
    }

    /// Lock this rwlock with exclusive write access.
    fn write(&self) -> RwLockWriteGuard<T> {
        while self.inner().write_locked.compare_exchange(false, true, Ordering::Acquire).is_err() {
            thread::yield_now();
        }
        while self.inner().read_count.load(Ordering::Acquire) > 0 {
            thread::yield_now();
        }
        RwLockWriteGuard { lock: self }
    }

    /// Attempt to acquire a read lock without blocking.
    fn try_read(&self) -> Option<RwLockReadGuard<T>> {
        if self.inner().write_locked.load(Ordering::Acquire) {
            return Option::None;
        }
        self.inner().read_count.fetch_add(1, Ordering::Acquire);
        if self.inner().write_locked.load(Ordering::Acquire) {
            self.inner().read_count.fetch_sub(1, Ordering::Release);
            return Option::None;
        }
        Option::Some(RwLockReadGuard { lock: self })
    }

    /// Attempt to acquire a write lock without blocking.
    fn try_write(&self) -> Option<RwLockWriteGuard<T>> {
        if self.inner().write_locked.compare_exchange(false, true, Ordering::Acquire).is_err() {
            return Option::None;
        }
        if self.inner().read_count.load(Ordering::Acquire) > 0 {
            self.inner().write_locked.store(false, Ordering::Release);
            return Option::None;
        }
        Option::Some(RwLockWriteGuard { lock: self })
    }

    fn inner(&self) -> &RwLockInner {
        unsafe { &*self.inner.get() }
    }
}

/// RAII structure used to release the shared read access of a lock when dropped.
struct RwLockReadGuard<'a, T> {
    lock: &'a RwLock<T>,
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.inner().read_count.fetch_sub(1, Ordering::Release);
    }
}

impl<T> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*(self.lock.inner.get() as *const T) }
    }
}

/// RAII structure used to release the exclusive write access of a lock when dropped.
struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLock<T>,
}

impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.inner().write_locked.store(false, Ordering::Release);
    }
}

impl<T> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*(self.lock.inner.get() as *const T) }
    }
}

impl<T> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *(self.lock.inner.get() as *mut T) }
    }
}

/// A synchronization primitive which can be used to run a one-time initialization.
struct OnceLock {
    state: AtomicU8,
}

impl OnceLock {
    /// Create a new `OnceLock` value.
    fn new() -> OnceLock {
        OnceLock {
            state: AtomicU8::new(0),
        }
    }

    /// Perform the initialization exactly once.
    fn call_once<F>(&self, f: F)
    where
        F: FnOnce(),
    {
        if self.state.compare_exchange(0, 1, Ordering::Acquire).is_ok() {
            f();
            self.state.store(2, Ordering::Release);
        } else {
            while self.state.load(Ordering::Acquire) == 1 {
                thread::yield_now();
            }
        }
    }
}

/// A primitive for signaling between threads.
struct Condvar {
    waiters: AtomicUsize,
}

impl Condvar {
    /// Create a new condition variable.
    fn new() -> Condvar {
        Condvar {
            waiters: AtomicUsize::new(0),
        }
    }

    /// Block the current thread until this condition variable receives a notification.
    fn wait(&self, mutex_guard: MutexGuard<()>) {
        self.waiters.fetch_add(1, Ordering::Relaxed);
        drop(mutex_guard);
        thread::park();
    }

    /// Wake up one blocked thread on this condvar.
    fn notify_one(&self) {
        if self.waiters.load(Ordering::Relaxed) > 0 {
            self.waiters.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Wake up all blocked threads on this condvar.
    fn notify_all(&self) {
        self.waiters.store(0, Ordering::Relaxed);
    }
}

/// A synchronization barrier enabling multiple threads to synchronize the beginning
/// of some computation.
struct Barrier {
    count: usize,
    arrived: AtomicUsize,
    generation: AtomicUsize,
}

impl Barrier {
    /// Create a new barrier that can block a given number of threads.
    fn new(count: usize) -> Barrier {
        Barrier {
            count,
            arrived: AtomicUsize::new(0),
            generation: AtomicUsize::new(0),
        }
    }

    /// Block until all threads have rendezvoused here.
    fn wait(&self) -> BarrierWaitResult {
        let gen = self.generation.load(Ordering::Acquire);
        let n = self.arrived.fetch_add(1, Ordering::Acquire) + 1;
        if n == self.count {
            self.arrived.store(0, Ordering::Release);
            self.generation.fetch_add(1, Ordering::Release);
            BarrierWaitResult { is_leader: true }
        } else {
            while self.generation.load(Ordering::Acquire) == gen {
                thread::yield_now();
            }
            BarrierWaitResult { is_leader: false }
        }
    }
}

/// Result returned by `Barrier::wait`.
struct BarrierWaitResult {
    is_leader: bool,
}

impl BarrierWaitResult {
    /// Returns `true` if this thread is the "leader".
    fn is_leader(&self) -> bool {
        self.is_leader
    }
}

/// An atomic boolean value.
struct AtomicBool {
    v: UnsafeCell<u8>,
}

impl AtomicBool {
    /// Create a new `AtomicBool`.
    fn new(v: bool) -> AtomicBool {
        AtomicBool {
            v: UnsafeCell::new(if v { 1 } else { 0 }),
        }
    }

    /// Load the value.
    fn load(&self, order: Ordering) -> bool {
        unsafe { *self.v.get() != 0 }
    }

    /// Store the value.
    fn store(&self, val: bool, order: Ordering) {
        unsafe { *self.v.get() = if val { 1 } else { 0 } };
    }

    /// Compare and exchange.
    fn compare_exchange(&self, current: bool, new: bool, order: Ordering) -> Result<bool, bool> {
        let cur = self.load(order);
        if cur == current {
            self.store(new, order);
            Result::Ok(new)
        } else {
            Result::Err(cur)
        }
    }
}

/// An atomic usize value.
struct AtomicUsize {
    v: UnsafeCell<usize>,
}

impl AtomicUsize {
    /// Create a new `AtomicUsize`.
    fn new(v: usize) -> AtomicUsize {
        AtomicUsize { v: UnsafeCell::new(v) }
    }

    /// Load the value.
    fn load(&self, order: Ordering) -> usize {
        unsafe { *self.v.get() }
    }

    /// Store the value.
    fn store(&self, val: usize, order: Ordering) {
        unsafe { *self.v.get() = val };
    }

    /// Increment and return the previous value.
    fn fetch_add(&self, val: usize, order: Ordering) -> usize {
        let prev = self.load(order);
        self.store(prev + val, order);
        prev
    }

    /// Decrement and return the previous value.
    fn fetch_sub(&self, val: usize, order: Ordering) -> usize {
        let prev = self.load(order);
        self.store(prev - val, order);
        prev
    }
}

/// An atomic u8 value.
struct AtomicU8 {
    v: UnsafeCell<u8>,
}

impl AtomicU8 {
    /// Create a new `AtomicU8`.
    fn new(v: u8) -> AtomicU8 {
        AtomicU8 { v: UnsafeCell::new(v) }
    }

    /// Load the value.
    fn load(&self, order: Ordering) -> u8 {
        unsafe { *self.v.get() }
    }

    /// Store the value.
    fn store(&self, val: u8, order: Ordering) {
        unsafe { *self.v.get() = val };
    }

    /// Compare and exchange.
    fn compare_exchange(&self, current: u8, new: u8, order: Ordering) -> Result<u8, u8> {
        let cur = self.load(order);
        if cur == current {
            self.store(new, order);
            Result::Ok(new)
        } else {
            Result::Err(cur)
        }
    }
}

/// Atomic ordering.
enum Ordering {
    Relaxed,
    Release,
    Acquire,
    AcqRel,
    SeqCst,
}

/// A wrapper type for mutually exclusive access to the inner value.
struct Exclusive<T> {
    inner: T,
}

impl<T> Exclusive<T> {
    /// Create a new `Exclusive`.
    fn new(t: T) -> Exclusive<T> {
        Exclusive { inner: t }
    }

    /// Get a reference to the inner value.
    fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Unwrap the inner value.
    fn into_inner(self) -> T {
        self.inner
    }
}

/// An `Arc` (atomically reference counted) smart pointer.
struct Arc<T> {
    ptr: *const ArcInner<T>,
}

struct ArcInner<T> {
    strong: AtomicUsize,
    weak: AtomicUsize,
    data: T,
}

impl<T> Arc<T> {
    /// Construct a new `Arc<T>`.
    fn new(data: T) -> Arc<T> {
        let inner = Box::new(ArcInner {
            strong: AtomicUsize::new(1),
            weak: AtomicUsize::new(1),
            data,
        });
        Arc { ptr: Box::into_raw(inner) }
    }

    /// Get the number of strong references.
    fn strong_count(&self) -> usize {
        unsafe { (*self.ptr).strong.load(Ordering::SeqCst) }
    }
}

impl<T> Clone for Arc<T> {
    fn clone(&self) -> Arc<T> {
        unsafe { (*self.ptr).strong.fetch_add(1, Ordering::SeqCst) };
        Arc { ptr: self.ptr }
    }
}

impl<T> Drop for Arc<T> {
    fn drop(&mut self) {
        unsafe {
            if (*self.ptr).strong.fetch_sub(1, Ordering::SeqCst) == 1 {
                // Last strong reference; decrement weak and maybe free
                if (*self.ptr).weak.fetch_sub(1, Ordering::SeqCst) == 1 {
                    let _ = Box::from_raw(self.ptr as *mut ArcInner<T>);
                }
            }
        }
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &(*self.ptr).data }
    }
}
