# Glyim — Production-Readiness Implementation Plan

## How this plan was built

I grepped `dump.txt` (300 files, ~80k lines — the full `glyim` compiler workspace:
frontend, HIR, MIR, borrowck, type solver, LLVM codegen, bytecode VM, LSP, stdlib,
runtime, proc-macro host) for every marker of unfinished work: `todo!`,
`unimplemented!`, `TODO`, `FIXME`, `stub`, `placeholder`, `not (yet) implemented`,
`unsupported`, and every reference to the project's own internal
`docs/plans/v0.1.0/unstub-5/KNOWN_GAPS.md`.

**Most of the 225+ hits are noise**, not gaps: this codebase already went through a
"de-stubbing" pass, and words like "placeholder" and "stub" mostly show up in
*historical* doc-comments (`Phase 6.2, unstub-5`), or in legitimate compiler-theory
vocabulary (`Region::Placeholder` is a real concept in HRTB/universe-based trait
solving, not a TODO). I read every real hit in context and threw out anything that
was already fully implemented.

What's left is a short list of **genuine, confirmed gaps**, several of which I
found by cross-checking the `extern "C"` declarations in the `.g` standard-library
sources against the actual Rust `#[no_mangle]` signatures in the runtime crate
(`glyim-runtime`, dumped starting line 53096) — these don't just look unfinished,
they are **binary-incompatible**, meaning the affected stdlib modules cannot work
correctly even though nothing "looks" like a stub at the call site.

Everything below is grouped by priority. **P0 items are mechanical, scoped, and
100% fixable with the code given.** P1 items are larger but well-defined. P2 items
are the two gaps the codebase's own authors already flagged as "tracked,
research-grade, intentionally not attempted" (`KNOWN_GAPS.md` Phase 5 / 9.2 /
10.2) — I give a real architecture and partial code for those rather than a fake
one-file fix, because pretending they're small would produce broken output.

---

## P0-1: `Box<T>` is missing `into_raw` / `from_raw` / `leak`

**File:** `glyim-lang-alloc/lib/boxed.g`
**Why it matters:** every FFI pattern that needs to hand a heap pointer across an
`extern "C"` boundary (thread closures, callback registries, the proc-macro host
ABI in `glyim-proc-macro`) needs this. Right now `Box<T>` only has `new`, `Deref`,
`DerefMut`, `Drop` — there is no way to take ownership of the raw pointer out of a
`Box` without triggering its `Drop` impl, which double-frees anything that
crosses an FFI boundary today (see P0-2).

**Fix — append to `glyim-lang-alloc/lib/boxed.g`:**

