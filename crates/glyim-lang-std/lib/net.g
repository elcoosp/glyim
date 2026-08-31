//! Networking primitives for the Glyim standard library.
//!
//! This module provides networking functionality for TCP, UDP, and IP address handling.

use io::{Read, Write, Error, Result};

/// An IP address, either IPv4 or IPv6.
enum IpAddr {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

/// An IPv4 address.
struct Ipv4Addr {
    octets: [u8; 4],
}

impl Ipv4Addr {
    /// Create a new IPv4 address from octets.
    fn new(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
        Ipv4Addr { octets: [a, b, c, d] }
    }

    /// Return the octets of the address.
    fn octets(&self) -> &[u8; 4] {
        &self.octets
    }

    /// Return `true` for the special 'unspecified' address (0.0.0.0).
    fn is_unspecified(&self) -> bool {
        self.octets == [0, 0, 0, 0]
    }

    /// Return `true` for the loopback address (127.0.0.0/8).
    fn is_loopback(&self) -> bool {
        self.octets[0] == 127
    }

    /// The localhost address (127.0.0.1).
    fn localhost() -> Ipv4Addr {
        Ipv4Addr::new(127, 0, 0, 1)
    }

    /// The unspecified address (0.0.0.0).
    fn unspecified() -> Ipv4Addr {
        Ipv4Addr::new(0, 0, 0, 0)
    }
}

/// An IPv6 address.
struct Ipv6Addr {
    segments: [u16; 8],
}

impl Ipv6Addr {
    /// Create a new IPv6 address from eight 16-bit segments.
    fn new(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> Ipv6Addr {
        Ipv6Addr { segments: [a, b, c, d, e, f, g, h] }
    }

    /// Return the segments of the address.
    fn segments(&self) -> &[u16; 8] {
        &self.segments
    }

    /// Return `true` for the special 'unspecified' address (::).
    fn is_unspecified(&self) -> bool {
        self.segments == [0, 0, 0, 0, 0, 0, 0, 0]
    }

    /// Return `true` for the loopback address (::1).
    fn is_loopback(&self) -> bool {
        self.segments == [0, 0, 0, 0, 0, 0, 0, 1]
    }

    /// The localhost address (::1).
    fn localhost() -> Ipv6Addr {
        Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)
    }

    /// The unspecified address (::).
    fn unspecified() -> Ipv6Addr {
        Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0)
    }
}

/// A socket address, either IPv4 or IPv6.
enum SocketAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
}

/// A socket address for IPv4.
struct SocketAddrV4 {
    ip: Ipv4Addr,
    port: u16,
}

impl SocketAddrV4 {
    /// Create a new socket address from an IP and port.
    fn new(ip: Ipv4Addr, port: u16) -> SocketAddrV4 {
        SocketAddrV4 { ip, port }
    }

    /// Return the IP address.
    fn ip(&self) -> &Ipv4Addr {
        &self.ip
    }

    /// Return the port.
    fn port(&self) -> u16 {
        self.port
    }
}

/// A socket address for IPv6.
struct SocketAddrV6 {
    ip: Ipv6Addr,
    port: u16,
    flowinfo: u32,
    scope_id: u32,
}

impl SocketAddrV6 {
    /// Create a new socket address from an IP, port, flowinfo, and scope_id.
    fn new(ip: Ipv6Addr, port: u16, flowinfo: u32, scope_id: u32) -> SocketAddrV6 {
        SocketAddrV6 { ip, port, flowinfo, scope_id }
    }

    /// Return the IP address.
    fn ip(&self) -> &Ipv6Addr {
        &self.ip
    }

    /// Return the port.
    fn port(&self) -> u16 {
        self.port
    }
}

/// A TCP stream between a local and a remote socket.
struct TcpStream {
    fd: i32,
}

