//! Asynchronous runtime foundation for the Glyim core library.
//!
//! This module defines the minimal `Future` model required to make
//! `async`/`.await` usable for straight-line, non-concurrent code in its
//! first iteration. The `Waker`/`Context` types are intentionally minimal:
//! a single-threaded, no-op waker is enough to drive futures to completion
//! via a `block_on`-style poll loop. A real multi-threaded waker / I/O
//! reactor is a tracked follow-up (see `KNOWN_GAPS.md` Phase 5).

/// The result of polling a [`Future`].
///
/// `Ready` carries the produced value; `Pending` means the future is not yet
/// complete and must be polled again after its waker is signalled.
pub enum Poll<T> {
    /// The future has completed with a value.
    Ready(T),
    /// The future is not yet complete.
    Pending,
}

/// A handle to a task's waker.
///
/// A waker lets a future signal that it is ready to make progress. The first
/// iteration used a no-op waker and a busy-spinning `block_on`; this version
/// records the id of the executor thread and `wake`/`wake_by_ref` call the
/// runtime's `glyim_thread_unpark` directly, so a parked executor thread is
/// released as soon as a future becomes ready (plan §P2-2). The waker is
/// self-contained (no dependency on the `thread` stdlib module) because it
/// talks to the runtime FFI directly.
pub struct Waker {
    thread_id: usize,
}

impl Waker {
    /// Construct a waker bound to the executor thread `id`.
    fn new(id: usize) -> Waker {
        Waker { thread_id: id }
    }

    /// Wake the associated task: release the parked executor thread.
    fn wake(&self) {
        extern "C" {
            fn glyim_thread_unpark(id: usize);
        }
        unsafe { glyim_thread_unpark(self.thread_id) };
    }

    /// Wake the associated task by reference (same as [`wake`](Waker::wake)).
    fn wake_by_ref(&self) {
        self.wake();
    }
}

/// Per-poll contextual data handed to [`Future::poll`].
///
/// Carries the [`Waker`] the future should use to signal readiness.
pub struct Context {
    waker: Waker,
}

impl Context {
    /// Construct a `Context` owning the given [`Waker`].
    fn from_waker(waker: Waker) -> Context {
        Context { waker }
    }

    /// Borrow the [`Waker`] associated with this poll.
    fn waker(&self) -> &Waker {
        &self.waker
    }
}

/// A computation that may complete in the future.
///
/// A future is driven by repeatedly calling [`poll`](Future::poll) until it
/// returns [`Poll::Ready`]. Each call resumes the future where it last
/// suspended (at an `.await` point once `async fn` desugaring lands).
pub trait Future {
    /// The type of value produced on completion.
    type Output;

    /// Attempt to resolve the future to a final value, registering the
    /// current task for wake-up if the value is not yet available.
    fn poll(&mut self, cx: &mut Context) -> Poll<Self::Output>;
}
