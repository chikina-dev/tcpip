use std::io::{self, BufRead};
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tcpip_userland::Host;
use tcpip_userland::application::dhcp::DHCP_SERVER_PORT;
use tcpip_userland::application::dhcp_server::DhcpServer;
use tcpip_userland::link::LinkEndpoint;
use tcpip_userland::link::ethernet::MacAddr;

use crate::command::common::{clear_router_uplink, print_leases, print_usage, store_router_uplink};
use crate::command::wan_config::{
    InterfaceDefinition, InterfaceMode as ConfigInterfaceMode, WanConfig,
};

const DEFAULT_MASK: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 0);
const DEFAULT_PREFIX_LEN: u8 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InterfaceId {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
enum InterfaceLinkMode {
    Listen,
    Uplink(SocketAddr),
}

impl InterfaceLinkMode {
    fn describe(self) -> String {
        match self {
            Self::Listen => "listen".to_string(),
            Self::Uplink(addr) => format!("uplink={addr}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct InterfaceConfig {
    label: &'static str,
    mode: InterfaceLinkMode,
    ip: Ipv4Addr,
    prefix_len: u8,
    mac: MacAddr,
}

impl InterfaceConfig {
    fn network(self) -> Ipv4Addr {
        masked_ip(self.ip, prefix_len_to_mask(self.prefix_len))
    }

    fn contains(self, ip: Ipv4Addr) -> bool {
        masked(ip, self.prefix_len) == masked(self.ip, self.prefix_len)
    }
}

#[derive(Clone, Copy, Debug)]
struct Route {
    network: Ipv4Addr,
    prefix_len: u8,
    next_hop: Option<Ipv4Addr>,
}

impl Route {
    fn matches(self, ip: Ipv4Addr) -> bool {
        masked(ip, self.prefix_len) == masked(self.network, self.prefix_len)
    }
}

pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() == 1 {
        let config = WanConfig::load()?;
        let router_id = MacAddr::from_str(&args[0]).map_err(io::Error::other)?;
        let router = config.find_router(router_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("router {router_id} not found in wan.toml"),
            )
        })?;
        let mut expanded = vec![
            router.if0.bind.to_string(),
            interface_mode_text(router.if0),
            router.if0.ip.to_string(),
            router.if0.mac.to_string(),
            router.if1.bind.to_string(),
            interface_mode_text(router.if1),
            router.if1.ip.to_string(),
            router.if1.mac.to_string(),
        ];
        for route in &router.routes {
            expanded.push(route.cidr.clone());
            expanded.push(route.next_hop.clone());
        }
        return run(expanded);
    }

    if args.len() < 8 || (args.len() - 8) % 2 != 0 {
        print_usage();
        return Ok(());
    }

    let left_bind = SocketAddr::from_str(&args[0])?;
    let left_mode = parse_link_mode(&args[1])?;
    let left_ip = Ipv4Addr::from_str(&args[2])?;
    let left_mac = MacAddr::from_str(&args[3]).map_err(io::Error::other)?;

    let right_bind = SocketAddr::from_str(&args[4])?;
    let right_mode = parse_link_mode(&args[5])?;
    let right_ip = Ipv4Addr::from_str(&args[6])?;
    let right_mac = MacAddr::from_str(&args[7]).map_err(io::Error::other)?;

    let left_interface = InterfaceConfig {
        label: "if0",
        mode: left_mode,
        ip: left_ip,
        prefix_len: DEFAULT_PREFIX_LEN,
        mac: left_mac,
    };
    let right_interface = InterfaceConfig {
        label: "if1",
        mode: right_mode,
        ip: right_ip,
        prefix_len: DEFAULT_PREFIX_LEN,
        mac: right_mac,
    };

    let mut routes = vec![
        Route {
            network: left_interface.network(),
            prefix_len: left_interface.prefix_len,
            next_hop: None,
        },
        Route {
            network: right_interface.network(),
            prefix_len: right_interface.prefix_len,
            next_hop: None,
        },
    ];

    for chunk in args[8..].chunks(2) {
        let (network, prefix_len) = parse_cidr(&chunk[0])?;
        routes.push(Route {
            network,
            prefix_len,
            next_hop: if chunk[1] == "direct" {
                None
            } else {
                Some(Ipv4Addr::from_str(&chunk[1])?)
            },
        });
    }

    let left_link = build_link(left_bind, left_mode, left_mac)?;
    let right_link = build_link(right_bind, right_mode, right_mac)?;
    let left_actual_bind = left_link.local_addr()?.expect("udp addr");
    let right_actual_bind = right_link.local_addr()?.expect("udp addr");

    let mut left = Host::new_with_link("router-left", left_ip, left_mac, left_link);
    left.set_subnet_mask(DEFAULT_MASK);
    left.enable_ip_forwarding();

    let mut right = Host::new_with_link("router-right", right_ip, right_mac, right_link);
    right.set_subnet_mask(DEFAULT_MASK);
    right.enable_ip_forwarding();

    let (sender, receiver) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    println!("router started");
    print_interface(left_interface, left_actual_bind);
    print_interface(right_interface, right_actual_bind);
    println!("commands: show routes, show ifaces, show leases, /quit");

    let mut left_dhcp = dhcp_server_for_interface(left_interface)?;
    let mut right_dhcp = dhcp_server_for_interface(right_interface)?;
    if left_dhcp.is_some() {
        left.open_udp(DHCP_SERVER_PORT);
        store_router_uplink(left_interface.ip, left_actual_bind)?;
    }
    if right_dhcp.is_some() {
        right.open_udp(DHCP_SERVER_PORT);
        store_router_uplink(right_interface.ip, right_actual_bind)?;
    }

    loop {
        if let Some(server) = left_dhcp.as_mut() {
            for event in server.expire() {
                println!("{} {}", left_interface.label, event.describe());
            }
        }
        if let Some(server) = right_dhcp.as_mut() {
            for event in server.expire() {
                println!("{} {}", right_interface.label, event.describe());
            }
        }

        match receiver.try_recv() {
            Ok(line) if line == "/quit" => break,
            Ok(line) if line == "show routes" => print_routes(&routes),
            Ok(line) if line == "show ifaces" => {
                print_interface(left_interface, left_actual_bind);
                print_interface(right_interface, right_actual_bind);
            }
            Ok(line) if line == "show leases" => {
                println!("{}:", left_interface.label);
                if let Some(server) = left_dhcp.as_ref() {
                    print_leases(&server.leases());
                } else {
                    println!("leases: disabled");
                }
                println!("{}:", right_interface.label);
                if let Some(server) = right_dhcp.as_ref() {
                    print_leases(&server.leases());
                } else {
                    println!("leases: disabled");
                }
            }
            Ok(line) if !line.is_empty() => eprintln!("unknown router command: {line}"),
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => break,
        }

        let mut progressed = false;
        progressed |= left.tick() > 0;
        progressed |= right.tick() > 0;
        if let Some(server) = left_dhcp.as_mut() {
            while let Some(datagram) = left.recv_udp(DHCP_SERVER_PORT) {
                if let Some(event) = server.handle_datagram(&mut left, &datagram.payload) {
                    println!("{} {}", left_interface.label, event.describe());
                }
            }
        }
        if let Some(server) = right_dhcp.as_mut() {
            while let Some(datagram) = right.recv_udp(DHCP_SERVER_PORT) {
                if let Some(event) = server.handle_datagram(&mut right, &datagram.payload) {
                    println!("{} {}", right_interface.label, event.describe());
                }
            }
        }
        progressed |= forward_pending(
            InterfaceId::Left,
            &routes,
            left_interface,
            right_interface,
            &mut left,
            &mut right,
        );
        progressed |= forward_pending(
            InterfaceId::Right,
            &routes,
            left_interface,
            right_interface,
            &mut right,
            &mut left,
        );

        if !progressed {
            thread::sleep(Duration::from_millis(10));
        }
    }

    if matches!(left_interface.mode, InterfaceLinkMode::Listen) {
        clear_router_uplink(left_interface.ip, left_actual_bind)?;
    }
    if matches!(right_interface.mode, InterfaceLinkMode::Listen) {
        clear_router_uplink(right_interface.ip, right_actual_bind)?;
    }

    Ok(())
}

fn interface_mode_text(interface: InterfaceDefinition) -> String {
    match interface.mode {
        ConfigInterfaceMode::Listen => "listen".to_string(),
        ConfigInterfaceMode::Uplink => interface
            .uplink
            .expect("uplink mode must have uplink addr")
            .to_string(),
    }
}

fn build_link(
    bind_addr: SocketAddr,
    mode: InterfaceLinkMode,
    mac: MacAddr,
) -> io::Result<LinkEndpoint> {
    match mode {
        InterfaceLinkMode::Listen => LinkEndpoint::udp_port(bind_addr, mac),
        InterfaceLinkMode::Uplink(uplink) => LinkEndpoint::udp(bind_addr, uplink, mac),
    }
}

fn forward_pending(
    ingress: InterfaceId,
    routes: &[Route],
    left_interface: InterfaceConfig,
    right_interface: InterfaceConfig,
    inbound: &mut Host,
    other: &mut Host,
) -> bool {
    let mut progressed = false;

    while let Some(packet) = inbound.recv_forwarded_ipv4() {
        let Some(route) = lookup_route(routes, packet.dst) else {
            continue;
        };
        let Some((egress, next_hop)) =
            resolve_egress(route, packet.dst, left_interface, right_interface)
        else {
            continue;
        };

        if egress == ingress {
            let _ = inbound.send_ipv4_via(packet, next_hop);
        } else {
            let _ = other.send_ipv4_via(packet, next_hop);
        }

        progressed = true;
    }

    progressed
}

fn resolve_egress(
    route: Route,
    dst_ip: Ipv4Addr,
    left_interface: InterfaceConfig,
    right_interface: InterfaceConfig,
) -> Option<(InterfaceId, Option<Ipv4Addr>)> {
    let next_hop = route.next_hop.unwrap_or(dst_ip);
    if left_interface.contains(next_hop) {
        Some((InterfaceId::Left, route.next_hop))
    } else if right_interface.contains(next_hop) {
        Some((InterfaceId::Right, route.next_hop))
    } else {
        None
    }
}

fn lookup_route(routes: &[Route], dst_ip: Ipv4Addr) -> Option<Route> {
    let mut best = None;
    for route in routes.iter().copied() {
        if !route.matches(dst_ip) {
            continue;
        }

        if best
            .as_ref()
            .is_none_or(|current: &Route| route.prefix_len > current.prefix_len)
        {
            best = Some(route);
        }
    }

    best
}

fn print_routes(routes: &[Route]) {
    println!("routes:");
    for route in routes {
        let next_hop = route
            .next_hop
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "direct".to_string());
        println!("  {}/{} via {}", route.network, route.prefix_len, next_hop);
    }
}

