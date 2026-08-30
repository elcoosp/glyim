//! A minimal single-reactor-thread I/O readiness dispatcher (plan §P2-2).
//!
//! The reactor owns one `mio::Poll` running on a dedicated background thread.
//! Callers register a non-blocking file descriptor together with the [`Waker`]
//! that should be signalled when the fd becomes readable (or writable, per the
//! registered `Interest`). The background thread blocks in `poll.poll(...)` and,
//! on each readiness event, marks the associated slot and calls the waker so the
//! executor's parked thread is released instead of busy-spinning.
//!
//! Two registration paths are supported:
//! * [`Reactor::register`] takes ownership of a `mio::Source` (used by Rust-side
//!   async tests and futures).
//! * [`Reactor::register_fd`] takes a raw fd + an executor thread id and wakes
//!   that thread via `glyim_thread_unpark` on readiness. This is the bridge the
//!   `.g` async socket futures use: on first `poll` they set the fd non-blocking,
//!   call `glyim_reactor_register(fd, interest, current_thread_id)`, and return
//!   `Pending`; the reactor wakes the executor thread when mio reports readiness.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use mio::{event::Source, Interest, Token};
use std::os::unix::io::FromRawFd;

use crate::async_runtime::Waker;

/// A type the reactor can register with `mio::Poll` and drive to readiness.
///
/// `mio::Source` is object-unsafe (its `register` takes `&mut self` with a
/// fixed signature), so we wrap it in a small trait the reactor thread uses to
/// hand the source to `Poll::registry().register` with the right `Interest`.
trait Register: Send {
    fn register_with(
        &mut self,
        poll: &mut mio::Poll,
        token: Token,
        interest: Interest,
    ) -> std::io::Result<()>;
}

impl<S: Source + Send> Register for S {
    fn register_with(
        &mut self,
        poll: &mut mio::Poll,
        token: Token,
        interest: Interest,
    ) -> std::io::Result<()> {
        poll.registry().register(self, token, interest)
    }
}

/// Commands sent from the executor thread to the reactor thread.
enum Command {
    /// Register `source` under `token` for `interest`; on readiness call the
    /// slot's waker (created in `register`).
    Register {
        token: usize,
        source: Box<dyn Register + Send>,
        interest: Interest,
    },
    /// Register a raw fd for `interest`; on readiness wake executor `thread_id`
    /// via `glyim_thread_unpark` (the `.g` async `Future`/`Waker` model).
    RegisterFd {
        token: usize,
        fd: i32,
        interest: Interest,
        thread_id: usize,
    },
    /// Stop the reactor thread.
    Shutdown,
}

struct Slot {
    waker: Waker,
    /// Set by the reactor thread the moment an event is seen, so a re-poll
    /// that races the waker is still correct.
    ready: Arc<AtomicBool>,
}

