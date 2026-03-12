use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::thread;
use std::time::{Duration, Instant};

use tcpip_userland::application::dhcp_server::LeaseEntry;
use tcpip_userland::application::http::{HttpRequest, HttpResponse};
use tcpip_userland::link::switch::LearningSwitch;

pub(crate) fn route_request(request: &HttpRequest) -> HttpResponse {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => HttpResponse::ok("hello from tcpip_userland http server\n"),
        ("GET", "/hello") => HttpResponse::ok("hello\n"),
        _ => HttpResponse::not_found("not found\n"),
    }
}

pub(crate) fn switch_state_path() -> PathBuf {
    PathBuf::from(".tcpip_switch_addr")
}

pub(crate) fn router_uplink_state_path(gateway_ip: Ipv4Addr) -> PathBuf {
    PathBuf::from(format!(
        ".tcpip_router_uplink_{}",
        gateway_ip.to_string().replace('.', "_")
    ))
}

pub(crate) fn default_bind_addr() -> SocketAddr {
    SocketAddr::from_str("127.0.0.1:0").expect("valid default bind addr")
}

pub(crate) fn load_switch_addr() -> io::Result<SocketAddr> {
    let path = switch_state_path();
    let started = Instant::now();
    loop {
        match fs::read_to_string(&path) {
            Ok(text) => {
                return SocketAddr::from_str(text.trim()).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid switch address in {}", path.display()),
                    )
                });
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && started.elapsed() < Duration::from_secs(2) =>
            {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "failed to read switch state from {}: {}",
                        path.display(),
                        error
                    ),
                ));
            }
        }
    }
}

pub(crate) fn store_switch_addr(addr: SocketAddr) -> io::Result<()> {
    fs::write(switch_state_path(), format!("{addr}\n"))
}

pub(crate) fn clear_switch_addr(addr: SocketAddr) -> io::Result<()> {
    let path = switch_state_path();
    match fs::read_to_string(&path) {
        Ok(current) if current.trim() == addr.to_string() => match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        },
        Ok(_) | Err(_) => Ok(()),
    }
}

pub(crate) fn load_uplink_for_gateway(gateway_ip: Ipv4Addr) -> io::Result<SocketAddr> {
    let path = router_uplink_state_path(gateway_ip);
    let content = fs::read_to_string(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to read router uplink state from {}: {}",
                path.display(),
                error
            ),
        )
    })?;

    SocketAddr::from_str(content.trim()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid router uplink address in {}", path.display()),
        )
    })
}

pub(crate) fn store_router_uplink(gateway_ip: Ipv4Addr, addr: SocketAddr) -> io::Result<()> {
    fs::write(router_uplink_state_path(gateway_ip), format!("{addr}\n"))
}

pub(crate) fn clear_router_uplink(gateway_ip: Ipv4Addr, addr: SocketAddr) -> io::Result<()> {
    let path = router_uplink_state_path(gateway_ip);
    match fs::read_to_string(&path) {
        Ok(current) if current.trim() == addr.to_string() => match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        },
        Ok(_) | Err(_) => Ok(()),
    }
}

pub(crate) fn parse_send_command(line: &str) -> Result<(Ipv4Addr, u16, String), &'static str> {
    let trimmed = line.trim();
    let mut parts = trimmed.split_whitespace();
    let command = parts.next().unwrap_or_default();
    if command != "send" {
        return Err("use: send <dst-ip> <dst-port> <message>");
    }

    let dst_ip_text = parts
        .next()
        .ok_or("use: send <dst-ip> <dst-port> <message>")?;
    let dst_port_text = parts
        .next()
        .ok_or("use: send <dst-ip> <dst-port> <message>")?;
    let dst_ip = dst_ip_text
        .parse::<Ipv4Addr>()
        .map_err(|_| "invalid destination ip")?;
    let dst_port = dst_port_text
        .parse::<u16>()
        .map_err(|_| "invalid destination port")?;

    let payload = trimmed
        .strip_prefix(command)
        .and_then(|rest| rest.trim_start().strip_prefix(dst_ip_text))
        .and_then(|rest| rest.trim_start().strip_prefix(dst_port_text))
        .map(str::trim_start)
        .filter(|rest| !rest.is_empty())
        .ok_or("use: send <dst-ip> <dst-port> <message>")?
        .to_string();

    Ok((dst_ip, dst_port, payload))
}