impl TcpStream {
    /// Open a TCP connection to a remote host.
    fn connect(addr: &str) -> Result<TcpStream> {
        extern "C" {
            fn glyim_net_tcp_connect(addr: *const u8, addr_len: usize, port: u16) -> i32;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address".to_string())),
        };
        let fd = unsafe { glyim_net_tcp_connect(host.as_ptr(), host.len(), port) };
        if fd < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(TcpStream { fd })
        }
    }

    /// Set the read timeout.
    fn set_read_timeout(&self, dur: Option<Duration>) -> Result<()> {
        extern "C" {
            fn glyim_net_set_read_timeout(fd: i32, secs: u64, nanos: u32) -> i32;
        }
        let (secs, nanos) = match dur {
            Option::Some(d) => (d.as_secs(), d.subsec_nanos()),
            Option::None => (0, 0),
        };
        let rc = unsafe { glyim_net_set_read_timeout(self.fd, secs, nanos) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(())
        }
    }

    /// Set the write timeout.
    fn set_write_timeout(&self, dur: Option<Duration>) -> Result<()> {
        extern "C" {
            fn glyim_net_set_write_timeout(fd: i32, secs: u64, nanos: u32) -> i32;
        }
        let (secs, nanos) = match dur {
            Option::Some(d) => (d.as_secs(), d.subsec_nanos()),
            Option::None => (0, 0),
        };
        let rc = unsafe { glyim_net_set_write_timeout(self.fd, secs, nanos) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(())
        }
    }

    /// Put this socket into non-blocking mode. Required before driving the
    /// connection through the async I/O reactor (`glyim_reactor_register`).
    fn set_nonblocking(&self, nonblocking: bool) -> Result<()> {
        extern "C" {
            fn glyim_net_tcp_set_nonblocking(fd: i32, enabled: i32) -> i32;
        }
        let rc = unsafe { glyim_net_tcp_set_nonblocking(self.fd, if nonblocking { 1 } else { 0 }) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(())
        }
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        extern "C" {
            fn glyim_net_tcp_read(fd: i32, buf: *mut u8, len: usize) -> isize;
        }
        let n = unsafe { glyim_net_tcp_read(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        extern "C" {
            fn glyim_net_tcp_write(fd: i32, buf: *const u8, len: usize) -> isize;
        }
        let n = unsafe { glyim_net_tcp_write(self.fd, buf.as_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }

    fn flush(&mut self) -> Result<()> {
        Result::Ok(())
    }
}

/// A future returned by [`TcpStream::read_async`]: reads from a non-blocking
/// socket, registering the fd with the async I/O reactor on first poll so the
/// executor parks (instead of busy-spinning) until the reactor reports
/// readability.
struct ReadFuture<'a> {
    stream: &'a mut TcpStream,
    buf: &'a mut [u8],
    registered: bool,
    token: usize,
}

impl<'a> Future for ReadFuture<'a> {
    type Output = Result<usize, Error>;

    fn poll(&mut self, _cx: &mut Context) -> Poll<Result<usize, Error>> {
        extern "C" {
            fn glyim_reactor_register(fd: i32, interest: u32, thread_id: usize) -> usize;
            fn glyim_reactor_deregister(token: usize);
        }
        if !self.registered {
            // Put the socket in non-blocking mode and register it with the
            // global reactor, keyed on the current executor thread. The reactor
            // will `unpark` this thread when the fd becomes readable.
            self.stream.set_nonblocking(true).expect("set_nonblocking");
            let tid = thread::current_id() as usize;
            self.token = unsafe { glyim_reactor_register(self.stream.fd, 1, tid) };
            self.registered = true;
            return Poll::Pending;
        }
        match self.stream.read(self.buf) {
            Result::Ok(n) => {
                unsafe { glyim_reactor_deregister(self.token) };
                Poll::Ready(Result::Ok(n))
            }
            Result::Err(e) => {
                if e.kind() == ErrorKind::WouldBlock {
                    // Not ready yet; the reactor will wake us.
                    Poll::Pending
                } else {
                    unsafe { glyim_reactor_deregister(self.token) };
                    Poll::Ready(Result::Err(e))
                }
            }
        }
    }
}

/// A future returned by [`TcpStream::write_async`]: writes to a non-blocking
/// socket, registering the fd with the async I/O reactor on first poll.
struct WriteFuture<'a> {
    stream: &'a mut TcpStream,
    buf: &'a [u8],
    written: usize,
    registered: bool,
    token: usize,
}

impl<'a> Future for WriteFuture<'a> {
    type Output = Result<usize, Error>;

    fn poll(&mut self, _cx: &mut Context) -> Poll<Result<usize, Error>> {
        if !self.registered {
            self.stream.set_nonblocking(true).expect("set_nonblocking");
            let tid = thread::current_id() as usize;
            self.token = unsafe { glyim_reactor_register(self.stream.fd, 2, tid) };
            self.registered = true;
            return Poll::Pending;
        }
        // Write whatever remains; loop until the socket accepts bytes or blocks.
        while self.written < self.buf.len() {
            match self.stream.write(&self.buf[self.written..]) {
                Result::Ok(0) => break,
                Result::Ok(n) => self.written += n,
                Result::Err(e) => {
                    if e.kind() == ErrorKind::WouldBlock {
                        return Poll::Pending;
                    }
                    unsafe { glyim_reactor_deregister(self.token) };
                    return Poll::Ready(Result::Err(e));
                }
            }
        }
        unsafe { glyim_reactor_deregister(self.token) };
        Poll::Ready(Result::Ok(self.written))
    }
}

impl TcpStream {
    /// Asynchronously read from this socket. Returns a future that resolves to
    /// the number of bytes read. The fd is registered with the async I/O
    /// reactor on first poll.
    fn read_async<'b>(&'b mut self, buf: &'b mut [u8]) -> ReadFuture<'b> {
        ReadFuture {
            stream: self,
            buf,
            registered: false,
            token: 0,
        }
    }

    /// Asynchronously write to this socket. Returns a future that resolves to
    /// the number of bytes written. The fd is registered with the async I/O
    /// reactor on first poll.
    fn write_async<'b>(&'b mut self, buf: &'b [u8]) -> WriteFuture<'b> {
        WriteFuture {
            stream: self,
            buf,
            written: 0,
            registered: false,
            token: 0,
        }
    }
}

/// A TCP socket server, listening for connections.
struct TcpListener {
    fd: i32,
}

impl TcpListener {
    /// Create a new `TcpListener` bound to the specified address.
    fn bind(addr: &str) -> Result<TcpListener> {
        extern "C" {
            fn glyim_net_tcp_bind(addr: *const u8, addr_len: usize, port: u16) -> i32;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address".to_string())),
        };
        let fd = unsafe { glyim_net_tcp_bind(host.as_ptr(), host.len(), port) };
        if fd < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(TcpListener { fd })
        }
    }

    /// Accept a new incoming connection.
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

    /// Returns the local socket address of this listener.
    fn local_addr(&self) -> Result<String> {
        extern "C" { fn glyim_net_tcp_local_addr(fd: i32, buf: *mut u8, buf_len: usize) -> i32; }
        let mut buf = [0u8; 256];
        let n = unsafe { glyim_net_tcp_local_addr(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(String::from_utf8_lossy(&buf[..n as usize]).to_string())
        }
    }
}

/// A UDP socket.
struct UdpSocket {
    fd: i32,
}

impl UdpSocket {
    /// Create a new `UdpSocket` bound to the specified address.
    fn bind(addr: &str) -> Result<UdpSocket> {
        extern "C" {
            fn glyim_net_udp_bind(addr: *const u8, addr_len: usize, port: u16) -> i32;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address".to_string())),
        };
        let fd = unsafe { glyim_net_udp_bind(host.as_ptr(), host.len(), port) };
        if fd < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(UdpSocket { fd })
        }
    }

    /// Send data on the socket to the given address.
    fn send_to(&self, buf: &[u8], addr: &str) -> Result<usize> {
        extern "C" {
            fn glyim_net_udp_send_to(
                fd: i32, buf: *const u8, len: usize,
                addr: *const u8, addr_len: usize, port: u16,
            ) -> isize;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address".to_string())),
        };
        let n = unsafe { glyim_net_udp_send_to(self.fd, buf.as_ptr(), buf.len(), host.as_ptr(), host.len(), port) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }

    /// Receive data from the socket.
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

    /// Connect this UDP socket to a remote address.
    fn connect(&self, addr: &str) -> Result<()> {
        extern "C" {
            fn glyim_net_udp_connect(fd: i32, addr: *const u8, addr_len: usize, port: u16) -> i32;
        }
        let (host, port) = match split_host_port(addr) {
            Option::Some(hp) => hp,
            Option::None => return Result::Err(Error::invalid_input("invalid address".to_string())),
        };
        let rc = unsafe { glyim_net_udp_connect(self.fd, host.as_ptr(), host.len(), port) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(())
        }
    }

    /// Send data on the socket to the remote address to which it is connected.
    fn send(&self, buf: &[u8]) -> Result<usize> {
        extern "C" {
            fn glyim_net_udp_send(fd: i32, buf: *const u8, len: usize) -> isize;
        }
        let n = unsafe { glyim_net_udp_send(self.fd, buf.as_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }

    /// Receive data on the socket from the remote address to which it is connected.
    fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        extern "C" {
            fn glyim_net_udp_recv(fd: i32, buf: *mut u8, len: usize) -> isize;
        }
        let n = unsafe { glyim_net_udp_recv(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }
}

/// Split an address of the form `host:port` into `(host, port)`.
/// The port separator is the LAST ':' (so IPv6 `::` compression is ignored).
fn split_host_port(addr: &str) -> Option<(String, u16)> {
    let bytes = addr.as_bytes();
    let mut colon = addr.len();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            colon = i;
        }
        i += 1;
    }
    if colon == 0 || colon == addr.len() {
        return Option::None;
    }
    let port = match parse_u16_dec(addr, colon + 1, addr.len()) {
        Option::Some(v) => v,
        Option::None => return Option::None,
    };
    // Build the host string from the byte range [0, colon).
    let mut host: String = String::new();
    let mut j = 0;
    while j < colon {
        host.push(bytes[j] as u8);
        j += 1;
    }
    Option::Some((host, port))
}

/// Parse an IPv6 address from the byte range `s[start..end]`, supporting `::`.
fn parse_ipv6_range(s: &str, start: usize, end: usize) -> Option<Ipv6Addr> {
    if start >= end || end > s.len() {
        return Option::None;
    }
    let bytes = s.as_bytes();
    // Find the `::` compression marker (two consecutive colons) if present.
    let mut dcolon = end; // index of the first ':' of "::", or `end` if absent
    let mut k = start;
    while k + 1 < end {
        if bytes[k] == b':' && bytes[k + 1] == b':' {
            dcolon = k;
            break;
        }
        k += 1;
    }
    let (head_end, tail_start) = if dcolon < end {
        (dcolon, dcolon + 2)
    } else {
        (end, end)
    };

    let mut segments: [u16; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    let mut head_count = 0;
    // Parse head segments [start, head_end) split on ':'.
    if head_end > start {
        let mut seg_start = start;
        let mut idx = start;
        while idx <= head_end {
            if idx == head_end || bytes[idx] == b':' {
                if idx > seg_start {
                    if head_count >= 8 {
                        return Option::None;
                    }
                    segments[head_count] = match parse_u16_hex(s, seg_start, idx) {
                        Option::Some(v) => v,
                        Option::None => return Option::None,
                    };
                    head_count += 1;
                }
                seg_start = idx + 1;
            }
            idx += 1;
        }
    }

    let mut tail_count = 0;
    if tail_start < end {
        // Collect tail segments left-to-right into a temp array.
        let mut tail_segs: [u16; 8] = [0; 8];
        let mut seg_start = tail_start;
        let mut idx = tail_start;
        while idx <= end {
            if idx == end || bytes[idx] == b':' {
                if idx > seg_start {
                    if tail_count >= 8 {
                        return Option::None;
                    }
                    tail_segs[tail_count] = match parse_u16_hex(s, seg_start, idx) {
                        Option::Some(v) => v,
                        Option::None => return Option::None,
                    };
                    tail_count += 1;
                }
                seg_start = idx + 1;
            }
            idx += 1;
        }
        // Place them at the end of `segments`, leaving the `::` gap in the middle.
        let mut i = 0;
        while i < tail_count {
            segments[8 - tail_count + i] = tail_segs[i];
            i += 1;
        }
    } else {
        if head_count != 8 {
            return Option::None;
        }
    }

    if head_count + tail_count > 8 {
        return Option::None;
    }

    Option::Some(Ipv6Addr::new(
        segments[0], segments[1], segments[2], segments[3],
        segments[4], segments[5], segments[6], segments[7],
    ))
}

/// Parse a hexadecimal `u16` from the byte range `s[start..end]`.
fn parse_u16_hex(s: &str, start: usize, end: usize) -> Option<u16> {
    if start >= end || end > s.len() {
        return Option::None;
    }
    let bytes = s.as_bytes();
    let mut value: u32 = 0;
    let mut i = start;
    while i < end {
        let ch = bytes[i];
        let digit: u32 = if ch >= 48 && ch <= 57 {
            // '0'..='9'
            (ch as u32) - 48
        } else if ch >= 97 && ch <= 102 {
            // 'a'..='f'
            (ch as u32) - 87
        } else if ch >= 65 && ch <= 70 {
            // 'A'..='F'
            (ch as u32) - 55
        } else {
            return Option::None;
        };
        value = value * 16 + digit;
        if value > 0xFFFF {
            return Option::None;
        }
        i += 1;
    }
    Option::Some(value as u16)
}

/// Parse a decimal `u8` from the byte range `s[start..end]`.
fn parse_u8_dec(s: &str, start: usize, end: usize) -> Option<u8> {
    if start >= end || end > s.len() {
        return Option::None;
    }
    let bytes = s.as_bytes();
    let mut value: u32 = 0;
    let mut i = start;
    while i < end {
        let ch = bytes[i];
        if ch < b'0' || ch > b'9' {
            return Option::None;
        }
        value = value * 10 + ((ch as u32) - (b'0' as u32));
        if value > 0xFF {
            return Option::None;
        }
        i += 1;
    }
    Option::Some(value as u8)
}

/// Parse a decimal `u16` from the byte range `s[start..end]`.
fn parse_u16_dec(s: &str, start: usize, end: usize) -> Option<u16> {
    if start >= end || end > s.len() {
        return Option::None;
    }
    let bytes = s.as_bytes();
    let mut value: u32 = 0;
    let mut i = start;
    while i < end {
        let ch = bytes[i];
        if ch < b'0' || ch > b'9' {
            return Option::None;
        }
        value = value * 10 + ((ch as u32) - (b'0' as u32));
        if value > 0xFFFF {
            return Option::None;
        }
        i += 1;
    }
    Option::Some(value as u16)
}

/// Parse an IP address from the byte range `s[start..end]`.
fn parse_ip_addr(s: &str, start: usize, end: usize) -> Option<IpAddr> {
    if start >= end || end > s.len() {
        return Option::None;
    }
    let bytes = s.as_bytes();
    // Detect an IPv6 address (contains ':') within the range.
    let mut has_colon = false;
    let mut j = start;
    while j < end {
        if bytes[j] == b':' {
            has_colon = true;
            break;
        }
        j += 1;
    }
    if has_colon {
        // IPv6 (may contain `::` zero-compression). An IPv4-mapped form
        // (`::ffff:1.2.3.4`) is out of scope for this pass.
        parse_ipv6_range(s, start, end).map(IpAddr::V6)
    } else {
        // Manual split on '.' into at most 4 octets (no iterator needed).
        let mut octets = [0u8; 4];
        let mut part_count = 0;
        let mut seg_start = start;
        let mut idx = start;
        while idx <= end {
            if idx == end || bytes[idx] == b'.' {
                let part = if idx > seg_start {
                    match parse_u8_dec(s, seg_start, idx) {
                        Option::Some(v) => v,
                        Option::None => return Option::None,
                    }
                } else {
                    return Option::None;
                };
                octets[part_count] = part;
                part_count += 1;
                if part_count > 4 {
                    return Option::None;
                }
                seg_start = idx + 1;
            }
            idx += 1;
        }
        if part_count != 4 {
            return Option::None;
        }
        Option::Some(IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3])))
    }
}

/// Parse an IP address from the whole string `s`.
fn parse_ip_addr_full(s: &str) -> Option<IpAddr> {
    parse_ip_addr(s, 0, s.len())
}

/// Parse a socket address from a string (e.g. "127.0.0.1:8080").
fn parse_socket_addr(s: &str) -> Option<SocketAddr> {
    // Manual split on the LAST ':' (no rsplitn iterator needed).
    let bytes = s.as_bytes();
    let mut colon = s.len();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            colon = i;
        }
        i += 1;
    }
    if colon == 0 || colon == s.len() {
        return Option::None;
    }
    let port = match parse_u16_dec(s, colon + 1, s.len()) {
        Option::Some(v) => v,
        Option::None => return Option::None,
    };
    let ip = match parse_ip_addr(s, 0, colon) {
        Option::Some(v) => v,
        Option::None => return Option::None,
    };
    match ip {
        IpAddr::V4(v4) => Option::Some(SocketAddr::V4(SocketAddrV4::new(v4, port))),
        IpAddr::V6(v6) => Option::Some(SocketAddr::V6(SocketAddrV6::new(v6, port, 0, 0))),
    }
}
