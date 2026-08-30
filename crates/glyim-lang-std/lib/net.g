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
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
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
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
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
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
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
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
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
            Option::None => return Result::Err(Error::invalid_input("invalid address")),
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

/// Parse an IP address from a string.
fn parse_ip_addr(s: &str) -> Option<IpAddr> {
    if s.contains(':') {
        // IPv6 (may contain `::` zero-compression). An IPv4-mapped form
        // (`::ffff:1.2.3.4`) is out of scope for this pass.
        parse_ipv6(s).map(IpAddr::V6)
    } else {
        let parts: Vec<&str> = s.split('.');
        if parts.len() != 4 {
            return Option::None;
        }
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

/// Parse a socket address from a string (e.g. "127.0.0.1:8080").
fn parse_socket_addr(s: &str) -> Option<SocketAddr> {
    let parts: Vec<&str> = s.rsplitn(2, ':');
    if parts.len() != 2 {
        return Option::None;
    }
    let port = parts[0].parse::<u16>()?;
    let ip = parse_ip_addr(parts[1])?;
    match ip {
        IpAddr::V4(v4) => Option::Some(SocketAddr::V4(SocketAddrV4::new(v4, port))),
        IpAddr::V6(v6) => Option::Some(SocketAddr::V6(SocketAddrV6::new(v6, port, 0, 0))),
    }
}
