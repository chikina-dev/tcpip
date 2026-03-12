use std::net::Ipv4Addr;

use tcpip_userland::Host;
use tcpip_userland::internet::ip::Ipv4Packet;
use tcpip_userland::link::SharedMedium;
use tcpip_userland::link::ethernet::MacAddr;

fn build_routed_topology() -> [Host; 6] {
    let lan1 = SharedMedium::new();
    let transit = SharedMedium::new();
    let lan2 = SharedMedium::new();

    let mut left_host = Host::new(
        "left-host",
        Ipv4Addr::new(10, 0, 1, 10),
        MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x01, 0x0a]),
        &lan1,
    );
    left_host.set_default_gateway(Some(Ipv4Addr::new(10, 0, 1, 1)));

    let mut router1_left = Host::new(
        "router1-left",
        Ipv4Addr::new(10, 0, 1, 1),
        MacAddr::new([0x02, 0x00, 0x00, 0x01, 0x01, 0x01]),
        &lan1,
    );
    router1_left.enable_ip_forwarding();

    let mut router1_right = Host::new(
        "router1-right",
        Ipv4Addr::new(10, 0, 12, 1),
        MacAddr::new([0x02, 0x00, 0x00, 0x01, 0x0c, 0x01]),
        &transit,
    );
    router1_right.enable_ip_forwarding();

    let mut router2_left = Host::new(
        "router2-left",
        Ipv4Addr::new(10, 0, 12, 2),
        MacAddr::new([0x02, 0x00, 0x00, 0x02, 0x0c, 0x02]),
        &transit,
    );
    router2_left.enable_ip_forwarding();

    let mut router2_right = Host::new(
        "router2-right",
        Ipv4Addr::new(10, 0, 2, 1),
        MacAddr::new([0x02, 0x00, 0x00, 0x02, 0x02, 0x01]),
        &lan2,
    );
    router2_right.enable_ip_forwarding();

    let mut right_host = Host::new(
        "right-host",
        Ipv4Addr::new(10, 0, 2, 10),
        MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x02, 0x0a]),
        &lan2,
    );
    right_host.set_default_gateway(Some(Ipv4Addr::new(10, 0, 2, 1)));

    [
        left_host,
        router1_left,
        router1_right,
        router2_left,
        router2_right,
        right_host,
    ]
}

fn run_routed_until_idle(hosts: &mut [Host; 6]) {
    loop {
        let mut progressed = false;

        for host in &mut *hosts {
            if host.tick() > 0 {
                progressed = true;
            }
        }

        progressed |= route_packet_from_left_router(hosts);
        progressed |= route_packet_from_right_router(hosts);

        if !progressed {
            break;
        }
    }
}

fn route_packet_from_left_router(hosts: &mut [Host; 6]) -> bool {
    let mut progressed = false;

    while let Some(packet) = hosts[1].recv_forwarded_ipv4() {
        progressed = true;
        route_router1_ingress_left(packet, &mut hosts[2]);
    }

    while let Some(packet) = hosts[2].recv_forwarded_ipv4() {
        progressed = true;
        route_router1_ingress_right(packet, &mut hosts[1]);
    }

    progressed
}

fn route_packet_from_right_router(hosts: &mut [Host; 6]) -> bool {
    let mut progressed = false;

    while let Some(packet) = hosts[3].recv_forwarded_ipv4() {
        progressed = true;
        route_router2_ingress_left(packet, &mut hosts[4]);
    }

    while let Some(packet) = hosts[4].recv_forwarded_ipv4() {
        progressed = true;
        route_router2_ingress_right(packet, &mut hosts[3]);
    }

    progressed
}

fn route_router1_ingress_left(packet: Ipv4Packet, router1_right: &mut Host) {
    if packet.dst.octets()[0..3] == [10, 0, 12] {
        let _ = router1_right.send_ipv4_via(packet, None);
    } else if packet.dst.octets()[0..3] == [10, 0, 2] {
        let _ = router1_right.send_ipv4_via(packet, Some(Ipv4Addr::new(10, 0, 12, 2)));
    }
}

fn route_router1_ingress_right(packet: Ipv4Packet, router1_left: &mut Host) {
    if packet.dst.octets()[0..3] == [10, 0, 1] {
        let _ = router1_left.send_ipv4_via(packet, None);
    }
}

fn route_router2_ingress_left(packet: Ipv4Packet, router2_right: &mut Host) {
    if packet.dst.octets()[0..3] == [10, 0, 2] {
        let _ = router2_right.send_ipv4_via(packet, None);
    }
}

fn route_router2_ingress_right(packet: Ipv4Packet, router2_left: &mut Host) {
    if packet.dst.octets()[0..3] == [10, 0, 1] {
        let _ = router2_left.send_ipv4_via(packet, Some(Ipv4Addr::new(10, 0, 12, 1)));
    }
}

#[test]
fn udp_crosses_two_routers() {
    let mut hosts = build_routed_topology();
    hosts[5].open_udp(7000);

    hosts[0].send_udp(6000, hosts[5].ip(), 7000, b"hello-through-l3".to_vec());
    run_routed_until_idle(&mut hosts);

    let datagram = hosts[5].recv_udp(7000).expect("udp datagram");
    assert_eq!(*datagram.peer.ip(), hosts[0].ip());
    assert_eq!(datagram.payload, b"hello-through-l3".to_vec());
}

#[test]
fn icmp_crosses_two_routers() {
    let mut hosts = build_routed_topology();

    let _ = hosts[0].ping(hosts[5].ip(), b"ping-over-routers".to_vec());
    run_routed_until_idle(&mut hosts);

    let reply = hosts[0].recv_ping_reply().expect("icmp echo reply");
    assert_eq!(reply.source, hosts[5].ip());
    assert_eq!(reply.payload, b"ping-over-routers".to_vec());
}
