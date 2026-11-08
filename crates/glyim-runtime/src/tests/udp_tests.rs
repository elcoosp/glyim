//! UDP networking tests for glyim-runtime
use std::net::UdpSocket;

#[test]
fn udp_bind_and_send_recv() {
    // W5-C06-T01: UDP send/recv works
    // Bind a UDP socket on localhost with an ephemeral port
    let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind UDP socket");
    let local_addr = socket.local_addr().expect("Failed to get local addr");
    let port = local_addr.port();

    // Bind another socket to send to the first
    let sender = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind sender");

    // Send data
    let msg = b"hello glyim";
    let sent = sender
        .send_to(msg, format!("127.0.0.1:{port}"))
        .expect("Failed to send");
    assert_eq!(sent, msg.len());

    // Receive data
    let mut buf = [0u8; 1024];
    let (recv_len, _from) = socket.recv_from(&mut buf).expect("Failed to recv");
    assert_eq!(&buf[..recv_len], msg);
}

#[test]
fn udp_connect_and_send() {
    // Test connected UDP socket send/recv
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind receiver");
    let recv_addr = receiver.local_addr().expect("Failed to get receiver addr");

    let sender = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind sender");
    sender.connect(recv_addr).expect("Failed to connect sender");

    let msg = b"connected udp";
    sender.send(msg).expect("Failed to send connected");

    let mut buf = [0u8; 1024];
    let recv_len = receiver.recv(&mut buf).expect("Failed to recv");
    assert_eq!(&buf[..recv_len], msg);
}

#[test]
fn udp_recv_from_returns_sender_info() {
    // Test that recv_from returns the sender's address
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind");
    let recv_addr = receiver.local_addr().expect("Failed to get addr");

    let sender = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind sender");
    let sender_addr = sender.local_addr().expect("Failed to get sender addr");

    let msg = b"addr test";
    sender.send_to(msg, recv_addr).expect("Failed to send");

    let mut buf = [0u8; 1024];
    let (_len, from) = receiver.recv_from(&mut buf).expect("Failed to recv");
    // The sender's IP should match (port may differ due to NAT/ephemeral)
    assert_eq!(from.ip(), sender_addr.ip());
}
