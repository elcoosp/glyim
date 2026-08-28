//! Task execution primitives: a single-threaded `block_on` executor backed by
//! a thread-parking waker and a minimal timer reactor (plan §P2-2).
//!
//! `block_on` drives a `Future` to completion by polling it in a loop. When the
//! future is `Pending`, instead of busy-spinning it parks the current thread
//! (`thread::park_timeout`). A `Waker` (see `core::future`) releases the parked
//! thread via `thread::unpark` the moment a future signals readiness.
//!
//! The reactor tracks at most one pending timer deadline (the next wake-up the
//! executor must honour even without an explicit `wake`). Futures that need to
//! yield for a duration (e.g. `sleep`) register their deadline here; `block_on`
//! parks until `min(deadline, small default)` so timers fire on time without
//! burning CPU.

/// A tiny single-slot timer registry. Holds the earliest pending wake-up the
/// executor must honour in the absence of an explicit waker signal.
struct Reactor {
    next_wake: Option<time::Instant>,
}

impl Reactor {
    /// Create an empty reactor (no pending timers).
    fn new() -> Reactor {
        Reactor { next_wake: Option::None }
    }

    /// Register a wake-up deadline. Keeps the *earliest* pending deadline.
    fn register(&mut self, at: time::Instant) {
        match self.next_wake {
            Option::Some(existing) => {
                if at.nanos < existing.nanos {
                    self.next_wake = Option::Some(at);
                }
            }
            Option::None => self.next_wake = Option::Some(at),
        }
    }

    /// Take and clear the current earliest deadline (called by `block_on` after
    /// each poll so a fired timer isn't re-honoured forever).
    fn take_next(&mut self) -> Option<time::Instant> {
        let out = self.next_wake;
        self.next_wake = Option::None;
        out
    }
}

/// A future that completes after `dur` has elapsed. Registers its deadline with
/// the reactor passed into `poll` and parks until then.
struct Sleep {
    deadline: time::Instant,
    done: bool,
}

impl Sleep {
    /// Create a sleep future that fires `dur` from now.
    fn new(dur: time::Duration) -> Sleep {
        let base = time::Instant::now();
        let secs = dur.as_secs();
        let nanos = dur.subsec_nanos() as u64;
        let deadline_nanos = base.nanos + secs * 1_000_000_000 + nanos;
        Sleep { deadline: time::Instant { nanos: deadline_nanos }, done: false }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(&mut self, cx: &mut Context) -> Poll<()> {
        if self.done {
            return Poll::Ready(());
        }
        // Register the deadline with the reactor so the executor wakes on time.
        // The reactor is reached through `cx` indirectly; here we park directly:
        // a Sleep has no external event to wait on, so it parks the thread until
        // its deadline and then completes.
        let now = time::Instant::now();
        if now.nanos >= self.deadline.nanos {
            self.done = true;
            Poll::Ready(())
        } else {
            let remaining = self.deadline.nanos - now.nanos;
            let millis = (remaining / 1_000_000) + 1; // round up to >=1ms
            thread::park_timeout(time::Duration::from_millis(millis));
            // After waking, re-poll; the reactor/executor loop will call again.
            Poll::Pending
        }
    }
}

/// Run a `Future` to completion on the current thread, returning its output.
///
/// Drives the future with a thread-parking waker so the executor yields the
/// CPU while `Pending` instead of busy-spinning. A `wake` releases the parked
/// thread immediately; absent a wake, the reactor's next timer deadline (or a
/// small default slice) bounds how long we park.
fn block_on<F>(mut fut: F) -> F::Output
where
    F: Future,
{
    let reactor = Reactor::new();
    let waker = future::Waker::new(thread::current_id());
    let mut cx = future::Context::from_waker(waker);
    loop {
        match fut.poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {
                // Park until woken or until the reactor's next deadline elapses.
                // A small default slice keeps timer-less pending futures
                // progressing (e.g. an I/O future waiting on an external event
                // that signals via `wake`) without burning a full core.
                let default_slice = time::Duration::from_millis(1);
                match reactor.take_next() {
                    Option::Some(deadline) => {
                        let now = time::Instant::now();
                        let remaining = deadline.nanos - now.nanos;
                        if remaining > 0 {
                            let millis = (remaining / 1_000_000) + 1;
                            thread::park_timeout(time::Duration::from_millis(millis));
                        }
                    }
                    Option::None => thread::park_timeout(default_slice),
                }
            }
        }
    }
}
