//! UDP networking tests for glyim-runtime

use crate::*;
use std::net::UdpSocket;

#[test]
fn udp_send_recv() {
    // Create server socket
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server_port = server.local_addr().unwrap().port();

    // Create client socket
    let client_fd = unsafe { glyim_net_udp_bind(c"127.0.0.1".as_ptr().cast::<u8>(), 9, 0) };
    assert!(client_fd >= 0);

    let msg = b"ping";
    let dest_addr = b"127.0.0.1\0";
    let sent = unsafe {
        glyim_net_udp_send_to(
            client_fd,
            msg.as_ptr(),
            msg.len(),
            dest_addr.as_ptr(),
            dest_addr.len() - 1,
            server_port,
        )
    };
    assert_eq!(sent, msg.len() as isize);

    // Receive on server
    let mut buf = [0u8; 4];
    let (n, src_addr) = server.recv_from(&mut buf).unwrap();
    assert_eq!(n, msg.len());
    assert_eq!(&buf[..], msg);

    // Echo back from server
    server.send_to(&buf[..n], src_addr).unwrap();

    // Receive echo on client
    let mut echo_buf = [0u8; 4];
    let mut src_addr_buf = [0u8; 64];
    let mut src_addr_len = src_addr_buf.len();
    let mut src_port = 0;
    let recv = unsafe {
        glyim_net_udp_recv_from(
            client_fd,
            echo_buf.as_mut_ptr(),
            echo_buf.len(),
            src_addr_buf.as_mut_ptr(),
            &mut src_addr_len,
            &mut src_port,
        )
    };
    assert_eq!(recv, msg.len() as isize);
    assert_eq!(&echo_buf[..], msg);
}

#[test]
fn udp_connect_send_recv() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server_port = server.local_addr().unwrap().port();

    let client_fd = unsafe { glyim_net_udp_bind(c"127.0.0.1".as_ptr().cast::<u8>(), 9, 0) };
    assert!(client_fd >= 0);

    let connect_result = unsafe {
        glyim_net_udp_connect(
            client_fd,
            c"127.0.0.1".as_ptr().cast::<u8>(),
            9,
            server_port,
        )
    };
    assert_eq!(connect_result, 0);

    let msg = b"hello";
    let sent = unsafe { glyim_net_udp_send(client_fd, msg.as_ptr(), msg.len()) };
    assert_eq!(sent, msg.len() as isize);

    let mut buf = [0u8; 5];
    let (n, _) = server.recv_from(&mut buf).unwrap();
    assert_eq!(n, msg.len());
    assert_eq!(&buf[..], msg);
}