/// The reactor: a background thread plus the shared registration table.
pub struct Reactor {
    tx: Sender<Command>,
    next_token: Mutex<usize>,
    slots: Arc<Mutex<HashMap<usize, Slot>>>,
    /// Owned registered sources, kept alive for the reactor's lifetime. Dropping
    /// a mio `Source` deregisters its fd from kqueue, so the source MUST outlive
    /// the registration — hence it is stored here (and removed on `deregister`).
    sources: Arc<Mutex<HashMap<usize, Box<dyn Register + Send>>>>,
    join: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Reactor {
    /// Start the reactor background thread.
    pub fn new() -> std::io::Result<Arc<Reactor>> {
        let (tx, rx) = channel::<Command>();
        let slots: Arc<Mutex<HashMap<usize, Slot>>> = Arc::new(Mutex::new(HashMap::new()));
        let slots_bg = slots.clone();
        let sources: Arc<Mutex<HashMap<usize, Box<dyn Register + Send>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let sources_bg = sources.clone();

        let handle = std::thread::Builder::new()
            .name("glyim-io-reactor".to_string())
            .spawn(move || run_reactor(rx, slots_bg, sources_bg))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        Ok(Arc::new(Reactor {
            tx,
            next_token: Mutex::new(0),
            slots,
            sources,
            join: Mutex::new(Some(handle)),
        }))
    }

    /// Register `source` for `interest`; `waker` is signalled on the next event.
    /// Returns a token the caller keeps to `deregister` later.
    pub fn register<S: Source + Send + 'static>(
        self: &Arc<Self>,
        source: S,
        waker: Waker,
        interest: Interest,
    ) -> std::io::Result<usize> {
        let mut nt = self.next_token.lock().unwrap();
        *nt += 1;
        let token = *nt;
        self.slots.lock().unwrap().insert(
            token,
            Slot {
                waker: waker.clone(),
                ready: Arc::new(AtomicBool::new(false)),
            },
        );
        self.tx
            .send(Command::Register {
                token,
                source: Box::new(source),
                interest,
            })
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "reactor closed"))?;
        Ok(token)
    }

    /// Register a raw, already-non-blocking fd for `interest`, waking the
    /// executor thread `thread_id` (via `glyim_thread_unpark`) when it becomes
    /// ready. Returns a token usable with [`Reactor::deregister`].
    ///
    /// Unlike [`Reactor::register`], this borrows the fd only for registration
    /// (via `mio::net::TcpStream::from_raw_fd`) and stores the resulting source
    /// in `sources` so it stays registered for the reactor's lifetime. This is
    /// the bridge the `.g` async socket futures use.
    pub fn register_fd(&self, fd: i32, interest: Interest, thread_id: usize) -> usize {
        let mut nt = self.next_token.lock().unwrap();
        *nt += 1;
        let token = *nt;
        self.slots.lock().unwrap().insert(
            token,
            Slot {
                waker: Waker::new(),
                ready: Arc::new(AtomicBool::new(false)),
            },
        );
        self.tx
            .send(Command::RegisterFd {
                token,
                fd,
                interest,
                thread_id,
            })
            .ok();
        token
    }

    /// Remove a previously registered fd: clears the slot bookkeeping AND drops
    /// the owned source (which deregisters it from kqueue).
    pub fn deregister(&self, token: usize) {
        self.slots.lock().unwrap().remove(&token);
        self.sources.lock().unwrap().remove(&token);
    }

    /// Has `token`'s fd had a readiness event since the last clear?
    pub fn take_ready(&self, token: usize) -> bool {
        self.slots
            .lock()
            .unwrap()
            .get(&token)
            .map(|s| s.ready.swap(false, Ordering::AcqRel))
            .unwrap_or(false)
    }

    /// Lazily-started process-wide reactor singleton. The `.g` async runtime
    /// registers fds here so a single background thread drives all I/O.
    pub fn global() -> Arc<Reactor> {
        static GLOBAL: std::sync::OnceLock<Arc<Reactor>> = std::sync::OnceLock::new();
        GLOBAL
            .get_or_init(|| {
                // If the reactor can't start (no kqueue/epoll), fall back to a
                // minimal inert reactor (no background thread). Async I/O would
                // then rely on the executor's poll-timeout to make progress —
                // the pre-reactor behavior.
                Reactor::new().unwrap_or_else(|_| {
                    let (tx, _rx) = channel::<Command>();
                    Arc::new(Reactor {
                        tx,
                        next_token: Mutex::new(0),
                        slots: Arc::new(Mutex::new(HashMap::new())),
                        sources: Arc::new(Mutex::new(HashMap::new())),
                        join: Mutex::new(None),
                    })
                })
            })
            .clone()
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(h) = self.join.lock().unwrap().take() {
            let _ = h.join();
        }
    }
}

