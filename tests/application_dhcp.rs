use std::net::Ipv4Addr;

use tcpip_userland::application::dhcp::{DEFAULT_LEASE_SECONDS, DhcpMessage};
use tcpip_userland::link::ethernet::MacAddr;

#[test]
fn dhcp_messages_round_trip() {
    let mac = MacAddr::new([0x02, 0, 0, 0, 0, 1]);
    let ip = Ipv4Addr::new(10, 0, 0, 10);
    let messages = [
        DhcpMessage::Discover { client_mac: mac },
        DhcpMessage::Offer {
            client_mac: mac,
            offered_ip: ip,
            lease_seconds: DEFAULT_LEASE_SECONDS,
            gateway_ip: Ipv4Addr::new(10, 0, 0, 1),
        },
        DhcpMessage::Request {
            client_mac: mac,
            requested_ip: ip,
        },
        DhcpMessage::Ack {
            client_mac: mac,
            assigned_ip: ip,
            lease_seconds: DEFAULT_LEASE_SECONDS,
            gateway_ip: Ipv4Addr::new(10, 0, 0, 1),
        },
        DhcpMessage::Release {
            client_mac: mac,
            released_ip: ip,
        },
    ];

    for message in messages {
        assert_eq!(DhcpMessage::decode(&message.encode()), Some(message));
    }
}
