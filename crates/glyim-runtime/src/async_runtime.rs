//! Minimal single-threaded async executor (Phase 5 MVP).
//!
//! This is the `block_on`-style poll-to-completion executor described in the
//! unstub-5 plan (§5.2). It drives a [`Future`] to completion by repeatedly
//! calling [`Future::poll`] until it yields [`Poll::Ready`].
//!
//! The `Future`/`Poll`/`Context`/`Waker` model mirrors `glyim-lang-core/lib/
//! future.g` so that, once `async fn`/`.await` desugaring lands in the
//! compiler front-end, the generated state machines can be driven by this
//! exact loop. Until then, the no-op `Waker` keeps the executor correct for
//! straight-line futures that become `Ready` on the first poll or after a
//! bounded number of polls.
//!
//! A real multi-threaded waker and an I/O reactor are tracked follow-ups
//! (see `docs/plans/v0.1.0/unstub-5/KNOWN_GAPS.md` Phase 5).

/// The result of polling a [`Future`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll<T> {
    /// The future has completed with a value.
    Ready(T),
    /// The future is not yet complete and must be polled again.
    Pending,
}

use std::sync::{Arc, Condvar, Mutex};

/// A handle to wake a parked task. Backed by a shared `(ready-flag,
/// condvar)` pair supplied by [`block_on`]; `wake` flips the flag and
/// notifies so the executor's wait is released instead of spinning.
#[derive(Debug, Clone)]
pub struct Waker {
    pair: Arc<(Mutex<bool>, Condvar)>,
}

impl Waker {
    /// Construct a standalone waker (used by the I/O reactor and other external
    /// readiness sources that need to signal an executor thread).
    pub fn new() -> Waker {
        Waker {
            pair: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// Wake the associated task: mark it ready and release the executor's wait.
    pub fn wake(&self) {
        let (lock, cvar) = &*self.pair;
        *lock.lock().unwrap() = true;
        cvar.notify_one();
    }
    /// Wake by reference. Identical to [`Waker::wake`] for this executor.
    pub fn wake_by_ref(&self) {
        self.wake();
    }
}

/// Per-poll context handed to [`Future::poll`].
pub struct Context {
    waker: Waker,
}

impl Context {
    /// Construct a `Context` carrying the given [`Waker`].
    pub fn new() -> Context {
        Context {
            waker: Waker {
                pair: Arc::new((Mutex::new(false), Condvar::new())),
            },
        }
    }

    /// Borrow the [`Waker`] for this poll.
    pub fn waker(&self) -> &Waker {
        &self.waker
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// A computation that may complete in the future.
pub trait Future {
    /// The value produced on completion.
    type Output;
    /// Attempt to resolve the future, registering wake-up via `cx` if pending.
    fn poll(&mut self, cx: &mut Context) -> Poll<Self::Output>;
}

/// Drive `future` to completion, polling until it returns [`Poll::Ready`].
///
/// On [`Poll::Pending`] the executor parks on the waker's condvar rather than
/// busy-spinning: a real async task calls `cx.waker().wake()` (typically from a
/// reactor) to resume. A future that returns `Pending` without ever waking the
/// waker would block here — which is the correct "no reactor scheduled me"
/// behaviour once `async fn` / `.await` desugaring lands, since a genuine
/// pending future must always signal re-poll via its waker.
pub fn block_on<F: Future>(mut future: F) -> F::Output {
    let mut cx = Context::new();
    loop {
        match future.poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => {
                let (lock, cvar) = &*cx.waker.pair;
                let mut ready = lock.lock().unwrap();
                while !*ready {
                    ready = cvar.wait(ready).unwrap();
                }
                *ready = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A future that becomes `Ready` after `remaining` polls.
    struct PendingThenReady {
        remaining: usize,
        value: i32,
    }

    impl Future for PendingThenReady {
        type Output = i32;
        fn poll(&mut self, cx: &mut Context) -> Poll<i32> {
            if self.remaining == 0 {
                Poll::Ready(self.value)
            } else {
                self.remaining -= 1;
                // Signal the executor to re-poll us: a pending future must
                // always wake its waker so `block_on` resumes it.
                cx.waker().wake();
                Poll::Pending
            }
        }
    }

    #[test]
    fn block_on_returns_ready_value() {
        // No pending: resolves on the first poll.
        let out = block_on(PendingThenReady {
            remaining: 0,
            value: 42,
        });
        assert_eq!(out, 42);
    }

    #[test]
    fn block_on_polls_until_ready() {
        // Two pending polls, then ready.
        let out = block_on(PendingThenReady {
            remaining: 2,
            value: 7,
        });
        assert_eq!(out, 7);
    }

    #[test]
    fn poll_enum_roundtrip() {
        let r: Poll<i32> = Poll::Ready(1);
        let p: Poll<i32> = Poll::Pending;
        assert_eq!(r, Poll::Ready(1));
        assert_eq!(p, Poll::Pending);
        assert_ne!(r, p);
    }
}