pub(crate) fn parse_ping_command(line: &str) -> Result<Ipv4Addr, &'static str> {
    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or_default();
    if command != "/ping" {
        return Err("use: /ping <dst-ip>");
    }

    parts
        .next()
        .ok_or("use: /ping <dst-ip>")?
        .parse::<Ipv4Addr>()
        .map_err(|_| "invalid destination ip")
}

pub(crate) fn print_mac_table(switch: &LearningSwitch) {
    let entries = switch.mac_table();
    if entries.is_empty() {
        println!("mac table: empty");
        return;
    }

    println!("mac table:");
    for (mac, port) in entries {
        println!("  {mac} -> {port}");
    }
}

pub(crate) fn print_ports(switch: &LearningSwitch) {
    let ports = switch.ports();
    if ports.is_empty() {
        println!("ports: empty");
        return;
    }

    println!("ports:");
    for port in ports {
        println!("  {port}");
    }
}

pub(crate) fn print_leases(entries: &[LeaseEntry]) {
    if entries.is_empty() {
        println!("leases: empty");
        return;
    }

    println!("leases:");
    for entry in entries {
        println!(
            "  {} -> {} ({}s left)",
            entry.mac,
            entry.ip,
            entry.remaining.as_secs()
        );
    }
}

pub(crate) fn print_usage() {
    eprintln!("usage:");
    eprintln!("  cargo run -- demo");
    eprintln!("  cargo run -- switch [bind]");
    eprintln!("  cargo run -- wan [bind]");
    eprintln!("  cargo run -- gateway");
    eprintln!("  cargo run -- gateway [bind]");
    eprintln!("  cargo run -- gateway <gateway-ip> <mac> <pool-start> <pool-end>");
    eprintln!("  cargo run -- gateway <bind> <gateway-ip> <mac> <pool-start> <pool-end>");
    eprintln!("  cargo run -- dhcp-server <server-ip> <mac> <pool-start> <pool-end>");
    eprintln!(
        "  cargo run -- dhcp-server <bind> <switch> <server-ip> <mac> <pool-start> <pool-end>"
    );
    eprintln!("  cargo run -- dhcp-client <mac>");
    eprintln!("  cargo run -- dhcp-client <bind> <switch> <mac>");
    eprintln!("  cargo run -- chat <mac> <listen-port>");
    eprintln!("  cargo run -- chat <mac> <listen-port> <uplink-mac>");
    eprintln!("  cargo run -- chat <mac> <listen-port> <gateway-ip>");
    eprintln!("  cargo run -- chat <mac> <listen-port> <src-port>");
    eprintln!("  cargo run -- chat <mac> <listen-port> <src-port> <gateway-ip>");
    eprintln!("  cargo run -- chat <local-ip|auto> <mac> <listen-port> <src-port>");
    eprintln!("  cargo run -- chat <local-ip|auto> <mac> <listen-port> <src-port> <gateway-ip>");
    eprintln!("  cargo run -- chat <bind> <switch> <local-ip|auto> <mac> <listen-port> <src-port>");
    eprintln!(
        "  cargo run -- router <if0-bind> <if0-uplink|listen> <if0-ip> <if0-mac> <if1-bind> <if1-uplink|listen> <if1-ip> <if1-mac> [<cidr> <next-hop|direct>]..."
    );
    eprintln!("  cargo run -- router <router-mac>");
    eprintln!("  cargo run -- http-server <mac> <listen-port>");
    eprintln!("  cargo run -- http-server <mac> <listen-port> <uplink-mac>");
    eprintln!("  cargo run -- http-server <local-ip|auto> <mac> <listen-port>");
    eprintln!("  cargo run -- http-server <bind> <switch> <local-ip|auto> <mac> <listen-port>");
    eprintln!("  cargo run -- http-get <peer-ip> <mac> <server-port> <path>");
    eprintln!("  cargo run -- http-get <peer-ip> <mac> <server-port> <path> <uplink-mac>");
    eprintln!("  cargo run -- http-get <local-ip|auto> <peer-ip> <mac> <server-port> <path>");
    eprintln!(
        "  cargo run -- http-get <bind> <switch> <local-ip|auto> <peer-ip> <mac> <server-port> <path>"
    );
}