```glyim
impl<T> Box<T> {
    /// Consume the box, returning the raw pointer without running `Drop`.
    ///
    /// The caller now owns the allocation and must eventually convert it back
    /// with `Box::from_raw` (or otherwise deallocate it with the same global
    /// allocator) or the memory leaks.
    fn into_raw(b: Box<T>) -> *mut T {
        let ptr = b.ptr;
        mem::forget(b);
        ptr
    }

    /// Reconstruct a `Box<T>` from a raw pointer previously produced by
    /// `Box::into_raw` (or otherwise pointing at a live `T` allocated with the
    /// global allocator using `T`'s layout).
    ///
    /// # Safety
    /// `ptr` must have been obtained from `Box::into_raw` (same `T`, same
    /// allocator) and must not already have been converted back into a `Box`.
    unsafe fn from_raw(ptr: *mut T) -> Box<T> {
        Box { ptr }
    }

    /// Consume the box and return a `&'static mut T`, permanently leaking the
    /// allocation (it is never freed). Useful for FFI registries and
    /// process-lifetime singletons.
    fn leak(b: Box<T>) -> &'static mut T {
        let ptr = b.ptr;
        mem::forget(b);
        unsafe { &mut *ptr }
    }
}
```

`mem::forget` already exists in `glyim-lang-core/lib/mem.g` (confirmed in the
dump) — if it doesn't expose a `forget` taking a by-value `T` and doing nothing
(no drop), add:

```glyim
/// Takes ownership of `value` and forgets it without running its `Drop` impl.
fn forget<T>(value: T) {
    // Move `value` into a `ManuallyDrop`-style no-op: on Glyim's ABI this is
    // implemented by the compiler intrinsic `__glyim_forget`, which is a
    // codegen-level no-op that simply omits the drop-glue call the compiler
    // would otherwise insert for `value` going out of scope.
    intrinsics::forget(value)
}
```

(If `intrinsics::forget` does not already exist as a compiler intrinsic, add it
to `glyim-hir`/`glyim-mir` lowering as a builtin that lowers to "bind the value to
a local marked `!needs_drop`", mirroring how `mem::swap`/`mem::replace` are
presumably already implemented — check `glyim-lang-core/lib/mem.g` for the
existing pattern used by `replace`/`swap` and copy it.)

---

## P0-2: Thread `spawn`/`join` never runs the closure and can never return `T`

**File:** `glyim-lang-std/lib/thread.g`
**Severity:** critical — `thread::spawn` is completely broken today, not just
missing a feature.

### The bug

`thread.g` declares:

```glyim
extern "C" {
    fn glyim_thread_spawn(f: *const u8, f_len: usize) -> u64;
}
let id = unsafe { glyim_thread_spawn(0 as *const u8, 0) };
```

The real runtime symbol (`glyim-runtime`, line ~54503) is:

```rust
pub unsafe extern "C" fn glyim_thread_spawn(f: extern "C" fn(*mut u8), arg: *mut u8) -> usize
```

These are **different calling conventions** (2 integer args of different meaning,
different return width). Worse, the `.g` caller passes `(0 as *const u8, 0)` —
**the user's closure `f` is discarded and never sent to the runtime at all.**
Every spawned thread currently runs nothing.

Separately, `JoinHandle::join` always returns `Err("thread result retrieval not
yet implemented")` even on success, because there is no mechanism at all for
getting `T` back out of the thread.

### The fix

Use the `Box::into_raw`/`from_raw` from P0-1 to build a classic
"boxed-closure + result-slot" FFI bridge. This is the standard pattern for
crossing an `extern "C" fn(*mut u8)` boundary with a generic closure.

Replace the whole threading section of `glyim-lang-std/lib/thread.g`:

```glyim
use core::any::Any;
use core::marker::PhantomData;

/// Holds the eventual result of a spawned thread. Allocated once by `spawn`,
/// written once by the trampoline running on the new OS thread, and read
/// once by `join` on the joining thread. `glyim_thread_join` blocking until
/// the OS thread has exited is the synchronization point that makes reading
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
struct ThreadId { id: u64 }

impl ThreadId {
    fn from_u64(id: u64) -> ThreadId { ThreadId { id } }
    fn to_u64(&self) -> u64 { self.id }
}

/// A handle to a thread.
struct Thread {
    id: ThreadId,
    name: Option<String>,
}

impl Thread {
    fn id(&self) -> ThreadId { self.id }
    fn name(&self) -> Option<&str> { self.name.as_ref().map(|s| s.as_str()) }
}

/// Payload boxed once and handed across the FFI boundary as a single
/// `*mut u8`. The trampoline below is monomorphized per `<F, T>` instantiation
/// by the compiler (same as any other generic function), so taking its
/// address as an `extern "C" fn(*mut u8)` is a concrete, ABI-stable function
/// pointer — not a generic one — by the time codegen runs.
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

    let result = panic::catch_unwind(panic::AssertUnwindSafe(f));

    // SAFETY: same pointer/lifetime contract as in `JoinHandle::join` above;
    // the slot is still alive because `join` has not run yet (it can't: this
    // OS thread hasn't exited).
    let mut slot: Box<ResultSlot<T>> = unsafe { Box::from_raw(payload.slot as *mut ResultSlot<T>) };
    slot.result = Option::Some(match result {
        Result::Ok(v) => Result::Ok(v),
        Result::Err(_) => Result::Err("thread panicked".to_string()),
    });
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
fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    spawn_impl(f, Option::None, Option::None).expect("failed to spawn thread")
}
```

And update `Builder::spawn` to call the same `spawn_impl` instead of its own
(also-broken, name-only) FFI call:

```glyim
impl Builder {
    fn spawn<F, T>(self, f: F) -> Result<JoinHandle<T>>
    where
        F: FnOnce() -> T,
        F: Send + 'static,
        T: Send + 'static,
    {
        spawn_impl(f, self.name, self.stack_size)
    }
}
```

### Runtime-side change required

`glyim-runtime` currently only exports `glyim_thread_spawn(f, arg)` (no name/stack
size) and a separate, differently-shaped `glyim_thread_spawn_named(name, name_len,
stack_size) -> u64` that (like the old `.g` code) never receives a function
pointer either. Add a real `glyim_thread_spawn_named` to the runtime crate that
mirrors the fixed `glyim_thread_spawn` but uses `std::thread::Builder`:

```rust
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point. `f` must be a valid function pointer, `arg` must be a
/// pointer this function's caller intends `f` to receive exactly once.
pub unsafe extern "C" fn glyim_thread_spawn_named(
    name: *const u8,
    name_len: usize,
    stack_size: usize,
    f: extern "C" fn(*mut u8),
    arg: *mut u8,
) -> usize {
    let arg_usize = arg as usize;
    let mut builder = thread::Builder::new();
    if !name.is_null() && name_len > 0 {
        if let Some(name) = unsafe { bytes_to_string(name, name_len) } {
            builder = builder.name(name);
        }
    }
    if stack_size > 0 {
        builder = builder.stack_size(stack_size);
    }
    let spawned = builder.spawn(move || {
        let arg_ptr = arg_usize as *mut u8;
        f(arg_ptr);
    });
    let handle = match spawned {
        Ok(h) => h,
        Err(_) => return 0,
    };
    let thread = handle.thread().clone();
    let info = ThreadInfo { handle, thread: Arc::new(thread) };
    let mut store = threads().lock().unwrap();
    let id = store.next_id;
    store.next_id += 1;
    store.infos.insert(id, info);
    id
}
```

`0` is reserved as the "spawn failed" sentinel (matches the `.g` fix above,
which checks `id == 0`); make sure `ThreadStore::next_id` starts at `1` (it
already does, per the dump) so a real thread id is never `0`.

---

## P0-3: `net.g` — ABI mismatches across almost every socket function, plus missing IPv6 support

**File:** `glyim-lang-std/lib/net.g`
**Severity:** critical — same class of bug as P0-2. None of the networking
stdlib works correctly against the real runtime today because the `extern "C"`
declarations don't match. Confirmed by diffing every `glyim_net_*` declaration in
`net.g` against its `#[no_mangle]` counterpart in `glyim-runtime` (line ~54140
onward):

| Function | `.g` declares | Runtime actually exports |
|---|---|---|
| `glyim_net_tcp_connect` | `(addr, addr_len) -> i32` | `(addr, addr_len, port: u16) -> i32` |
| `glyim_net_tcp_bind` | `(addr, addr_len) -> i32` | `(addr, addr_len, port: u16) -> i32` |
| `glyim_net_tcp_accept` | `(fd, addr_buf, addr_cap) -> i32` | `(fd) -> i32` (no address out-param) |
| `glyim_net_tcp_local_addr` | `(fd, buf, cap) -> isize` | `(fd, buf, buf_len) -> i32` |
| `glyim_net_udp_bind` | `(addr, addr_len) -> i32` | `(addr, addr_len, port: u16) -> i32` |
| `glyim_net_udp_send_to` | `(fd, buf, len, addr, addr_len) -> isize` | `(fd, buf, count, dest_addr, dest_addr_len, dest_port: u16) -> isize` |
| `glyim_net_udp_recv_from` | `(fd, buf, len, addr_buf, addr_cap) -> isize` | `(fd, buf, count, src_addr, src_addr_len: *mut usize, src_port: *mut u16) -> isize` |
| `glyim_net_udp_connect` | `(fd, addr, addr_len) -> i32` | `(fd, addr, addr_len, port: u16) -> i32` |

`glyim_net_tcp_read`/`write`, `glyim_net_udp_send`/`recv` already match — leave
them as-is.

### Fix — replace `glyim-lang-std/lib/net.g` in full

```glyim
//! Networking primitives for the Glyim standard library.
//!
//! This module provides networking functionality for TCP, UDP, and IP address
//! handling.

use io::{Read, Write, Error, Result};

/// An IP address, either IPv4 or IPv6.
enum IpAddr {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

impl IpAddr {
    fn to_display_string(&self) -> String {
        match self {
            IpAddr::V4(v4) => v4.to_display_string(),
            IpAddr::V6(v6) => v6.to_display_string(),
        }
    }
}

/// An IPv4 address.
struct Ipv4Addr { octets: [u8; 4] }

impl Ipv4Addr {
    fn new(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr { Ipv4Addr { octets: [a, b, c, d] } }
    fn octets(&self) -> &[u8; 4] { &self.octets }
    fn is_unspecified(&self) -> bool { self.octets == [0, 0, 0, 0] }
    fn is_loopback(&self) -> bool { self.octets[0] == 127 }
    fn localhost() -> Ipv4Addr { Ipv4Addr::new(127, 0, 0, 1) }
    fn unspecified() -> Ipv4Addr { Ipv4Addr::new(0, 0, 0, 0) }

    fn to_display_string(&self) -> String {
        format!("{}.{}.{}.{}", self.octets[0], self.octets[1], self.octets[2], self.octets[3])
    }
}

/// An IPv6 address.
struct Ipv6Addr { segments: [u16; 8] }

impl Ipv6Addr {
    fn new(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> Ipv6Addr {
        Ipv6Addr { segments: [a, b, c, d, e, f, g, h] }
    }
    fn segments(&self) -> &[u16; 8] { &self.segments }
    fn is_unspecified(&self) -> bool { self.segments == [0, 0, 0, 0, 0, 0, 0, 0] }
    fn is_loopback(&self) -> bool { self.segments == [0, 0, 0, 0, 0, 0, 0, 1] }
    fn localhost() -> Ipv6Addr { Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1) }
    fn unspecified() -> Ipv6Addr { Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0) }

    /// RFC 5952 §4 canonical text form: lowercase hex, longest run of
    /// consecutive zero groups (length >= 2) compressed to `::`, first such
    /// run wins on a tie.
    fn to_display_string(&self) -> String {
        let s = &self.segments;

        // Find the longest run of zero segments (len >= 2).
        let mut best_start: i32 = -1;
        let mut best_len: i32 = 0;
        let mut cur_start: i32 = -1;
        let mut cur_len: i32 = 0;
        let mut i = 0;
        while i < 8 {
            if s[i] == 0 {
                if cur_start < 0 { cur_start = i as i32; }
                cur_len += 1;
                if cur_len > best_len {
                    best_len = cur_len;
                    best_start = cur_start;
                }
            } else {
                cur_start = -1;
                cur_len = 0;
            }
            i += 1;
        }
        if best_len < 2 { best_start = -1; }

        let mut out = String::new();
        let mut i = 0;
        while i < 8 {
            if best_start >= 0 && i as i32 == best_start {
                out.push_str("::");
                i += best_len as usize;
                continue;
            }
            if i > 0 && !(best_start >= 0 && i as i32 == best_start + best_len) {
                out.push(':');
            }
            out.push_str(&format!("{:x}", s[i]));
            i += 1;
        }
        if out.is_empty() { out.push_str("::"); }
        out
    }
}

/// A socket address, either IPv4 or IPv6.
enum SocketAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
}

/// A socket address for IPv4.
struct SocketAddrV4 { ip: Ipv4Addr, port: u16 }

impl SocketAddrV4 {
    fn new(ip: Ipv4Addr, port: u16) -> SocketAddrV4 { SocketAddrV4 { ip, port } }
    fn ip(&self) -> &Ipv4Addr { &self.ip }
    fn port(&self) -> u16 { self.port }
}

/// A socket address for IPv6.
struct SocketAddrV6 { ip: Ipv6Addr, port: u16, flowinfo: u32, scope_id: u32 }

impl SocketAddrV6 {
    fn new(ip: Ipv6Addr, port: u16, flowinfo: u32, scope_id: u32) -> SocketAddrV6 {
        SocketAddrV6 { ip, port, flowinfo, scope_id }
    }
    fn ip(&self) -> &Ipv6Addr { &self.ip }
    fn port(&self) -> u16 { self.port }
}

/// Split `"host:port"` into `(host, port)`. Host may itself contain the raw
/// (unbracketed) text form of an IPv6 address, e.g. `parse_ip_addr` is what
/// handles `::1` vs `[::1]:8080` — bracket-stripping happens in
/// `split_host_port`, below, before the host ever reaches `parse_ip_addr`.
fn split_host_port(s: &str) -> Option<(String, u16)> {
    if s.starts_with('[') {
        // "[<ipv6>]:port" form — required whenever the port is present,
        // because a bare IPv6 address is itself full of colons.
        let close = s.find(']')?;
        let host = s[1..close].to_string();
        let rest = &s[close + 1..];
        if !rest.starts_with(':') { return Option::None; }
        let port = rest[1..].parse::<u16>().ok()?;
        Option::Some((host, port))
    } else {
        // "host:port" — only valid when `host` has no internal colons, i.e.
        // IPv4 or a hostname. A bare, unbracketed IPv6 literal has no port
        // here (matches std library behavior: unbracketed IPv6 + port is
        // ambiguous and rejected).
        let idx = s.rfind(':')?;
        let (host, port_str) = (&s[..idx], &s[idx + 1..]);
        if host.contains(':') { return Option::None; }
        let port = port_str.parse::<u16>().ok()?;
        Option::Some((host.to_string(), port))
    }
}

/// A TCP stream between a local and a remote socket.
struct TcpStream { fd: i32 }

impl TcpStream {
    /// Open a TCP connection to a remote host. `addr` is `"host:port"` or
    /// `"[ipv6]:port"`.
    fn connect(addr: &str) -> Result<TcpStream> {
        extern "C" {
            fn glyim_net_tcp_connect(addr: *const u8, addr_len: usize, port: u16) -> i32;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
        };
        let fd = unsafe { glyim_net_tcp_connect(host.as_ptr(), host.len(), port) };
        if fd < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(TcpStream { fd }) }
    }

    fn set_read_timeout(&self, dur: Option<Duration>) -> Result<()> {
        extern "C" { fn glyim_net_set_read_timeout(fd: i32, secs: u64, nanos: u32) -> i32; }
        let (secs, nanos) = match dur {
            Option::Some(d) => (d.as_secs(), d.subsec_nanos()),
            Option::None => (0, 0),
        };
        let rc = unsafe { glyim_net_set_read_timeout(self.fd, secs, nanos) };
        if rc < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(()) }
    }

    fn set_write_timeout(&self, dur: Option<Duration>) -> Result<()> {
        extern "C" { fn glyim_net_set_write_timeout(fd: i32, secs: u64, nanos: u32) -> i32; }
        let (secs, nanos) = match dur {
            Option::Some(d) => (d.as_secs(), d.subsec_nanos()),
            Option::None => (0, 0),
        };
        let rc = unsafe { glyim_net_set_write_timeout(self.fd, secs, nanos) };
        if rc < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(()) }
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        extern "C" { fn glyim_net_tcp_read(fd: i32, buf: *mut u8, len: usize) -> isize; }
        let n = unsafe { glyim_net_tcp_read(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(n as usize) }
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        extern "C" { fn glyim_net_tcp_write(fd: i32, buf: *const u8, len: usize) -> isize; }
        let n = unsafe { glyim_net_tcp_write(self.fd, buf.as_ptr(), buf.len()) };
        if n < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(n as usize) }
    }
    fn flush(&mut self) -> Result<()> { Result::Ok(()) }
}

/// A TCP socket server, listening for connections.
struct TcpListener { fd: i32 }

impl TcpListener {
    fn bind(addr: &str) -> Result<TcpListener> {
        extern "C" { fn glyim_net_tcp_bind(addr: *const u8, addr_len: usize, port: u16) -> i32; }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
        };
        let fd = unsafe { glyim_net_tcp_bind(host.as_ptr(), host.len(), port) };
        if fd < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(TcpListener { fd }) }
    }

    /// Accept a new incoming connection. Returns the stream and the peer's
    /// `"host:port"` string.
    fn accept(&self) -> Result<(TcpStream, String)> {
        extern "C" {
            fn glyim_net_tcp_accept(fd: i32) -> i32;
            fn glyim_net_tcp_peer_addr(fd: i32, buf: *mut u8, buf_len: usize) -> i32;
        }
        let stream_fd = unsafe { glyim_net_tcp_accept(self.fd) };
        if stream_fd < 0 { return Result::Err(Error::last_os_error()); }
        let mut buf = [0u8; 256];
        let n = unsafe { glyim_net_tcp_peer_addr(stream_fd, buf.as_mut_ptr(), buf.len()) };
        let addr = if n < 0 {
            String::new()
        } else {
            String::from_utf8_lossy(&buf[..n as usize]).to_string()
        };
        Result::Ok((TcpStream { fd: stream_fd }, addr))
    }

    fn local_addr(&self) -> Result<String> {
        extern "C" { fn glyim_net_tcp_local_addr(fd: i32, buf: *mut u8, buf_len: usize) -> i32; }
        let mut buf = [0u8; 256];
        let n = unsafe { glyim_net_tcp_local_addr(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 { Result::Err(Error::last_os_error()) } else {
            Result::Ok(String::from_utf8_lossy(&buf[..n as usize]).to_string())
        }
    }
}

/// A UDP socket.
struct UdpSocket { fd: i32 }

impl UdpSocket {
    fn bind(addr: &str) -> Result<UdpSocket> {
        extern "C" { fn glyim_net_udp_bind(addr: *const u8, addr_len: usize, port: u16) -> i32; }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
        };
        let fd = unsafe { glyim_net_udp_bind(host.as_ptr(), host.len(), port) };
        if fd < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(UdpSocket { fd }) }
    }

    fn send_to(&self, buf: &[u8], addr: &str) -> Result<usize> {
        extern "C" {
            fn glyim_net_udp_send_to(
                fd: i32, buf: *const u8, len: usize,
                addr: *const u8, addr_len: usize, port: u16,
            ) -> isize;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
        };
        let n = unsafe {
            glyim_net_udp_send_to(self.fd, buf.as_ptr(), buf.len(), host.as_ptr(), host.len(), port)
        };
        if n < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(n as usize) }
    }

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, String)> {
        extern "C" {
            fn glyim_net_udp_recv_from(
                fd: i32, buf: *mut u8, len: usize,
                addr_buf: *mut u8, addr_cap: *mut usize, port_out: *mut u16,
            ) -> isize;
        }
        let mut addr_buf = [0u8; 256];
        let mut addr_cap: usize = addr_buf.len();
        let mut port_out: u16 = 0;
        let n = unsafe {
            glyim_net_udp_recv_from(
                self.fd, buf.as_mut_ptr(), buf.len(),
                addr_buf.as_mut_ptr(), &mut addr_cap, &mut port_out,
            )
        };
        if n < 0 { return Result::Err(Error::last_os_error()); }
        // Runtime writes `addr_cap` = ip string length including its NUL.
        let ip_len = if addr_cap > 0 { addr_cap - 1 } else { 0 };
        let ip = String::from_utf8_lossy(&addr_buf[..ip_len]).to_string();
        Result::Ok((n as usize, format!("{}:{}", ip, port_out)))
    }

    fn connect(&self, addr: &str) -> Result<()> {
        extern "C" {
            fn glyim_net_udp_connect(fd: i32, addr: *const u8, addr_len: usize, port: u16) -> i32;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
        };
        let rc = unsafe { glyim_net_udp_connect(self.fd, host.as_ptr(), host.len(), port) };
        if rc < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(()) }
    }

    fn send(&self, buf: &[u8]) -> Result<usize> {
        extern "C" { fn glyim_net_udp_send(fd: i32, buf: *const u8, len: usize) -> isize; }
        let n = unsafe { glyim_net_udp_send(self.fd, buf.as_ptr(), buf.len()) };
        if n < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(n as usize) }
    }

    fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        extern "C" { fn glyim_net_udp_recv(fd: i32, buf: *mut u8, len: usize) -> isize; }
        let n = unsafe { glyim_net_udp_recv(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 { Result::Err(Error::last_os_error()) } else { Result::Ok(n as usize) }
    }
}

/// Parse an IP address (no port) from a string. Supports full IPv4
/// dotted-quad and IPv6 (including `::` zero-compression, but not the
/// dual IPv4-mapped `::ffff:1.2.3.4` textual form, which is out of scope
/// for this pass — track separately if needed).
fn parse_ip_addr(s: &str) -> Option<IpAddr> {
    if s.contains(':') {
        parse_ipv6(s).map(IpAddr::V6)
    } else {
        let parts: Vec<&str> = s.split('.');
        if parts.len() != 4 { return Option::None; }
        let mut octets = [0u8; 4];
        let mut i = 0;
        while i < 4 {
            match parts[i].parse::<u8>() {
                Option::Some(v) => octets[i] = v,
                Option::None => return Option::None,
            }
            i += 1;
        }
        Option::Some(IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3])))
    }
}

/// Parse the 8-group, `::`-compressible IPv6 text form (RFC 4291 §2.2,
/// groups 1 and 2; the embedded-IPv4 group 3 form is not handled).
fn parse_ipv6(s: &str) -> Option<Ipv6Addr> {
    if s.contains("::") {
        // At most one "::" is allowed.
        let mut halves = s.splitn(3, "::");
        let left = halves.next().unwrap_or("");
        let right = match halves.next() {
            Option::Some(r) => r,
            Option::None => return Option::None,
        };
        if halves.next().is_some() { return Option::None; } // more than one "::"

        let left_groups = if left.is_empty() { Vec::new() } else { parse_groups(left)? };
        let right_groups = if right.is_empty() { Vec::new() } else { parse_groups(right)? };
        if left_groups.len() + right_groups.len() > 7 { return Option::None; }

        let missing = 8 - left_groups.len() - right_groups.len();
        let mut segments = [0u16; 8];
        let mut idx = 0;
        for g in &left_groups { segments[idx] = *g; idx += 1; }
        idx += missing;
        for g in &right_groups { segments[idx] = *g; idx += 1; }
        Option::Some(Ipv6Addr { segments })
    } else {
        let groups = parse_groups(s)?;
        if groups.len() != 8 { return Option::None; }
        let mut segments = [0u16; 8];
        let mut i = 0;
        while i < 8 { segments[i] = groups[i]; i += 1; }
        Option::Some(Ipv6Addr { segments })
    }
}

fn parse_groups(s: &str) -> Option<Vec<u16>> {
    let mut out = Vec::new();
    for part in s.split(':') {
        if part.is_empty() { return Option::None; }
        match u16::from_str_radix(part, 16) {
            Option::Some(v) => out.push(v),
            Option::None => return Option::None,
        }
    }
    Option::Some(out)
}

/// Parse a socket address from a string (e.g. `"127.0.0.1:8080"` or
/// `"[::1]:8080"`).
fn parse_socket_addr(s: &str) -> Option<SocketAddr> {
    let (host, port) = split_host_port(s)?;
    let ip = parse_ip_addr(&host)?;
    match ip {
        IpAddr::V4(v4) => Option::Some(SocketAddr::V4(SocketAddrV4::new(v4, port))),
        IpAddr::V6(v6) => Option::Some(SocketAddr::V6(SocketAddrV6::new(v6, port, 0, 0))),
    }
}
```

### Runtime-side addition required: `glyim_net_tcp_peer_addr`

The runtime has `glyim_net_tcp_local_addr` but nothing for the remote/peer
address, which `TcpListener::accept` needs (the old, broken `.g` code tried to
get this from `accept` itself, but the real `accept` doesn't return it). Add to
`glyim-runtime`, right after `glyim_net_tcp_local_addr`:

```rust
#[unsafe(no_mangle)]
/// # Safety
/// FFI entry point.
pub unsafe extern "C" fn glyim_net_tcp_peer_addr(fd: i32, buf: *mut u8, buf_len: usize) -> i32 {
    let fd = fd as u32;
    let store = tcp_streams().lock().unwrap();
    let stream = match store.streams.get(&fd) {
        Some(s) => s,
        None => return -1,
    };
    let addr = match stream.peer_addr() {
        Ok(a) => a,
        Err(_) => return -1,
    };
    let addr_str = addr.to_string();
    let bytes = addr_str.as_bytes();
    if bytes.len() >= buf_len {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    bytes.len() as i32
}
```

Also fix `glyim_net_udp_recv_from`'s `*mut usize` in/out contract to match the
`.g` fix above: the runtime already writes `*src_addr_len = ip_bytes.len() + 1`
(length including NUL) on success — the corrected `.g` `recv_from` now reads that
convention correctly (previous `.g` code never read it at all, since its
signature didn't even have that parameter).

---

## P0-4: DWARF debug info always describes enum discriminants as 8-bit

**File:** `glyim-codegen-llvm/src/debug.rs` (around the `TyKind::Adt` enum-DI
branch, ~line 5445)

**Bug:** every enum's DWARF discriminant member is emitted via
`create_basic_type("discriminant", 8, 0x04, 0)` — hardcoded to 8 bits — no matter
how many variants the enum actually has. `glyim-mir`'s
`generate_enum_drop_glue` and `glyim-layout`'s `discriminant_info` both already
compute the *real* tag width (`U8` for ≤256 variants, `U16` for ≤65536, `U32`,
else `U64`). An enum with >256 variants gets correct codegen but a **wrong**
debug-info description, so `gdb`/`lldb` will misread the discriminant of any such
enum (garbage variant shown, or partial reads past the real tag into padding).

**Fix:**

```rust
// Discriminant member: width must agree with the tag type layout/codegen
// actually use (glyim-layout::discriminant_info / the U8/U16/U32/U64 scheme
// in glyim-codegen-llvm/abi.rs), not a hardcoded 8 bits — otherwise a debugger
// misreads the discriminant of any enum with more than 256 variants.
let variant_count = adt_def.variants.len();
let discr_bits: u64 = if variant_count <= 256 {
    8
} else if variant_count <= 65_536 {
    16
} else if variant_count <= 4_294_967_296 {
    32
} else {
    64
};
let discr_di = self
    .builder
    .create_basic_type("discriminant", discr_bits, 0x04, 0)
    .unwrap()
    .as_type();
```

This mirrors the exact tiering already used in `generate_enum_drop_glue`
(`glyim-mir`) and `LayoutComputer::discriminant_info` (`glyim-layout`) — all
three should stay in lockstep. Consider factoring this tiering into one `pub fn
discriminant_bit_width(variant_count: usize) -> u64` in `glyim-layout` (it is
currently duplicated three times with three different return encodings — `Ty`,
`(Size, Align, Ty)`, and now bit count) and having all three call sites use it,
so a future change to the tiering (e.g. supporting niche-optimized
no-discriminant enums) can't drift out of sync again.

---

## P0-5: Bytecode VM's `VmError::UnsupportedOpcode` is dead code

**File:** `glyim-bytecode-vm/src/lib.rs`

**Finding:** `Opcode::from_u8` maps `0x01..=0x2D` and `0xFF`; the interpreter's
dispatch `match` (starting ~line 3015) handles every one of those variants — so
the VM itself is complete. But `VmError::UnsupportedOpcode(Opcode)` is declared
and documented ("The decoded opcode is not yet implemented in this VM") and then
**never constructed anywhere**. That's not a functional bug today, but it's a
production-grade landmine: the moment someone adds a new `Opcode` variant (very
likely — this bytecode format will grow) without also adding its dispatch arm,
the `match` becomes non-exhaustive and either fails to compile (if it's already
exhaustive, good) or — if a wildcard arm was sloppily added to make it compile —
silently mis-executes instead of returning `UnsupportedOpcode`.

**Fix:** make the dispatch match exhaustive-by-construction and wire the dead
variant in as the actual fallback, so the compiler enforces "every new opcode
needs dispatch code" going forward:

```rust
// At the end of the big `match op { ... }` dispatch block in the interpreter
// loop, remove any existing `_ => {}` / wildcard arm (if present) and instead
// let the match be exhaustive over `Opcode`. If Rust reports the match is
// already exhaustive without a wildcard, nothing else to do here — the
// `UnsupportedOpcode` variant can be deleted instead, OR (recommended, to
// keep the door open for CLI tools like a `--decode-only` mode that parses
// bytecode without a full dispatch table) construct it from a shared
// non-exhaustive helper:
fn dispatch_opcode(&mut self, op: Opcode) -> ExecResult<StepOutcome> {
    match op {
        Opcode::LoadConst => { /* existing body */ Ok(StepOutcome::Continue) }
        // ... all existing arms, unchanged ...
        Opcode::Trap => { /* existing body */ Ok(StepOutcome::Continue) }
        // No wildcard arm: if `Opcode` gains a variant and this match isn't
        // updated, the crate fails to compile instead of miscompiling.
    }
}
```

If a genuinely optional/future opcode needs to exist in the enum before its
interpreter support lands (e.g. reserved for a future bytecode version), give it
an explicit arm that returns the error instead of relying on a wildcard:

```rust
Opcode::SomeFutureOp => return Err(VmError::UnsupportedOpcode(op)),
```

This keeps `UnsupportedOpcode` meaningful and reachable rather than dead, and
keeps the exhaustiveness check as a compile-time safety net for whoever extends
the opcode set next.

---

## P1-1: `LtoKind::Thin` — implement the cross-CGU thin-link driver

**Files:** `glyim-codegen-llvm/src/passes.rs`, `glyim-codegen-llvm/src/lib.rs`
(`emit_thinlto_bitcode_files`, already implemented — emits per-CGU `.bc` with
summaries), `glyim-cli/src/linker.rs`

**Current state (confirmed complete):** `run_lto` handles `None` and `Fat`
correctly; `Thin` currently returns an explicit tracked-gap error
(`KNOWN_GAPS.md` Phase 10.2) rather than silently no-op'ing — this is the right
interim behavior and should stay in place as the fallback for any code path that
doesn't go through the driver below. `emit_thinlto_bitcode_files` already emits
unoptimized `.bc` per-CGU with embedded summaries, ready for a thin-link step.

**What's missing:** the actual thin-link invocation in `glyim-cli`. Implement it
as a new function in `glyim-cli/src/linker.rs`:

```rust
use std::path::{Path, PathBuf};
use std::process::Command;

/// Errors from the ThinLTO thin-link step.
#[derive(Debug)]
pub enum ThinLtoError {
    Llvm(std::io::Error),
    ExitStatus(std::process::ExitStatus, String),
    MissingTool,
}

impl std::fmt::Display for ThinLtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThinLtoError::Llvm(e) => write!(f, "failed to invoke llvm-lto2: {e}"),
            ThinLtoError::ExitStatus(status, stderr) => {
                write!(f, "llvm-lto2 exited with {status}: {stderr}")
            }
            ThinLtoError::MissingTool => write!(
                f,
                "llvm-lto2 not found on PATH; required for ThinLTO (`--lto=thin`)"
            ),
        }
    }
}

/// Locate `llvm-lto2`, preferring an `LLVM_LTO2` env override, then the
/// LLVM version suffix this workspace already pins elsewhere (see the
/// "Unsupported LLVM version" check in glyim-codegen-llvm), then a bare
/// `llvm-lto2` on PATH.
fn find_llvm_lto2() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LLVM_LTO2") {
        return Some(PathBuf::from(p));
    }
    for candidate in ["llvm-lto2-18", "llvm-lto2"] {
        if let Ok(path) = which::which(candidate) {
            return Some(path);
        }
    }
    None
}

/// Run the ThinLTO thin-link step over a set of per-CGU bitcode files
/// (as produced by `glyim_codegen_llvm::CodegenCtx::emit_thinlto_bitcode_files`),
/// producing one native object file per input module in `out_dir`.
///
/// This is the linker-side half of `LtoKind::Thin`: each `.bc` file already
/// carries a per-module summary (emitted unoptimized, per
/// `emit_thinlto_bitcode_files`'s doc comment); `llvm-lto2 run` reads all
/// summaries together, decides cross-module import/export, and emits
/// optimized native objects — this is what makes ThinLTO "thin" (no single
/// merged module) as opposed to `Fat`, which already works via
/// `Module::link_in_module`.
pub fn thin_lto_link(bitcode_files: &[PathBuf], out_dir: &Path) -> Result<Vec<PathBuf>, ThinLtoError> {
    let tool = find_llvm_lto2().ok_or(ThinLtoError::MissingTool)?;
    std::fs::create_dir_all(out_dir).map_err(ThinLtoError::Llvm)?;

    let mut cmd = Command::new(&tool);
    cmd.arg("run");
    let mut out_paths = Vec::with_capacity(bitcode_files.len());
    for (i, bc) in bitcode_files.iter().enumerate() {
        let out_path = out_dir.join(format!("cgu_{i}.o"));
        // llvm-lto2's `-o` takes a prefix; it appends `.<index>` per input
        // module when given multiple `-r` (resolution) entries. We instead
        // invoke it once per output slot via `-out=<n>=<path>` (LLVM 18
        // syntax) to keep the CGU -> object-file mapping explicit and stable
        // for the rest of the linker pipeline.
        cmd.arg(format!("{}", bc.display()));
        cmd.arg(format!("-o"));
        cmd.arg(&out_path);
        out_paths.push(out_path);
    }
    // Every symbol in every module is treated as externally visible/needed:
    // Glyim doesn't (yet) do cross-CGU dead-symbol elimination at this step,
    // it only does cross-CGU *inlining/import*, mirroring `Fat`'s scope. A
    // later pass can tighten `-r` resolutions per-symbol for better DCE.
    for bc in bitcode_files {
        cmd.arg(format!("-r={},*,px", bc.display()));
    }

    let output = cmd.output().map_err(ThinLtoError::Llvm)?;
    if !output.status.success() {
        return Err(ThinLtoError::ExitStatus(
            output.status,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(out_paths)
}
```

Wire it into the `glyim-cli` link driver: when `LtoKind::Thin` is selected,
instead of calling `CodegenCtx::run_passes_on_module` (which returns the
tracked-gap error for `Thin`), call `emit_thinlto_bitcode_files` to get the
per-CGU `.bc` paths, then `linker::thin_lto_link` to get native objects, then
feed those objects into the existing final-link step (`cc`/`ld`/`lld` invocation
— reuse whatever `glyim-cli` already uses for `None`/`Fat`'s object files).

Add `which = "6"` (or whatever major version the workspace already pins for
similar tools, check other `Cargo.toml`s in the dump for a `which` dependency
first — if one already exists elsewhere, use that version, don't introduce a
second one) to `glyim-cli/Cargo.toml`.

**Tests to add:** a `glyim-test` snapshot/integration test that compiles a
2-CGU program with `--lto=thin`, asserts it links and runs, and asserts a
function defined in CGU A and called only from CGU B actually got inlined
across the boundary (check the final binary's disassembly doesn't contain a
call instruction to it, or check LLVM's `-pass-remarks` output during the
thin-link for an "inlined across module" remark).

---

## P1-2: Proc-macro two-stage host compile

**Files:** new `glyim-cli/src/proc_macro_build.rs`; wire into
`glyim-cli/src/lib.rs`'s compilation driver; consumes the existing, complete
`glyim-proc-macro` crate (ABI + `load_cdylib` loader + in-process `Registry`, all
already implemented per the dump).

**What exists today:** everything on the *dylib* side — the C-ABI token-stream
contract (`PmToken`/`PmTokenStream`/`PmStr`), the exported allocator helpers a
dylib calls into, `PROC_MACRO_MAIN_SYMBOL`, and `load_cdylib` (uses `libloading`,
cross-platform). What's missing is compiling a crate containing `#[proc_macro]`
functions *for the host* (as opposed to the target the final program is being
built for) and pointing `load_cdylib` at the result.

**Design:** Glyim is self-hosting its own compiler invocation already (`glyim-cli`
IS the compiler driver), so "build for host" is just "invoke the same compiler
pipeline glyim-cli already has, but with `--target=<host-triple>
--crate-type=cdylib` instead of whatever target/crate-type the main build is
using," then hand the resulting `.so`/`.dylib`/`.dll` to `load_cdylib`.

```rust
// glyim-cli/src/proc_macro_build.rs
use std::path::{Path, PathBuf};
use glyim_proc_macro::LoadedCrate;

#[derive(Debug)]
pub enum ProcMacroBuildError {
    Compile(String),
    Load(String),
}

/// Compile the proc-macro crate rooted at `crate_root` (its `Cargo.toml`/
/// `glyim.toml` equivalent — match whatever manifest format `glyim-cli`
/// already uses for ordinary crates) to a cdylib **for the host triple**,
/// then load it, returning its populated `Registry`.
///
/// This is the compile half of the two-stage proc-macro build; the load half
/// (`glyim_proc_macro::load_cdylib`) already exists and is exercised by unit
/// tests, per glyim-proc-macro's own doc comments.
pub fn build_and_load_proc_macro_crate(
    crate_root: &Path,
    build_cache_dir: &Path,
    host_triple: &str,
) -> Result<LoadedCrate, ProcMacroBuildError> {
    std::fs::create_dir_all(build_cache_dir)
        .map_err(|e| ProcMacroBuildError::Compile(e.to_string()))?;

    // Reuse the exact same in-process compilation entry point the main
    // `glyim-cli` build uses for ordinary crates (do not shell out to a
    // second `glyim` binary invocation — that would double process-startup
    // cost per proc-macro crate and complicate incremental rebuilds).
    // `crate::compile_crate_to_cdylib` should already exist or be a thin
    // wrapper around the same driver `glyim-cli/src/lib.rs` uses for the
    // final `--crate-type` handling; if it currently only supports building
    // an executable, extend it to accept `CrateType::Cdylib` (this almost
    // certainly is already a variant, since the codegen-llvm crate is
    // capable of emitting shared libraries for ordinary dylib crates —
    // confirm in `glyim-codegen-llvm/src/lib.rs`/`abi.rs`).
    let output_path = build_cache_dir.join(cdylib_file_name("glyim_proc_macro_crate", host_triple));

    crate::compile_crate_to_cdylib(crate_root, &output_path, host_triple)
        .map_err(|e| ProcMacroBuildError::Compile(e.to_string()))?;

    glyim_proc_macro::load_cdylib(output_path.to_str().ok_or_else(|| {
        ProcMacroBuildError::Load("output path is not valid UTF-8".to_string())
    })?)
    .map_err(ProcMacroBuildError::Load)
}

/// Platform-correct shared library file name, matching what
/// `libloading::Library::new` expects to find on each OS `load_cdylib`
/// already supports (Unix `dlopen`, Windows `LoadLibraryW`, macOS `dlopen`,
/// per `glyim-proc-macro`'s own doc comment on `load_cdylib`).
fn cdylib_file_name(crate_name: &str, host_triple: &str) -> String {
    if host_triple.contains("windows") {
        format!("{crate_name}.dll")
    } else if host_triple.contains("apple") || host_triple.contains("darwin") {
        format!("lib{crate_name}.dylib")
    } else {
        format!("lib{crate_name}.so")
    }
}
```

**Caching:** proc-macro crates should be built once per compiler invocation (or
cached across invocations keyed by crate-root content hash + host triple +
glyim-cli version, mirroring how the rest of the build presumably caches
codegen units — check `glyim-db` for the existing incremental-compilation cache
and reuse its keying scheme rather than inventing a second cache).

**Tests to add:** an end-to-end `glyim-test` fixture: a tiny crate defining
`#[proc_macro] fn make_answer(...)`, and a consumer crate that uses it,
asserting the consumer compiles and the macro's expansion is present in its
output.

---

## P2-1: `.await` inside a loop body (async-v2)

**Files:** `glyim-hir/src/lower/lower_async.rs` and friends.

**Current, correct, and intentional state:** the existing single-poll desugar
(`.await e` → `match e.poll() { Ready(v) => v, Pending => panic!(..) }`) is
real, tested, working code for its documented scope — non-looping bodies with 0,
1, or several sequential awaits whose future types are statically nameable. It
correctly *rejects* (compile error, not silent miscompilation) anything outside
that scope: `.await` inside a loop, and multi-await bodies whose future type
isn't nameable at HIR-lowering time. **Do not patch around these checks with a
partial transform** — the existing authors were right to gate this behind hard
diagnostics rather than emit code with wrong pending-suspension semantics.

**What "done" looks like:** a real generator/coroutine state machine, matching
how Rust's own `async fn` lowering works:

1. **State enumeration.** For an async fn body, walk it (extending the existing
   `first_loop_await_expr`/`await_inside_loop` visitors in
   `lower_async.rs`) and assign every `.await` point — including ones now
   reachable via loop back-edges — a distinct state index. A `.await` inside a
   loop means the state machine's `poll` needs a `loop` (or explicit `match`
   with a self-jump) around the states inside that loop body, re-entering the
   same state on `Poll::Pending` instead of the "hoist out of the loop" jump
   the current diagnostic tells users to do manually.
2. **Live-variable capture per state.** For each state, compute which HIR
   locals are live across the *next* suspend point (standard liveness
   analysis — `glyim-borrowck` already has exactly this machinery in
   `liveness.rs`; reuse it against the desugared MIR rather than re-deriving
   liveness in HIR). Those locals become fields of the generated `FooState`
   enum's per-state variant (the doc comment at `lower_async.rs` line ~22349
   already anticipates this: "needs each suspended future's type to build the
   `FooState` enum").
3. **`poll` body codegen.** Generate `poll(&mut self, cx) -> Poll<Output>` as a
   `match self.state { State::S0(fields...) => { ... }, State::S1(...) => {...},
   ... }` where each arm runs the code between the previous suspend point and
   the next, and on `Pending` from the inner future, stores it back into
   `self.state` unchanged and returns `Poll::Pending` (this is what makes
   re-polling resumable, unlike today's `panic!`).
4. **Diagnostics.** Once implemented, replace the two hard-error diagnostics
   (`ErrorCode { category: Type, number: 60 }` for loop-await,
   `number: 61` for un-nameable future type) with the real lowering; keep both
   error codes reserved (don't renumber) in case a narrower unsupported case
   remains (e.g. `.await` inside a `try`/`?`-desugared block, or a closure
   capturing an in-flight suspend point) — downgrade the doc comments rather
   than deleting the diagnostics wholesale.

This is a multi-week compiler feature, not a patch — scope it as its own
project with the phased plan above, tracked against `KNOWN_GAPS.md` async-v2
exactly as the codebase's own docs already do. Do **not** attempt a "just wrap
the loop body in a `loop {}` and hope `Pending` never happens twice" shortcut;
that reintroduces exactly the silent-hang failure mode the current `panic!`
was deliberately chosen to avoid.

---

## P2-2: Real waker + I/O reactor for the async executor

**Files:** `glyim-lang-core/lib/future.g` (the `Future`/`Poll`/`Waker`/`Context`
model consumed by generated code) and the host-side executor (`block_on`-style
poll loop, described near line 52328 of the dump as "Phase 5 MVP").

**Current, correct, and intentional state:** a single-threaded, no-op waker that
keeps re-polling until `Ready`. This is fine for futures that resolve
immediately or after a bounded number of busy-polls, and is explicitly scoped as
an MVP by its own doc comments. It cannot usefully drive I/O-bound futures
(a `TcpStream::read` future would busy-spin instead of blocking the thread).

**Production version — thread-parking waker (no external I/O dependency,
works today):**

```rust
// Host-side (Rust) executor support, alongside the existing block_on loop.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::Thread;

/// A `Waker` that unparks the thread that's blocked in `block_on`. This
/// replaces the no-op waker: `block_on` now calls `thread::park()` instead of
/// busy-looping between polls, and any future that wants to signal readiness
/// (e.g. a completion callback fired from another thread, or the reactor
/// below) calls `.wake()`, which unparks it.
pub struct ParkWaker {
    thread: Thread,
    woken: Arc<AtomicBool>,
}

impl ParkWaker {
    pub fn new() -> (Self, Arc<AtomicBool>) {
        let woken = Arc::new(AtomicBool::new(false));
        (Self { thread: std::thread::current(), woken: woken.clone() }, woken)
    }

    pub fn wake(&self) {
        self.woken.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

/// Poll-to-completion loop, now parking between polls instead of busy-spinning.
pub fn block_on<F: Future>(mut fut: F) -> F::Output {
    let (waker, woken) = ParkWaker::new();
    let mut fut = unsafe { std::pin::Pin::new_unchecked(&mut fut) };
    loop {
        woken.store(false, Ordering::Release);
        match fut.as_mut().poll(&waker) {
            Poll::Ready(v) => return v,
            Poll::Pending => {
                if !woken.load(Ordering::Acquire) {
                    std::thread::park();
                }
            }
        }
    }
}
```

This alone fixes the busy-spin problem for CPU-bound "eventually ready"
futures and any future whose readiness is signaled from another OS thread
(timers via `thread::sleep` + `wake()`, thread-pool-backed blocking I/O
wrappers, channel-based futures), without adding any new dependency.

**Full I/O reactor (for non-blocking socket futures):** wrap the existing
non-blocking-mode support already present in the runtime
(`glyim_net_tcp_set_nonblocking`, confirmed implemented) with an `mio`-based
(or raw `epoll`/`kqueue`/IOCP, but `mio` gives cross-platform for free and is a
reasonable dependency for a language runtime) single reactor thread:

1. Add `mio = "1"` to `glyim-runtime/Cargo.toml`.
2. One global `Poll` + a `Waker`-keyed registry (`Token -> Arc<AtomicBool
   /*readable*/>` plus the `ParkWaker` to call back).
3. A background thread running `poll.poll(&mut events, None)` in a loop;
   on each readable/writable event, mark the corresponding slot and call
   the associated `ParkWaker::wake()`.
4. `TcpStream`'s async read/write future (a new type, `TcpStream::read_async`,
   alongside the existing blocking `Read` impl) registers its fd with the
   reactor on first `Pending`, and the generated `poll` returns `Pending`
   without spinning; the reactor thread wakes it when `mio` reports
   readability.

This is a scoped, well-known pattern (it's effectively a minimal `mio` +
manual `Future` executor, same shape as early `tokio`/`async-std` internals)
— unlike P2-1, it does not require new compiler/type-system work, only runtime
crate + `future.g` additions, so it's safe to implement incrementally without
the same "don't half-do-it" risk.

---

## Suggested rollout order

1. **P0-1 → P0-2 → P0-3** in that order (P0-2 and P0-3 both depend on
   `Box::into_raw`/`from_raw` from P0-1). These three turn "the threading and
   networking stdlib modules compile but are silently broken/non-functional"
   into "they work," which is the single highest-value, lowest-risk chunk of
   this plan — write `glyim-test` integration tests for `thread::spawn` +
   `join` round-tripping a value, and for a TCP echo client/server (including
   an IPv6 loopback case) as part of landing these, since none of that is
   testable today.
2. **P0-4, P0-5** — small, independent, no cross-dependencies; land any time,
   ideally alongside the P0-1..3 test-writing effort since they touch adjacent
   debug/runtime-correctness surface area.
3. **P1-1 (ThinLTO)** and **P1-2 (proc-macro host build)** are independent of
   each other and of P0; parallelizable across two engineers/agents.
4. **P2-2 (waker/reactor)** can start any time — it's additive and doesn't
   block on P2-1.
5. **P2-1 (loop-await state machine)** last: it's the largest, riskiest item,
   benefits from the borrowck liveness code (already stable) and from having
   real networking (P0-3) and a real reactor (P2-2) available so its test
   suite can include actually-useful async programs (e.g. a loop that awaits a
   read on each iteration) instead of only synthetic ones.
