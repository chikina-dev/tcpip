use std::net::Ipv4Addr;

use crate::link::ethernet::MacAddr;

pub const DHCP_SERVER_PORT: u16 = 67;
pub const DHCP_CLIENT_PORT: u16 = 68;
pub const BROADCAST_IP: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);
pub const DEFAULT_LEASE_SECONDS: u64 = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DhcpMessage {
    Discover {
        client_mac: MacAddr,
    },
    Offer {
        client_mac: MacAddr,
        offered_ip: Ipv4Addr,
        lease_seconds: u64,
        gateway_ip: Ipv4Addr,
    },
    Request {
        client_mac: MacAddr,
        requested_ip: Ipv4Addr,
    },
    Ack {
        client_mac: MacAddr,
        assigned_ip: Ipv4Addr,
        lease_seconds: u64,
        gateway_ip: Ipv4Addr,
    },
    Release {
        client_mac: MacAddr,
        released_ip: Ipv4Addr,
    },
}

impl DhcpMessage {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Discover { client_mac } => format!("DISCOVER {client_mac}\n").into_bytes(),
            Self::Offer {
                client_mac,
                offered_ip,
                lease_seconds,
                gateway_ip,
            } => format!("OFFER {client_mac} {offered_ip} {lease_seconds} {gateway_ip}\n")
                .into_bytes(),
            Self::Request {
                client_mac,
                requested_ip,
            } => format!("REQUEST {client_mac} {requested_ip}\n").into_bytes(),
            Self::Ack {
                client_mac,
                assigned_ip,
                lease_seconds,
                gateway_ip,
            } => format!("ACK {client_mac} {assigned_ip} {lease_seconds} {gateway_ip}\n")
                .into_bytes(),
            Self::Release {
                client_mac,
                released_ip,
            } => format!("RELEASE {client_mac} {released_ip}\n").into_bytes(),
        }
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?.trim();
        let mut parts = text.split_whitespace();
        let kind = parts.next()?;
        let mac = parts.next()?.parse::<MacAddr>().ok()?;

        match kind {
            "DISCOVER" => Some(Self::Discover { client_mac: mac }),
            "OFFER" => Some(Self::Offer {
                client_mac: mac,
                offered_ip: parts.next()?.parse().ok()?,
                lease_seconds: parts.next()?.parse().ok()?,
                gateway_ip: parts.next()?.parse().ok()?,
            }),
            "REQUEST" => Some(Self::Request {
                client_mac: mac,
                requested_ip: parts.next()?.parse().ok()?,
            }),
            "ACK" => Some(Self::Ack {
                client_mac: mac,
                assigned_ip: parts.next()?.parse().ok()?,
                lease_seconds: parts.next()?.parse().ok()?,
                gateway_ip: parts.next()?.parse().ok()?,
            }),
            "RELEASE" => Some(Self::Release {
                client_mac: mac,
                released_ip: parts.next()?.parse().ok()?,
            }),
            _ => None,
        }
    }
}
