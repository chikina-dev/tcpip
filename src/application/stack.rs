use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};

use crate::internet::icmp::{IcmpKind, IcmpPacket};
use crate::internet::ip::{IpProtocol, Ipv4Packet};
use crate::link::arp::{ArpOperation, ArpPacket};
use crate::link::ethernet::{EtherType, EthernetFrame, MacAddr};
use crate::link::{LinkEndpoint, SharedMedium};
use crate::transport::tcp::{FLAG_ACK, FLAG_FIN, FLAG_SYN, TcpPacket};
use crate::transport::udp::UdpPacket;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UdpDatagram {
    pub peer: SocketAddrV4,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcmpEchoReply {
    pub source: Ipv4Addr,
    pub identifier: u16,
    pub sequence: u16,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionKey {
    pub local_ip: Ipv4Addr,
    pub local_port: u16,
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TcpState {
    SynSent,
    SynReceived,
    Established,
    Closed,
}

#[derive(Clone, Debug)]
struct TcpConnection {
    state: TcpState,
    send_next: u32,
    recv_next: u32,
    receive_queue: VecDeque<Vec<u8>>,
    accepted: bool,
}

pub struct Host {
    name: String,
    ip: Ipv4Addr,
    mac: MacAddr,
    subnet_mask: Ipv4Addr,
    default_gateway: Option<Ipv4Addr>,
    ip_forwarding: bool,
    trace_enabled: bool,
    link: LinkEndpoint,
    arp_cache: HashMap<Ipv4Addr, MacAddr>,
    pending_ip: HashMap<Ipv4Addr, Vec<Ipv4Packet>>,
    pending_arp: HashSet<Ipv4Addr>,
    udp_ports: HashMap<u16, VecDeque<UdpDatagram>>,
    tcp_listeners: HashMap<u16, VecDeque<TcpConnectionKey>>,
    tcp_connections: HashMap<TcpConnectionKey, TcpConnection>,
    icmp_replies: VecDeque<IcmpEchoReply>,
    forwarded_ipv4: VecDeque<Ipv4Packet>,
    next_ephemeral_port: u16,
    next_tcp_seq: u32,
    next_ping_id: u16,
    next_ping_seq: u16,
}

impl Host {
    pub fn new_with_link(
        name: impl Into<String>,
        ip: Ipv4Addr,
        mac: MacAddr,
        link: LinkEndpoint,
    ) -> Self {
        Self {
            name: name.into(),
            ip,
            mac,
            subnet_mask: Ipv4Addr::new(255, 255, 255, 0),
            default_gateway: None,
            ip_forwarding: false,
            trace_enabled: false,
            link,
            arp_cache: HashMap::new(),
            pending_ip: HashMap::new(),
            pending_arp: HashSet::new(),
            udp_ports: HashMap::new(),
            tcp_listeners: HashMap::new(),
            tcp_connections: HashMap::new(),
            icmp_replies: VecDeque::new(),
            forwarded_ipv4: VecDeque::new(),
            next_ephemeral_port: 40_000,
            next_tcp_seq: 1_000,
            next_ping_id: 1,
            next_ping_seq: 1,
        }
    }

    pub fn new(name: impl Into<String>, ip: Ipv4Addr, mac: MacAddr, medium: &SharedMedium) -> Self {
        Self::new_with_link(name, ip, mac, medium.connect(mac))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }

    pub fn set_ip(&mut self, ip: Ipv4Addr) {
        self.ip = ip;
    }

    pub fn set_subnet_mask(&mut self, subnet_mask: Ipv4Addr) {
        self.subnet_mask = subnet_mask;
    }

    pub fn set_default_gateway(&mut self, default_gateway: Option<Ipv4Addr>) {
        self.default_gateway = default_gateway;
    }

    pub fn enable_ip_forwarding(&mut self) {
        self.ip_forwarding = true;
    }

    pub fn mac(&self) -> MacAddr {
        self.mac
    }

    pub fn enable_trace(&mut self) {
        self.trace_enabled = true;
    }

    pub fn open_udp(&mut self, port: u16) {
        self.udp_ports.entry(port).or_default();
    }

    pub fn send_udp(
        &mut self,
        src_port: u16,
        dst_ip: Ipv4Addr,
        dst_port: u16,
        payload: impl Into<Vec<u8>>,
    ) {
        let udp = UdpPacket {
            src_port,
            dst_port,
            payload: payload.into(),
        };
        self.trace_transport(
            "udp",
            "send",
            self.ip,
            src_port,
            dst_ip,
            dst_port,
            &format!("payload={}", udp.payload.len()),
        );
        self.send_ip_packet(Ipv4Packet {
            src: self.ip,
            dst: dst_ip,
            protocol: IpProtocol::Udp,
            ttl: 64,
            payload: udp.encode(),
        });
    }

    pub fn recv_udp(&mut self, port: u16) -> Option<UdpDatagram> {
        self.udp_ports.get_mut(&port)?.pop_front()
    }

    pub fn listen_tcp(&mut self, port: u16) {
        self.tcp_listeners.entry(port).or_default();
    }

    pub fn connect_tcp(&mut self, remote_ip: Ipv4Addr, remote_port: u16) -> TcpConnectionKey {
        let local_port = self.alloc_ephemeral_port();
        self.connect_tcp_from(local_port, remote_ip, remote_port)
    }

    pub fn connect_tcp_from(
        &mut self,
        local_port: u16,
        remote_ip: Ipv4Addr,
        remote_port: u16,
    ) -> TcpConnectionKey {
        let initial_seq = self.alloc_tcp_seq();
        let key = TcpConnectionKey {
            local_ip: self.ip,
            local_port,
            remote_ip,
            remote_port,
        };
        self.tcp_connections.insert(
            key,
            TcpConnection {
                state: TcpState::SynSent,
                send_next: initial_seq + 1,
                recv_next: 0,
                receive_queue: VecDeque::new(),
                accepted: true,
            },
        );
        self.send_tcp_packet(
            key,
            TcpPacket {
                src_port: local_port,
                dst_port: remote_port,
                seq: initial_seq,
                ack: 0,
                flags: FLAG_SYN,
                window: 4096,
                payload: Vec::new(),
            },
        );
        key
    }

    pub fn accept_tcp(&mut self, port: u16) -> Option<TcpConnectionKey> {
        self.tcp_listeners.get_mut(&port)?.pop_front()
    }

    pub fn send_tcp(&mut self, key: TcpConnectionKey, payload: impl Into<Vec<u8>>) -> bool {
        let payload = payload.into();
        let Some(connection) = self.tcp_connections.get_mut(&key) else {
            return false;
        };
        if connection.state != TcpState::Established {
            return false;
        }

        let seq = connection.send_next;
        connection.send_next += payload.len() as u32;
        let ack = connection.recv_next;
        let packet = TcpPacket {
            src_port: key.local_port,
            dst_port: key.remote_port,
            seq,
            ack,
            flags: FLAG_ACK,
            window: 4096,
            payload,
        };
        self.send_tcp_packet(key, packet);
        true
    }

    pub fn recv_tcp(&mut self, key: TcpConnectionKey) -> Option<Vec<u8>> {
        self.tcp_connections
            .get_mut(&key)?
            .receive_queue
            .pop_front()
    }

    pub fn drop_tcp(&mut self, key: TcpConnectionKey) -> bool {
        self.tcp_connections.remove(&key).is_some()
    }

    pub fn is_tcp_established(&self, key: TcpConnectionKey) -> bool {
        self.tcp_connections
            .get(&key)
            .is_some_and(|connection| connection.state == TcpState::Established)
    }

    pub fn ping(&mut self, dst_ip: Ipv4Addr, payload: impl Into<Vec<u8>>) -> (u16, u16) {
        let identifier = self.next_ping_id;
        let sequence = self.next_ping_seq;
        self.next_ping_id = self.next_ping_id.wrapping_add(1);
        self.next_ping_seq = self.next_ping_seq.wrapping_add(1);

        self.send_ip_packet(Ipv4Packet {
            src: self.ip,
            dst: dst_ip,
            protocol: IpProtocol::Icmp,
            ttl: 64,
            payload: IcmpPacket {
                kind: IcmpKind::EchoRequest,
                identifier,
                sequence,
                payload: payload.into(),
            }
            .encode(),
        });

        self.trace_transport(
            "icmp",
            "send",
            self.ip,
            identifier,
            dst_ip,
            sequence,
            "echo-request",
        );

        (identifier, sequence)
    }

    pub fn recv_ping_reply(&mut self) -> Option<IcmpEchoReply> {
        self.icmp_replies.pop_front()
    }

    pub fn recv_forwarded_ipv4(&mut self) -> Option<Ipv4Packet> {
        self.forwarded_ipv4.pop_front()
    }

    pub fn send_ipv4_via(&mut self, packet: Ipv4Packet, next_hop: Option<Ipv4Addr>) -> bool {
        if packet.ttl == 0 {
            return false;
        }

        self.send_ip_packet_via(packet, next_hop);
        true
    }

    pub fn tick(&mut self) -> usize {
        let mut processed = 0;
        while let Some(frame) = self.link.recv() {
            processed += 1;
            self.handle_frame(frame);
        }
        processed
    }

    fn handle_frame(&mut self, frame: EthernetFrame) {
        match frame.ethertype {
            EtherType::Arp => {
                if let Some(packet) = ArpPacket::decode(&frame.payload) {
                    self.handle_arp(packet);
                }
            }
            EtherType::Ipv4 => {
                if let Some(packet) = Ipv4Packet::decode(&frame.payload) {
                    self.handle_ipv4(packet);
                }
            }
        }
    }

    fn handle_arp(&mut self, packet: ArpPacket) {
        self.trace_arp("recv", &packet);
        self.arp_cache.insert(packet.sender_ip, packet.sender_mac);
        self.pending_arp.remove(&packet.sender_ip);
        self.flush_pending_ip(packet.sender_ip);

        if packet.operation == ArpOperation::Request && packet.target_ip == self.ip {
            let reply = ArpPacket {
                operation: ArpOperation::Reply,
                sender_mac: self.mac,
                sender_ip: self.ip,
                target_mac: packet.sender_mac,
                target_ip: packet.sender_ip,
            };
            self.link.send(EthernetFrame {
                dst: packet.sender_mac,
                src: self.mac,
                ethertype: EtherType::Arp,
                payload: reply.encode(),
            });
            self.trace_arp("send", &reply);
        }
    }

    fn handle_ipv4(&mut self, packet: Ipv4Packet) {
        let is_broadcast = packet.dst == Ipv4Addr::new(255, 255, 255, 255);
        if packet.dst != self.ip && !is_broadcast {
            if self.ip_forwarding && packet.ttl > 1 {
                let mut forwarded = packet;
                forwarded.ttl -= 1;
                self.forwarded_ipv4.push_back(forwarded);
            }
            return;
        }

        self.trace_ip("recv", &packet);

        match packet.protocol {
            IpProtocol::Icmp => {
                if let Some(icmp) = IcmpPacket::decode(&packet.payload) {
                    self.handle_icmp(packet.src, icmp);
                }
            }
            IpProtocol::Udp => {
                if let Some(udp) = UdpPacket::decode(&packet.payload) {
                    self.handle_udp(packet.src, udp);
                }
            }
            IpProtocol::Tcp => {
                if let Some(tcp) = TcpPacket::decode(&packet.payload) {
                    self.handle_tcp(packet.src, tcp);
                }
            }
        }
    }

    fn handle_icmp(&mut self, src_ip: Ipv4Addr, packet: IcmpPacket) {
        match packet.kind {
            IcmpKind::EchoRequest => {
                self.trace_transport(
                    "icmp",
                    "recv",
                    src_ip,
                    packet.identifier,
                    self.ip,
                    packet.sequence,
                    "echo-request",
                );
                self.send_ip_packet(Ipv4Packet {
                    src: self.ip,
                    dst: src_ip,
                    protocol: IpProtocol::Icmp,
                    ttl: 64,
                    payload: IcmpPacket {
                        kind: IcmpKind::EchoReply,
                        identifier: packet.identifier,
                        sequence: packet.sequence,
                        payload: packet.payload,
                    }
                    .encode(),
                });
            }
            IcmpKind::EchoReply => {
                self.trace_transport(
                    "icmp",
                    "recv",
                    src_ip,
                    packet.identifier,
                    self.ip,
                    packet.sequence,
                    "echo-reply",
                );
                self.icmp_replies.push_back(IcmpEchoReply {
                    source: src_ip,
                    identifier: packet.identifier,
                    sequence: packet.sequence,
                    payload: packet.payload,
                });
            }
        }
    }

    fn handle_udp(&mut self, src_ip: Ipv4Addr, packet: UdpPacket) {
        self.trace_transport(
            "udp",
            "recv",
            src_ip,
            packet.src_port,
            self.ip,
            packet.dst_port,
            &format!("payload={}", packet.payload.len()),
        );
        if let Some(queue) = self.udp_ports.get_mut(&packet.dst_port) {
            queue.push_back(UdpDatagram {
                peer: SocketAddrV4::new(src_ip, packet.src_port),
                payload: packet.payload,
            });
        }
    }

    fn handle_tcp(&mut self, src_ip: Ipv4Addr, packet: TcpPacket) {
        trace_tcp(
            self.trace_enabled,
            &self.name,
            "recv",
            self.ip,
            packet.dst_port,
            src_ip,
            packet.src_port,
            packet.flags,
            packet.seq,
            packet.ack,
            packet.payload.len(),
        );

        if packet.flags & FLAG_FIN != 0 {
            let key = TcpConnectionKey {
                local_ip: self.ip,
                local_port: packet.dst_port,
                remote_ip: src_ip,
                remote_port: packet.src_port,
            };
            if let Some(connection) = self.tcp_connections.get_mut(&key) {
                connection.state = TcpState::Closed;
            }
            return;
        }

        let key = TcpConnectionKey {
            local_ip: self.ip,
            local_port: packet.dst_port,
            remote_ip: src_ip,
            remote_port: packet.src_port,
        };

        if packet.flags & FLAG_SYN != 0 && packet.flags & FLAG_ACK == 0 {
            if !self.tcp_listeners.contains_key(&packet.dst_port) {
                return;
            }

            if let Some(connection) = self.tcp_connections.get(&key) {
                if connection.state != TcpState::Closed {
                    return;
                }
            }

            let initial_seq = self.alloc_tcp_seq();
            self.tcp_connections.insert(
                key,
                TcpConnection {
                    state: TcpState::SynReceived,
                    send_next: initial_seq + 1,
                    recv_next: packet.seq + 1,
                    receive_queue: VecDeque::new(),
                    accepted: false,
                },
            );
            self.send_tcp_packet(
                key,
                TcpPacket {
                    src_port: key.local_port,
                    dst_port: key.remote_port,
                    seq: initial_seq,
                    ack: packet.seq + 1,
                    flags: FLAG_SYN | FLAG_ACK,
                    window: 4096,
                    payload: Vec::new(),
                },
            );
            return;
        }

        let mut response = None;
        let mut queue_accept = false;

        {
            let Some(connection) = self.tcp_connections.get_mut(&key) else {
                return;
            };

            if packet.flags & FLAG_SYN != 0
                && packet.flags & FLAG_ACK != 0
                && connection.state == TcpState::SynSent
            {
                if packet.ack != connection.send_next {
                    return;
                }
                connection.recv_next = packet.seq + 1;
                connection.state = TcpState::Established;
                response = Some(TcpPacket {
                    src_port: key.local_port,
                    dst_port: key.remote_port,
                    seq: connection.send_next,
                    ack: connection.recv_next,
                    flags: FLAG_ACK,
                    window: 4096,
                    payload: Vec::new(),
                });
            } else if packet.flags & FLAG_ACK != 0
                && connection.state == TcpState::SynReceived
                && packet.payload.is_empty()
            {
                if packet.ack == connection.send_next {
                    connection.state = TcpState::Established;
                    if !connection.accepted {
                        connection.accepted = true;
                        queue_accept = true;
                    }
                }
            } else if connection.state == TcpState::Established
                && packet.flags & FLAG_ACK != 0
                && !packet.payload.is_empty()
            {
                if packet.seq != connection.recv_next {
                    response = Some(TcpPacket {
                        src_port: key.local_port,
                        dst_port: key.remote_port,
                        seq: connection.send_next,
                        ack: connection.recv_next,
                        flags: FLAG_ACK,
                        window: 4096,
                        payload: Vec::new(),
                    });
                } else {
                    connection.recv_next += packet.payload.len() as u32;
                    connection.receive_queue.push_back(packet.payload);
                    response = Some(TcpPacket {
                        src_port: key.local_port,
                        dst_port: key.remote_port,
                        seq: connection.send_next,
                        ack: connection.recv_next,
                        flags: FLAG_ACK,
                        window: 4096,
                        payload: Vec::new(),
                    });
                }
            }
        }

        if queue_accept {
            if let Some(backlog) = self.tcp_listeners.get_mut(&key.local_port) {
                backlog.push_back(key);
            }
        }

        if let Some(packet) = response {
            self.send_tcp_packet(key, packet);
        }
    }

    fn send_ip_packet(&mut self, packet: Ipv4Packet) {
        self.send_ip_packet_via(packet, None);
    }

    fn send_ip_packet_via(&mut self, packet: Ipv4Packet, next_hop: Option<Ipv4Addr>) {
        self.trace_ip("send", &packet);
        if packet.dst == Ipv4Addr::new(255, 255, 255, 255) {
            self.link.send(EthernetFrame {
                dst: MacAddr::BROADCAST,
                src: self.mac,
                ethertype: EtherType::Ipv4,
                payload: packet.encode(),
            });
            return;
        }

        let next_hop_ip = next_hop.unwrap_or_else(|| self.resolve_next_hop(packet.dst));

        if let Some(dst_mac) = self.arp_cache.get(&next_hop_ip).copied() {
            self.link.send(EthernetFrame {
                dst: dst_mac,
                src: self.mac,
                ethertype: EtherType::Ipv4,
                payload: packet.encode(),
            });
            return;
        }

        self.pending_ip.entry(next_hop_ip).or_default().push(packet);
        if self.pending_arp.insert(next_hop_ip) {
            let request = ArpPacket {
                operation: ArpOperation::Request,
                sender_mac: self.mac,
                sender_ip: self.ip,
                target_mac: MacAddr::new([0; 6]),
                target_ip: next_hop_ip,
            };
            self.link.send(EthernetFrame {
                dst: MacAddr::BROADCAST,
                src: self.mac,
                ethertype: EtherType::Arp,
                payload: request.encode(),
            });
            self.trace_arp("send", &request);
        }
    }

    fn resolve_next_hop(&self, dst_ip: Ipv4Addr) -> Ipv4Addr {
        if self.is_on_link(dst_ip) {
            dst_ip
        } else {
            self.default_gateway.unwrap_or(dst_ip)
        }
    }

    fn is_on_link(&self, ip: Ipv4Addr) -> bool {
        ipv4_masked(self.ip, self.subnet_mask) == ipv4_masked(ip, self.subnet_mask)
    }

    fn flush_pending_ip(&mut self, dst_ip: Ipv4Addr) {
        let Some(dst_mac) = self.arp_cache.get(&dst_ip).copied() else {
            return;
        };
        let Some(packets) = self.pending_ip.remove(&dst_ip) else {
            return;
        };

        for packet in packets {
            self.link.send(EthernetFrame {
                dst: dst_mac,
                src: self.mac,
                ethertype: EtherType::Ipv4,
                payload: packet.encode(),
            });
        }
    }

    fn send_tcp_packet(&mut self, key: TcpConnectionKey, packet: TcpPacket) {
        trace_tcp(
            self.trace_enabled,
            &self.name,
            "send",
            key.local_ip,
            key.local_port,
            key.remote_ip,
            key.remote_port,
            packet.flags,
            packet.seq,
            packet.ack,
            packet.payload.len(),
        );
        self.send_ip_packet(Ipv4Packet {
            src: key.local_ip,
            dst: key.remote_ip,
            protocol: IpProtocol::Tcp,
            ttl: 64,
            payload: packet.encode(),
        });
    }

    fn alloc_ephemeral_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = self.next_ephemeral_port.saturating_add(1);
        port
    }

    fn alloc_tcp_seq(&mut self) -> u32 {
        let seq = self.next_tcp_seq;
        self.next_tcp_seq = self.next_tcp_seq.wrapping_add(1_000);
        seq
    }

    fn trace_ip(&self, direction: &str, packet: &Ipv4Packet) {
        if !self.trace_enabled {
            return;
        }

        eprintln!(
            "[ip {}] {} {} -> {} proto={:?} payload={}",
            self.name,
            direction,
            packet.src,
            packet.dst,
            packet.protocol,
            packet.payload.len()
        );
    }

    fn trace_arp(&self, direction: &str, packet: &ArpPacket) {
        if !self.trace_enabled {
            return;
        }

        eprintln!(
            "[arp {}] {} {:?} {}({}) -> {}({})",
            self.name,
            direction,
            packet.operation,
            packet.sender_ip,
            packet.sender_mac,
            packet.target_ip,
            packet.target_mac
        );
    }

    fn trace_transport(
        &self,
        protocol: &str,
        direction: &str,
        local_ip: Ipv4Addr,
        local_port: u16,
        remote_ip: Ipv4Addr,
        remote_port: u16,
        detail: &str,
    ) {
        if !self.trace_enabled {
            return;
        }

        eprintln!(
            "[{protocol} {}] {direction} {local_ip}:{local_port} -> {remote_ip}:{remote_port} {detail}",
            self.name
        );
    }
}

fn ipv4_masked(ip: Ipv4Addr, mask: Ipv4Addr) -> u32 {
    u32::from(ip) & u32::from(mask)
}

pub fn run_until_idle(hosts: &mut [Host]) {
    loop {
        let mut progressed = false;
        for host in &mut *hosts {
            if host.tick() > 0 {
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
}

fn trace_tcp(
    trace_enabled: bool,
    host_name: &str,
    direction: &str,
    local_ip: Ipv4Addr,
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
    flags: u8,
    seq: u32,
    ack: u32,
    payload_len: usize,
) {
    if !trace_enabled {
        return;
    }

    eprintln!(
        "[tcp {host_name}] {direction} {local_ip}:{local_port} <-> {remote_ip}:{remote_port} flags={flags:#04x} seq={seq} ack={ack} payload={payload_len}"
    );
}
