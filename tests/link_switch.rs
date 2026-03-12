use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tcpip_userland::link::ethernet::{EtherType, EthernetFrame, MacAddr};
use tcpip_userland::link::switch::LearningSwitch;

fn socket(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn frame(dst: MacAddr, src: MacAddr) -> EthernetFrame {
    EthernetFrame {
        dst,
        src,
        ethertype: EtherType::Ipv4,
        payload: b"frame".to_vec(),
    }
}

#[test]
fn floods_unknown_unicast_to_all_other_ports() {
    let mut switch = LearningSwitch::default();
    switch.register_port(MacAddr::new([0x02, 0, 0, 0, 0, 1]), socket(5001));
    switch.register_port(MacAddr::new([0x02, 0, 0, 0, 0, 2]), socket(5002));
    switch.register_port(MacAddr::new([0x02, 0, 0, 0, 0, 3]), socket(5003));

    let outputs = switch.forward(
        socket(5001),
        &frame(
            MacAddr::new([0x02, 0, 0, 0, 0, 9]),
            MacAddr::new([0x02, 0, 0, 0, 0, 1]),
        ),
    );

    assert!(outputs.contains(&socket(5002)));
    assert!(outputs.contains(&socket(5003)));
    assert!(!outputs.contains(&socket(5001)));
}

#[test]
fn forwards_known_unicast_only_to_learned_port() {
    let mut switch = LearningSwitch::default();
    switch.register_port(MacAddr::new([0x02, 0, 0, 0, 0, 1]), socket(5001));
    switch.register_port(MacAddr::new([0x02, 0, 0, 0, 0, 2]), socket(5002));

    let outputs = switch.forward(
        socket(5001),
        &frame(
            MacAddr::new([0x02, 0, 0, 0, 0, 2]),
            MacAddr::new([0x02, 0, 0, 0, 0, 1]),
        ),
    );

    assert_eq!(outputs, vec![socket(5002)]);
}
