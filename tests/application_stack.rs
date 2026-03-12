use std::net::Ipv4Addr;

use tcpip_userland::link::SharedMedium;
use tcpip_userland::link::ethernet::MacAddr;
use tcpip_userland::{Host, run_until_idle};

fn build_pair() -> [Host; 2] {
    let medium = SharedMedium::new();
    [
        Host::new(
            "alpha",
            Ipv4Addr::new(10, 0, 0, 1),
            MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            &medium,
        ),
        Host::new(
            "beta",
            Ipv4Addr::new(10, 0, 0, 2),
            MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
            &medium,
        ),
    ]
}

#[test]
fn udp_is_dispatched_per_port() {
    let mut hosts = build_pair();
    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];
        host_b.open_udp(7000);
        host_b.open_udp(7001);
        host_a.send_udp(50000, host_b.ip(), 7000, b"first".to_vec());
        host_a.send_udp(50001, host_b.ip(), 7001, b"second".to_vec());
    }
    run_until_idle(&mut hosts);

    let (left, right) = hosts.split_at_mut(1);
    let host_b = &mut right[0];
    assert_eq!(host_b.recv_udp(7000).unwrap().payload, b"first".to_vec());
    assert_eq!(host_b.recv_udp(7001).unwrap().payload, b"second".to_vec());
    assert!(left[0].recv_ping_reply().is_none());
}

#[test]
fn icmp_echo_round_trip_works() {
    let mut hosts = build_pair();
    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];
        let _ = host_a.ping(host_b.ip(), b"hello".to_vec());
    }
    run_until_idle(&mut hosts);
    let reply = hosts[0].recv_ping_reply().unwrap();
    assert_eq!(reply.source, Ipv4Addr::new(10, 0, 0, 2));
    assert_eq!(reply.payload, b"hello".to_vec());
}

#[test]
fn tcp_listener_can_accept_multiple_connections() {
    let mut hosts = build_pair();
    let client_a;
    let client_b;
    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];
        host_b.listen_tcp(9000);
        client_a = host_a.connect_tcp_from(41000, host_b.ip(), 9000);
        client_b = host_a.connect_tcp_from(41001, host_b.ip(), 9000);
    }
    run_until_idle(&mut hosts);

    let (server_a, server_b);
    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];
        assert!(host_a.is_tcp_established(client_a));
        assert!(host_a.is_tcp_established(client_b));
        server_a = host_b.accept_tcp(9000).unwrap();
        server_b = host_b.accept_tcp(9000).unwrap();
        assert!(host_a.send_tcp(client_a, b"one".to_vec()));
        assert!(host_a.send_tcp(client_b, b"two".to_vec()));
    }
    run_until_idle(&mut hosts);

    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];
        assert_eq!(host_b.recv_tcp(server_a).unwrap(), b"one".to_vec());
        assert_eq!(host_b.recv_tcp(server_b).unwrap(), b"two".to_vec());
        assert!(host_b.send_tcp(server_a, b"ack-one".to_vec()));
        assert!(host_b.send_tcp(server_b, b"ack-two".to_vec()));
        assert!(host_a.recv_tcp(client_a).is_none());
        assert!(host_a.recv_tcp(client_b).is_none());
    }
    run_until_idle(&mut hosts);

    let (left, _) = hosts.split_at_mut(1);
    let host_a = &mut left[0];
    assert_eq!(host_a.recv_tcp(client_a).unwrap(), b"ack-one".to_vec());
    assert_eq!(host_a.recv_tcp(client_b).unwrap(), b"ack-two".to_vec());
}

#[test]
fn udp_broadcast_reaches_other_host() {
    let medium = SharedMedium::new();
    let mut hosts = [
        Host::new(
            "alpha",
            Ipv4Addr::new(0, 0, 0, 0),
            MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            &medium,
        ),
        Host::new(
            "beta",
            Ipv4Addr::new(10, 0, 0, 2),
            MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
            &medium,
        ),
    ];

    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];
        host_b.open_udp(67);
        host_a.send_udp(
            68,
            Ipv4Addr::new(255, 255, 255, 255),
            67,
            b"discover".to_vec(),
        );
    }
    run_until_idle(&mut hosts);

    let (_, right) = hosts.split_at_mut(1);
    let host_b = &mut right[0];
    assert_eq!(host_b.recv_udp(67).unwrap().payload, b"discover".to_vec());
}
