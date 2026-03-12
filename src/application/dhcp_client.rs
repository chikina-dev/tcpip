use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::thread;
use std::time::{Duration, Instant};

use crate::Host;
use crate::application::dhcp::{BROADCAST_IP, DHCP_CLIENT_PORT, DHCP_SERVER_PORT, DhcpMessage};
use crate::link::LinkEndpoint;
use crate::link::ethernet::MacAddr;

const DHCP_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const DHCP_RENEW_NUMERATOR: u32 = 3;
const DHCP_RENEW_DENOMINATOR: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct AcquiredLease {
    pub actual_bind: SocketAddr,
    pub assigned_ip: Ipv4Addr,
    pub gateway_ip: Ipv4Addr,
    pub lease_duration: Duration,
    obtained_at: Instant,
}

#[derive(Clone, Debug)]
pub struct LeaseManager {
    switch_addr: SocketAddr,
    mac: MacAddr,
    assigned_ip: Option<Ipv4Addr>,
    default_gateway: Option<Ipv4Addr>,
    renew_at: Instant,
    expires_at: Instant,
    retry_at: Instant,
}

impl LeaseManager {
    pub fn bootstrap(
        bind_addr: SocketAddr,
        switch_addr: SocketAddr,
        mac: MacAddr,
    ) -> io::Result<(Ipv4Addr, Self)> {
        let lease = acquire(bind_addr, switch_addr, mac)?;
        let assigned_ip = lease.assigned_ip;
        Ok((assigned_ip, Self::from_acquired(switch_addr, mac, lease)))
    }

    fn from_acquired(switch_addr: SocketAddr, mac: MacAddr, lease: AcquiredLease) -> Self {
        let renew_at = lease.obtained_at
            + lease
                .lease_duration
                .mul_f64(f64::from(DHCP_RENEW_NUMERATOR) / f64::from(DHCP_RENEW_DENOMINATOR));
        let expires_at = lease.obtained_at + lease.lease_duration;
        Self {
            switch_addr,
            mac,
            assigned_ip: Some(lease.assigned_ip),
            default_gateway: Some(lease.gateway_ip),
            renew_at,
            expires_at,
            retry_at: lease.obtained_at,
        }
    }

    pub fn update_switch_addr(&mut self, switch_addr: SocketAddr) {
        self.switch_addr = switch_addr;
    }

    pub fn release_ip(&self) -> Option<Ipv4Addr> {
        self.assigned_ip
    }

    pub fn default_gateway(&self) -> Option<Ipv4Addr> {
        self.default_gateway
    }

    pub fn maintain(&mut self, host: &mut Host) {
        let now = Instant::now();
        if now < self.retry_at {
            return;
        }
        if self.assigned_ip.is_some() && now < self.renew_at {
            return;
        }

        match renew_or_reacquire(self.switch_addr, self.mac, self.assigned_ip) {
            Ok(lease) => {
                let previous_ip = self.assigned_ip;
                let next_ip = lease.assigned_ip;
                *self = Self::from_acquired(self.switch_addr, self.mac, lease);
                if host.ip() != next_ip {
                    host.set_ip(next_ip);
                }
                host.set_default_gateway(self.default_gateway);
                if previous_ip == Some(next_ip) {
                    println!("dhcp renewed local-ip={next_ip}");
                } else {
                    println!("dhcp reacquired local-ip={next_ip}");
                }
            }
            Err(error) => {
                if self.assigned_ip.is_some() && now >= self.expires_at {
                    if let Some(expired_ip) = self.assigned_ip.take() {
                        host.set_ip(Ipv4Addr::UNSPECIFIED);
                        self.default_gateway = None;
                        host.set_default_gateway(None);
                        eprintln!("dhcp lease expired for {expired_ip}: {error}");
                    }
                }
                self.retry_at = now + DHCP_RETRY_INTERVAL;
            }
        }
    }
}

pub fn acquire(
    bind_addr: SocketAddr,
    switch_addr: SocketAddr,
    mac: MacAddr,
) -> io::Result<AcquiredLease> {
    discover(bind_addr, switch_addr, mac)
}

pub fn send_release(
    switch_addr: SocketAddr,
    mac: MacAddr,
    local_ip: Ipv4Addr,
    bind_addr: SocketAddr,
) {
    let Ok(link) = LinkEndpoint::udp(bind_addr, switch_addr, mac) else {
        return;
    };
    let mut host = Host::new_with_link("dhcp-release", local_ip, mac, link);
    host.open_udp(DHCP_CLIENT_PORT);
    host.send_udp(
        DHCP_CLIENT_PORT,
        BROADCAST_IP,
        DHCP_SERVER_PORT,
        DhcpMessage::Release {
            client_mac: mac,
            released_ip: local_ip,
        }
        .encode(),
    );

    for _ in 0..10 {
        let _ = host.tick();
        thread::sleep(Duration::from_millis(10));
    }
}

