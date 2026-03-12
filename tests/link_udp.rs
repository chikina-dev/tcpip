use std::io;
use std::thread;
use std::time::Duration;

use tcpip_userland::link::LinkEndpoint;
use tcpip_userland::link::ethernet::{EtherType, EthernetFrame, MacAddr};

#[test]
fn udp_link_endpoint_transfers_frames_between_process_like_peers() {
    let left = match std::net::UdpSocket::bind("127.0.0.1:0") {
        Ok(socket) => socket,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to reserve left UDP port: {error}"),
    };
    let right = match std::net::UdpSocket::bind("127.0.0.1:0") {
        Ok(socket) => socket,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to reserve right UDP port: {error}"),
    };
    let left_addr = left.local_addr().unwrap();
    let right_addr = right.local_addr().unwrap();
    drop(left);
    drop(right);

    let left = match LinkEndpoint::udp(left_addr, right_addr, MacAddr::new([0x02, 0, 0, 0, 0, 1])) {
        Ok(endpoint) => endpoint,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to bind left UDP endpoint: {error}"),
    };
    let right = match LinkEndpoint::udp(right_addr, left_addr, MacAddr::new([0x02, 0, 0, 0, 0, 2]))
    {
        Ok(endpoint) => endpoint,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to bind right UDP endpoint: {error}"),
    };

    left.send(EthernetFrame {
        dst: MacAddr::new([0x02, 0, 0, 0, 0, 2]),
        src: MacAddr::new([0x02, 0, 0, 0, 0, 1]),
        ethertype: EtherType::Ipv4,
        payload: b"hello".to_vec(),
    });

    thread::sleep(Duration::from_millis(20));

    let frame = right.recv().unwrap();
    assert_eq!(frame.payload, b"hello".to_vec());
    assert_eq!(frame.ethertype, EtherType::Ipv4);
}

#[test]
fn udp_port_learns_peer_and_replies() {
    let port_socket = match std::net::UdpSocket::bind("127.0.0.1:0") {
        Ok(socket) => socket,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to reserve port UDP socket: {error}"),
    };
    let host_socket = match std::net::UdpSocket::bind("127.0.0.1:0") {
        Ok(socket) => socket,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to reserve host UDP socket: {error}"),
    };
    let port_addr = port_socket.local_addr().unwrap();
    let host_addr = host_socket.local_addr().unwrap();
    drop(port_socket);
    drop(host_socket);

    let port = match LinkEndpoint::udp_port(port_addr, MacAddr::new([0x02, 0, 0, 0, 0, 1])) {
        Ok(endpoint) => endpoint,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to bind port UDP endpoint: {error}"),
    };
    let host = match LinkEndpoint::udp(host_addr, port_addr, MacAddr::new([0x02, 0, 0, 0, 0, 2])) {
        Ok(endpoint) => endpoint,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to bind host UDP endpoint: {error}"),
    };

    thread::sleep(Duration::from_millis(20));
    assert!(port.recv().is_none());

    host.send(EthernetFrame {
        dst: MacAddr::new([0x02, 0, 0, 0, 0, 1]),
        src: MacAddr::new([0x02, 0, 0, 0, 0, 2]),
        ethertype: EtherType::Ipv4,
        payload: b"hello".to_vec(),
    });

    thread::sleep(Duration::from_millis(20));

    let frame = port.recv().unwrap();
    assert_eq!(frame.payload, b"hello".to_vec());

    port.send(EthernetFrame {
        dst: MacAddr::new([0x02, 0, 0, 0, 0, 2]),
        src: MacAddr::new([0x02, 0, 0, 0, 0, 1]),
        ethertype: EtherType::Ipv4,
        payload: b"reply".to_vec(),
    });

    thread::sleep(Duration::from_millis(20));

    let frame = host.recv().unwrap();
    assert_eq!(frame.payload, b"reply".to_vec());
}