fn print_interface(interface: InterfaceConfig, bind_addr: SocketAddr) {
    println!(
        "{}: bind={} {} ip={}/{} mac={}",
        interface.label,
        bind_addr,
        interface.mode.describe(),
        interface.ip,
        interface.prefix_len,
        interface.mac
    );
}

fn dhcp_server_for_interface(interface: InterfaceConfig) -> io::Result<Option<DhcpServer>> {
    if !matches!(interface.mode, InterfaceLinkMode::Listen) {
        return Ok(None);
    }

    let pool_start = Ipv4Addr::from(u32::from(interface.network()) + 10);
    let pool_end = Ipv4Addr::from(u32::from(interface.network()) + 99);
    let server = DhcpServer::new(pool_start, pool_end, interface.ip)?;
    println!(
        "{} dhcp-pool={}..={} gateway={}",
        interface.label, pool_start, pool_end, interface.ip
    );
    Ok(Some(server))
}

fn parse_link_mode(value: &str) -> io::Result<InterfaceLinkMode> {
    if value == "listen" {
        return Ok(InterfaceLinkMode::Listen);
    }

    let addr = SocketAddr::from_str(value).map_err(io::Error::other)?;
    Ok(InterfaceLinkMode::Uplink(addr))
}

fn parse_cidr(value: &str) -> io::Result<(Ipv4Addr, u8)> {
    let (network, prefix_len) = value.split_once('/').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "route must be written as a.b.c.d/prefix",
        )
    })?;
    let prefix_len = prefix_len.parse::<u8>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "route prefix must be an integer",
        )
    })?;
    if prefix_len > 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "route prefix must be between 0 and 32",
        ));
    }
    let network = Ipv4Addr::from_str(network).map_err(io::Error::other)?;
    Ok((
        masked_ip(network, prefix_len_to_mask(prefix_len)),
        prefix_len,
    ))
}

fn masked(ip: Ipv4Addr, prefix_len: u8) -> u32 {
    u32::from(ip) & u32::from(prefix_len_to_mask(prefix_len))
}

fn masked_ip(ip: Ipv4Addr, mask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) & u32::from(mask))
}

fn prefix_len_to_mask(prefix_len: u8) -> Ipv4Addr {
    let mask = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    };
    Ipv4Addr::from(mask)
}