fn discover(
    bind_addr: SocketAddr,
    switch_addr: SocketAddr,
    mac: MacAddr,
) -> io::Result<AcquiredLease> {
    let link = LinkEndpoint::udp(bind_addr, switch_addr, mac)?;
    let actual_bind = link.local_addr()?.expect("udp addr");
    let mut host = Host::new_with_link("dhcp-client", Ipv4Addr::new(0, 0, 0, 0), mac, link);
    host.open_udp(DHCP_CLIENT_PORT);
    host.send_udp(
        DHCP_CLIENT_PORT,
        BROADCAST_IP,
        DHCP_SERVER_PORT,
        DhcpMessage::Discover { client_mac: mac }.encode(),
    );

    let started = Instant::now();
    let mut requested = None;

    loop {
        let _ = host.tick();

        while let Some(datagram) = host.recv_udp(DHCP_CLIENT_PORT) {
            let Some(message) = DhcpMessage::decode(&datagram.payload) else {
                continue;
            };

            match message {
                DhcpMessage::Offer {
                    client_mac,
                    offered_ip,
                    lease_seconds: _,
                    gateway_ip: _,
                } if client_mac == mac => {
                    requested = Some(offered_ip);
                    host.send_udp(
                        DHCP_CLIENT_PORT,
                        BROADCAST_IP,
                        DHCP_SERVER_PORT,
                        DhcpMessage::Request {
                            client_mac: mac,
                            requested_ip: offered_ip,
                        }
                        .encode(),
                    );
                }
                DhcpMessage::Ack {
                    client_mac,
                    assigned_ip,
                    lease_seconds,
                    gateway_ip,
                } if client_mac == mac => {
                    host.set_ip(assigned_ip);
                    host.set_default_gateway(Some(gateway_ip));
                    return Ok(AcquiredLease {
                        actual_bind,
                        assigned_ip,
                        gateway_ip,
                        lease_duration: Duration::from_secs(lease_seconds),
                        obtained_at: Instant::now(),
                    });
                }
                _ => {}
            }
        }

        if started.elapsed() > Duration::from_secs(5) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "DHCP lease timed out",
            ));
        }

        if requested.is_none() && started.elapsed() > Duration::from_secs(2) {
            host.send_udp(
                DHCP_CLIENT_PORT,
                BROADCAST_IP,
                DHCP_SERVER_PORT,
                DhcpMessage::Discover { client_mac: mac }.encode(),
            );
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn request_renewal(
    bind_addr: SocketAddr,
    switch_addr: SocketAddr,
    mac: MacAddr,
    requested_ip: Ipv4Addr,
) -> io::Result<AcquiredLease> {
    let link = LinkEndpoint::udp(bind_addr, switch_addr, mac)?;
    let actual_bind = link.local_addr()?.expect("udp addr");
    let mut host = Host::new_with_link("dhcp-renew", Ipv4Addr::new(0, 0, 0, 0), mac, link);
    host.open_udp(DHCP_CLIENT_PORT);
    host.send_udp(
        DHCP_CLIENT_PORT,
        BROADCAST_IP,
        DHCP_SERVER_PORT,
        DhcpMessage::Request {
            client_mac: mac,
            requested_ip,
        }
        .encode(),
    );

    let started = Instant::now();
    loop {
        let _ = host.tick();

        while let Some(datagram) = host.recv_udp(DHCP_CLIENT_PORT) {
            let Some(message) = DhcpMessage::decode(&datagram.payload) else {
                continue;
            };

            if let DhcpMessage::Ack {
                client_mac,
                assigned_ip,
                lease_seconds,
                gateway_ip,
            } = message
            {
                if client_mac == mac {
                    host.set_ip(assigned_ip);
                    host.set_default_gateway(Some(gateway_ip));
                    return Ok(AcquiredLease {
                        actual_bind,
                        assigned_ip,
                        gateway_ip,
                        lease_duration: Duration::from_secs(lease_seconds),
                        obtained_at: Instant::now(),
                    });
                }
            }
        }

        if started.elapsed() > Duration::from_secs(2) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "DHCP renewal timed out",
            ));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn renew_or_reacquire(
    switch_addr: SocketAddr,
    mac: MacAddr,
    current_ip: Option<Ipv4Addr>,
) -> io::Result<AcquiredLease> {
    if let Some(current_ip) = current_ip {
        if let Ok(lease) = request_renewal(bind_addr(), switch_addr, mac, current_ip) {
            return Ok(lease);
        }
    }

    discover(bind_addr(), switch_addr, mac)
}

fn bind_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}