fn run_reactor(
    rx: Receiver<Command>,
    slots: Arc<Mutex<HashMap<usize, Slot>>>,
    sources: Arc<Mutex<HashMap<usize, Box<dyn Register + Send>>>>,
) {
    let mut poll = match mio::Poll::new() {
        Ok(p) => p,
        Err(_) => return,
    };
    // Map mio tokens -> our slot tokens.
    let mut mio_to_slot: HashMap<Token, usize> = HashMap::new();
    // For fds registered via `RegisterFd`, the executor thread to unpark on
    // readiness (the `.g` waker model resumes by thread id, not by Rust Waker).
    let fd_thread: Arc<Mutex<HashMap<usize, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut events = mio::Events::with_capacity(1024);

    loop {
        // Drain any pending registration commands.
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Command::Shutdown => return,
                Command::Register {
                    token,
                    source,
                    interest,
                    ..
                } => {
                    let mt = Token(token);
                    // Keep the source alive for the reactor's lifetime (dropping a
                    // mio `Source` deregisters its fd from kqueue).
                    sources.lock().unwrap().insert(token, source);
                    if let Some(src) = sources.lock().unwrap().get_mut(&token) {
                        let _ = src.register_with(&mut poll, mt, interest);
                    }
                    mio_to_slot.insert(mt, token);
                }
                Command::RegisterFd {
                    token,
                    fd,
                    interest,
                    thread_id,
                } => {
                    let mt = Token(token);
                    // Wrap the raw fd in a mio TcpStream and keep it alive in
                    // `sources` so the fd stays registered. The caller must have
                    // already put the fd in non-blocking mode.
                    let src = unsafe { mio::net::TcpStream::from_raw_fd(fd) };
                    sources.lock().unwrap().insert(token, Box::new(src));
                    if let Some(s) = sources.lock().unwrap().get_mut(&token) {
                        let _ = s.register_with(&mut poll, mt, interest);
                    }
                    mio_to_slot.insert(mt, token);
                    fd_thread.lock().unwrap().insert(token, thread_id);
                }
            }
        }

        // Block briefly for readiness. A non-zero timeout lets new
        // registrations be picked up promptly without a dedicated wake channel.
        if poll
            .poll(&mut events, Some(std::time::Duration::from_millis(50)))
            .is_err()
        {
            continue;
        }
        for event in events.iter() {
            if let Some(&slot_token) = mio_to_slot.get(&event.token()) {
                if let Some(slot) = slots.lock().unwrap().get(&slot_token) {
                    slot.ready.store(true, Ordering::Release);
                    slot.waker.wake();
                }
                // For fd-based registrations, wake the executor thread directly
                // (the `.g` async model resumes by thread id via unpark).
                if let Some(tid) = fd_thread.lock().unwrap().get(&slot_token).copied() {
                    unsafe { glyim_thread_unpark(tid) };
                }
            }
        }

        // Bail if shutdown arrived while we were polling.
        if matches!(rx.try_recv(), Ok(Command::Shutdown)) {
            return;
        }
    }
}

