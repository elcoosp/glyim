//! A minimal single-reactor-thread I/O readiness dispatcher (plan §P2-2).
//!
//! The reactor owns one `mio::Poll` running on a dedicated background thread.
//! Callers register a non-blocking file descriptor together with the [`Waker`]
//! that should be signalled when the fd becomes readable (or writable, per the
//! registered `Interest`). The background thread blocks in `poll.poll(...)`
//! and, on each readiness event, marks the associated slot and calls the waker
//! so the executor's parked thread is released instead of busy-spinning.
//!
//! This is the dependency-light (mio only) half of the async executor that lets
//! I/O-bound futures make progress: a `TcpStream::read_async` future would
//! register its fd on first `Pending` and the generated `poll` returns
//! `Pending` without spinning; the reactor thread wakes it when `mio` reports
//! readability.
//!
//! The `.g` async socket future types that *use* this reactor are a tracked
//! follow-up (they require compiler/type-system work for `async fn` in the
//! stdlib surface), so this module exposes a small, directly-testable Rust API
//! (`register` / `take_ready`) plus a real localhost-socket integration test.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use mio::{event::Source, Interest, Token};

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
    fn deregister_from(&mut self, poll: &mut mio::Poll) -> std::io::Result<()>;
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
    fn deregister_from(&mut self, poll: &mut mio::Poll) -> std::io::Result<()> {
        poll.registry().deregister(self)
    }
}

/// Commands sent from the executor thread to the reactor thread.
enum Command {
    /// Register `source` under `token` for `interest`; on readiness call
    /// `waker.wake()`.
    Register {
        token: usize,
        source: Box<dyn Register + Send>,
        waker: Waker,
        interest: Interest,
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
                waker,
                interest,
            })
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "reactor closed"))?;
        Ok(token)
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
                    // Best-effort: a registration that fails (e.g. fd already
                    // registered elsewhere) is skipped for this minimal impl.
                    if let Some(src) = sources.lock().unwrap().get_mut(&token) {
                        let _ = src.register_with(&mut poll, mt, interest);
                    }
                    mio_to_slot.insert(mt, token);
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
            }
        }

        // Bail if shutdown arrived while we were polling.
        if matches!(rx.try_recv(), Ok(Command::Shutdown)) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
}
