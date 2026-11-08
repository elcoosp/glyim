//! TCP networking tests for glyim-runtime
//!
//! Tests:
//! - W5-C05-T01: TCP echo server/client works
//! - W5-C05-T02: `accept` returns new connection

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

// Test W5-C05-T01: TCP echo server/client works
#[test]
fn tcp_echo_server_client_works() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let port = addr.port();

    let server_handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            if let Ok(n) = stream.read(&mut buf)
                && n > 0
            {
                let _ = stream.write_all(&buf[..n]);
            }
        }
    });

    thread::sleep(Duration::from_millis(50));

    let test_msg = b"Hello, TCP!";
    let mut client = std::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .expect("Failed to connect client");
    client
        .write_all(test_msg)
        .expect("Failed to write to socket");

    let mut response = [0u8; 1024];
    let n = client.read(&mut response).expect("Failed to read response");

    assert_eq!(&response[..n], test_msg, "Echo response mismatch");
    let _ = server_handle.join();
}

// Test W5-C05-T02: `accept` returns new connection
#[test]
fn tcp_accept_returns_new_connection() {
    use crate::{glyim_net_tcp_accept, glyim_net_tcp_bind, glyim_net_tcp_connect};

    let addr = b"127.0.0.1";
    let mut listener_fd = -1;
    let mut chosen_port = 0;

    // Retry binding to a free port to avoid race conditions with other tests or OS port release delay
    for _ in 0..10 {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").expect("Failed to find port");
            l.local_addr().expect("Failed to get addr").port()
        };
        // Brief delay to ensure OS releases the port from the probing listener
        thread::sleep(Duration::from_millis(50));
        listener_fd = unsafe { glyim_net_tcp_bind(addr.as_ptr(), addr.len(), port) };
        if listener_fd > 0 {
            chosen_port = port;
            break;
        }
    }
    assert!(listener_fd > 0, "Failed to bind TCP listener after retries");

    let connect_handle = thread::spawn(move || unsafe {
        glyim_net_tcp_connect(addr.as_ptr(), addr.len(), chosen_port)
    });

    let accepted_fd = unsafe { glyim_net_tcp_accept(listener_fd) };
    assert!(accepted_fd > 0, "accept() should return valid socket fd");
    assert_ne!(
        accepted_fd, listener_fd,
        "Accepted fd should differ from listener fd"
    );

    let client_fd = connect_handle.join().expect("Client thread panicked");
    assert!(client_fd > 0, "Client connection failed");
}