unsafe extern "C" {
    fn glyim_thread_unpark(id: usize);
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point used by the `.g` async runtime. Registers `fd` (which must
/// already be in non-blocking mode) for `interest` (1=READ, 2=WRITE, 3=BOTH)
/// and wakes the executor thread `thread_id` (via `glyim_thread_unpark`) when
/// the fd becomes ready. Returns a token to pass to `glyim_reactor_deregister`.
pub unsafe extern "C" fn glyim_reactor_register(fd: i32, interest: u32, thread_id: usize) -> usize {
    let interest = match interest {
        1 => Interest::READABLE,
        2 => Interest::WRITABLE,
        3 => Interest::READABLE | Interest::WRITABLE,
        _ => Interest::READABLE,
    };
    Reactor::global().register_fd(fd, interest, thread_id)
}

#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point. Removes a previously-registered fd from the reactor.
pub unsafe extern "C" fn glyim_reactor_deregister(token: usize) {
    Reactor::global().deregister(token);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::io::IntoRawFd;

    #[test]
    fn reactor_wakes_on_socket_readable() {
        // A plain std listener + peer, but the watched socket is wrapped by mio
        // via `from_std` so the reactor owns a real `mio::Source` fd.
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = std_listener.local_addr().unwrap();

        // Peer writes data to the connection.
        let mut peer = std::net::TcpStream::connect(addr).unwrap();
        let (accepted, _peer_addr) = std_listener.accept().unwrap();
        let client = unsafe { mio::net::TcpStream::from_std(accepted) };

        let reactor = Reactor::new().expect("reactor starts");
        let token = reactor
            .register(client, Waker::new(), Interest::READABLE)
            .expect("register fd");

        // Nothing written yet -> not ready.
        assert!(!reactor.take_ready(token));

        peer.write_all(b"hello world").unwrap();
        peer.flush().unwrap();

        // Poll via the reactor thread until readiness (up to ~1s).
        let mut ready = false;
        for _ in 0..20 {
            if reactor.take_ready(token) {
                ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(ready, "reactor must signal readability");

        drop(reactor);
        let _ = _peer_addr;
    }

    #[test]
    fn reactor_fd_registration_detects_readiness() {
        // Verify the fd-based registration path that the `.g` async futures use:
        // register a raw fd (interest=READABLE) and confirm the reactor reports
        // readiness when a peer writes to it. On readiness the reactor also calls
        // `glyim_thread_unpark(thread_id)` to wake the `.g` executor — the same
        // wake mechanism `task::block_on` relies on.
        use std::os::unix::io::IntoRawFd;

        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = std_listener.local_addr().unwrap();
        let mut peer = std::net::TcpStream::connect(addr).unwrap();
        let (accepted, _pa) = std_listener.accept().unwrap();
        let fd = accepted.into_raw_fd();

        let reactor = Reactor::new().expect("reactor starts");
        // `thread_id` is the executor thread the reactor will unpark on readiness;
        // any non-zero ThreadStore id is accepted here (the wake itself is covered
        // by the runtime's thread tests). We use the current thread's id.
        unsafe extern "C" {
            fn glyim_thread_current_id() -> usize;
        }
        let thread_id = unsafe { glyim_thread_current_id() };
        let token = reactor.register_fd(fd, Interest::READABLE, thread_id);

        // Give the reactor thread a moment to process the registration command
        // before the peer writes, so the fd is watched when data arrives.
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Write from the peer so the fd becomes readable.
        peer.write_all(b"ping").unwrap();
        peer.flush().unwrap();

        // The reactor must detect readiness on the registered fd.
        let mut ok = false;
        for _ in 0..40 {
            if reactor.take_ready(token) {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(ok, "reactor must detect readiness on the registered fd");
        reactor.deregister(token);
        drop(reactor);
        let _ = _pa;
    }

    #[test]
    fn reactor_unpark_wakes_executor_thread() {
        // Verify the wake primitive the reactor uses on fd readiness:
        // `glyim_thread_unpark(id)` must release a thread parked via the glyim
        // thread bridge. This is what makes `.g` async I/O resume without spinning.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc as StdArc;

        let woken = StdArc::new(AtomicBool::new(false));
        let woken_bg = woken.clone();
        unsafe extern "C" {
            fn glyim_thread_spawn(f: extern "C" fn(*mut u8), arg: *mut u8) -> usize;
            fn glyim_thread_unpark(id: usize);
        }
        extern "C" fn exec_entry(arg: *mut u8) {
            let w = unsafe { &*(arg as *const AtomicBool) };
            // Park; the test will unpark this thread via glyim_thread_unpark.
            std::thread::park();
            w.store(true, Ordering::SeqCst);
        }
        let exec_id =
            unsafe { glyim_thread_spawn(exec_entry, &*woken_bg as *const AtomicBool as *mut u8) };
        assert!(exec_id != 0, "executor must spawn with a ThreadStore id");

        // Unpark via the same id the reactor would use.
        unsafe { glyim_thread_unpark(exec_id) };

        // The executor should have been released and set the flag.
        let mut ok = false;
        for _ in 0..40 {
            if woken.load(Ordering::SeqCst) {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(ok, "glyim_thread_unpark must release the spawned executor thread");
    }
}
