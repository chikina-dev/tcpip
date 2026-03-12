use std::collections::HashMap;
use std::io;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use crate::Host;
use crate::application::dhcp::{
    BROADCAST_IP, DEFAULT_LEASE_SECONDS, DHCP_CLIENT_PORT, DHCP_SERVER_PORT, DhcpMessage,
};
use crate::link::ethernet::MacAddr;

const DHCP_OFFER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
struct LeaseRecord {
    ip: Ipv4Addr,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug)]
struct PendingOffer {
    ip: Ipv4Addr,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct LeaseEntry {
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub remaining: Duration,
}

#[derive(Clone, Copy, Debug)]
pub enum DhcpServerEvent {
    Offered { mac: MacAddr, ip: Ipv4Addr },
    Leased { mac: MacAddr, ip: Ipv4Addr },
    Released { mac: MacAddr, ip: Ipv4Addr },
    Expired { mac: MacAddr, ip: Ipv4Addr },
}

impl DhcpServerEvent {
    pub fn describe(self) -> String {
        match self {
            Self::Offered { mac, ip } => format!("offered {ip} to {mac}"),
            Self::Leased { mac, ip } => format!("leased {ip} to {mac}"),
            Self::Released { mac, ip } => format!("released {ip} from {mac}"),
            Self::Expired { mac, ip } => format!("expired {ip} from {mac}"),
        }
    }
}

pub struct DhcpServer {
    pool: Vec<Ipv4Addr>,
    gateway_ip: Ipv4Addr,
    leases: HashMap<MacAddr, LeaseRecord>,
    pending_offers: HashMap<MacAddr, PendingOffer>,
}

impl DhcpServer {
    pub fn new(pool_start: Ipv4Addr, pool_end: Ipv4Addr, gateway_ip: Ipv4Addr) -> io::Result<Self> {
        Ok(Self {
            pool: ip_range(pool_start, pool_end)?,
            gateway_ip,
            leases: HashMap::new(),
            pending_offers: HashMap::new(),
        })
    }

    pub fn lease_seconds(&self) -> u64 {
        DEFAULT_LEASE_SECONDS
    }

    pub fn expire(&mut self) -> Vec<DhcpServerEvent> {
        let now = Instant::now();
        let expired_leases: Vec<_> = self
            .leases
            .iter()
            .filter(|(_, lease)| lease.expires_at <= now)
            .map(|(mac, lease)| DhcpServerEvent::Expired {
                mac: *mac,
                ip: lease.ip,
            })
            .collect();
        for event in &expired_leases {
            if let DhcpServerEvent::Expired { mac, .. } = event {
                self.leases.remove(mac);
            }
        }
        self.pending_offers
            .retain(|_, offer| offer.expires_at > now);
        expired_leases
    }

    pub fn handle_datagram(&mut self, host: &mut Host, payload: &[u8]) -> Option<DhcpServerEvent> {
        let message = DhcpMessage::decode(payload)?;
        match message {
            DhcpMessage::Discover { client_mac } => {
                let offered_ip = self.pick_lease(client_mac)?;
                self.pending_offers.insert(
                    client_mac,
                    PendingOffer {
                        ip: offered_ip,
                        expires_at: Instant::now() + DHCP_OFFER_TIMEOUT,
                    },
                );
                host.send_udp(
                    DHCP_SERVER_PORT,
                    BROADCAST_IP,
                    DHCP_CLIENT_PORT,
                    DhcpMessage::Offer {
                        client_mac,
                        offered_ip,
                        lease_seconds: self.lease_seconds(),
                        gateway_ip: self.gateway_ip,
                    }
                    .encode(),
                );
                Some(DhcpServerEvent::Offered {
                    mac: client_mac,
                    ip: offered_ip,
                })
            }
            DhcpMessage::Request {
                client_mac,
                requested_ip,
            } => {
                let reserved_ip = self
                    .pending_offers
                    .get(&client_mac)
                    .map(|offer| offer.ip)
                    .or_else(|| self.leases.get(&client_mac).map(|lease| lease.ip));
                if reserved_ip != Some(requested_ip) {
                    return None;
                }

                self.leases.insert(
                    client_mac,
                    LeaseRecord {
                        ip: requested_ip,
                        expires_at: Instant::now() + Duration::from_secs(self.lease_seconds()),
                    },
                );
                self.pending_offers.remove(&client_mac);
                host.send_udp(
                    DHCP_SERVER_PORT,
                    BROADCAST_IP,
                    DHCP_CLIENT_PORT,
                    DhcpMessage::Ack {
                        client_mac,
                        assigned_ip: requested_ip,
                        lease_seconds: self.lease_seconds(),
                        gateway_ip: self.gateway_ip,
                    }
                    .encode(),
                );
                Some(DhcpServerEvent::Leased {
                    mac: client_mac,
                    ip: requested_ip,
                })
            }
            DhcpMessage::Release {
                client_mac,
                released_ip,
            } => {
                if self.leases.get(&client_mac).map(|lease| lease.ip) == Some(released_ip) {
                    self.leases.remove(&client_mac);
                    self.pending_offers.remove(&client_mac);
                    return Some(DhcpServerEvent::Released {
                        mac: client_mac,
                        ip: released_ip,
                    });
                }
                None
            }
            DhcpMessage::Offer { .. } | DhcpMessage::Ack { .. } => None,
        }
    }

    pub fn leases(&self) -> Vec<LeaseEntry> {
        let now = Instant::now();
        let mut entries: Vec<_> = self
            .leases
            .iter()
            .map(|(mac, lease)| LeaseEntry {
                mac: *mac,
                ip: lease.ip,
                remaining: lease.expires_at.saturating_duration_since(now),
            })
            .collect();
        entries.sort_by_key(|entry| (entry.mac.octets(), entry.ip.octets()));
        entries
    }

    fn pick_lease(&self, client_mac: MacAddr) -> Option<Ipv4Addr> {
        if let Some(current) = self.leases.get(&client_mac).map(|lease| lease.ip) {
            return Some(current);
        }
        if let Some(current) = self.pending_offers.get(&client_mac).map(|offer| offer.ip) {
            return Some(current);
        }

        self.pool.iter().copied().find(|candidate| {
            !self.leases.values().any(|lease| lease.ip == *candidate)
                && !self
                    .pending_offers
                    .values()
                    .any(|offer| offer.ip == *candidate)
        })
    }
}

fn ip_range(start: Ipv4Addr, end: Ipv4Addr) -> io::Result<Vec<Ipv4Addr>> {
    let start = u32::from(start);
    let end = u32::from(end);
    if start > end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pool start must be <= pool end",
        ));
    }

    Ok((start..=end).map(Ipv4Addr::from).collect())
}
