use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;

use tcpip_userland::link::ethernet::MacAddr;

#[derive(Clone, Debug)]
pub(crate) struct WanConfig {
    routers: Vec<RouterDefinition>,
}

#[derive(Clone, Debug)]
pub(crate) struct RouterDefinition {
    pub id: MacAddr,
    pub if0: InterfaceDefinition,
    pub if1: InterfaceDefinition,
    pub routes: Vec<RouteDefinition>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InterfaceDefinition {
    pub bind: SocketAddr,
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub mode: InterfaceMode,
    pub uplink: Option<SocketAddr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InterfaceMode {
    Listen,
    Uplink,
}

#[derive(Clone, Debug)]
pub(crate) struct RouteDefinition {
    pub cidr: String,
    pub next_hop: String,
}

#[derive(Default)]
struct PartialRouter {
    id: Option<MacAddr>,
    if0_bind: Option<SocketAddr>,
    if0_mac: Option<MacAddr>,
    if0_ip: Option<Ipv4Addr>,
    if0_mode: Option<InterfaceMode>,
    if0_uplink: Option<SocketAddr>,
    if1_bind: Option<SocketAddr>,
    if1_mac: Option<MacAddr>,
    if1_ip: Option<Ipv4Addr>,
    if1_mode: Option<InterfaceMode>,
    if1_uplink: Option<SocketAddr>,
    routes: Vec<RouteDefinition>,
}

enum Section {
    None,
    Router(usize),
    RouterRoute(usize),
}

impl WanConfig {
    pub(crate) fn load() -> io::Result<Self> {
        let path = PathBuf::from("wan.toml");
        let content = fs::read_to_string(&path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to read {}: {}", path.display(), error),
            )
        })?;
        parse_wan_config(&content)
    }

    pub(crate) fn find_router(&self, id: MacAddr) -> Option<&RouterDefinition> {
        self.routers.iter().find(|router| router.id == id)
    }

    pub(crate) fn find_uplink(&self, mac: MacAddr) -> Option<InterfaceDefinition> {
        for router in &self.routers {
            if router.id == mac {
                if router.if0.mode == InterfaceMode::Listen {
                    return Some(router.if0);
                }
                if router.if1.mode == InterfaceMode::Listen {
                    return Some(router.if1);
                }
            }

            if router.if0.mac == mac {
                return Some(router.if0);
            }
            if router.if1.mac == mac {
                return Some(router.if1);
            }
        }
        None
    }
}

fn parse_wan_config(content: &str) -> io::Result<WanConfig> {
    let mut routers = Vec::<PartialRouter>::new();
    let mut current_section = Section::None;

    for raw_line in content.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }

        match line {
            "[[router]]" => {
                routers.push(PartialRouter::default());
                current_section = Section::Router(routers.len() - 1);
                continue;
            }
            "[[router.route]]" => {
                let Some(router) = routers.last_mut() else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "router.route must come after router",
                    ));
                };
                router.routes.push(RouteDefinition {
                    cidr: String::new(),
                    next_hop: String::new(),
                });
                current_section = Section::RouterRoute(routers.len() - 1);
                continue;
            }
            _ => {}
        }

        let (key, value) = parse_key_value(line)?;
        match current_section {
            Section::None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "key/value must appear inside [[router]] or [[router.route]]",
                ));
            }
            Section::Router(index) => {
                let router = routers.get_mut(index).expect("router section");
                assign_router_key(router, key, value)?;
            }
            Section::RouterRoute(index) => {
                let router = routers.get_mut(index).expect("route section");
                let route = router.routes.last_mut().expect("route exists");
                assign_route_key(route, key, value)?;
            }
        }
    }

    let routers = routers
        .into_iter()
        .map(build_router)
        .collect::<io::Result<Vec<_>>>()?;

    Ok(WanConfig { routers })
}

fn parse_key_value(line: &str) -> io::Result<(&str, &str)> {
    let (key, raw_value) = line.split_once('=').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config line: {line}"),
        )
    })?;
    Ok((key.trim(), strip_quotes(raw_value.trim())?))
}

fn strip_quotes(value: &str) -> io::Result<&str> {
    if let Some(stripped) = value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
    {
        Ok(stripped)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected quoted string, got {value}"),
        ))
    }
}

fn assign_router_key(router: &mut PartialRouter, key: &str, value: &str) -> io::Result<()> {
    match key {
        "id" => router.id = Some(MacAddr::from_str(value).map_err(io::Error::other)?),
        "if0_bind" => {
            router.if0_bind = Some(SocketAddr::from_str(value).map_err(io::Error::other)?)
        }
        "if0_mac" => router.if0_mac = Some(MacAddr::from_str(value).map_err(io::Error::other)?),
        "if0_ip" => router.if0_ip = Some(Ipv4Addr::from_str(value).map_err(io::Error::other)?),
        "if0_mode" => router.if0_mode = Some(parse_mode(value)?),
        "if0_uplink" => {
            router.if0_uplink = Some(SocketAddr::from_str(value).map_err(io::Error::other)?)
        }
        "if1_bind" => {
            router.if1_bind = Some(SocketAddr::from_str(value).map_err(io::Error::other)?)
        }
        "if1_mac" => router.if1_mac = Some(MacAddr::from_str(value).map_err(io::Error::other)?),
        "if1_ip" => router.if1_ip = Some(Ipv4Addr::from_str(value).map_err(io::Error::other)?),
        "if1_mode" => router.if1_mode = Some(parse_mode(value)?),
        "if1_uplink" => {
            router.if1_uplink = Some(SocketAddr::from_str(value).map_err(io::Error::other)?)
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown router key: {key}"),
            ));
        }
    }
    Ok(())
}

fn assign_route_key(route: &mut RouteDefinition, key: &str, value: &str) -> io::Result<()> {
    match key {
        "cidr" => route.cidr = value.to_string(),
        "next_hop" => route.next_hop = value.to_string(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown router.route key: {key}"),
            ));
        }
    }
    Ok(())
}

fn build_router(partial: PartialRouter) -> io::Result<RouterDefinition> {
    let if0 = build_interface(
        partial.if0_bind,
        partial.if0_mac,
        partial.if0_ip,
        partial.if0_mode,
        partial.if0_uplink,
        "if0",
    )?;
    let if1 = build_interface(
        partial.if1_bind,
        partial.if1_mac,
        partial.if1_ip,
        partial.if1_mode,
        partial.if1_uplink,
        "if1",
    )?;

    for route in &partial.routes {
        if route.cidr.is_empty() || route.next_hop.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "each [[router.route]] needs cidr and next_hop",
            ));
        }
    }

    Ok(RouterDefinition {
        id: partial
            .id
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "router.id is required"))?,
        if0,
        if1,
        routes: partial.routes,
    })
}

fn build_interface(
    bind: Option<SocketAddr>,
    mac: Option<MacAddr>,
    ip: Option<Ipv4Addr>,
    mode: Option<InterfaceMode>,
    uplink: Option<SocketAddr>,
    label: &str,
) -> io::Result<InterfaceDefinition> {
    let mode = mode.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label}_mode is required"),
        )
    })?;
    if mode == InterfaceMode::Uplink && uplink.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label}_uplink is required when mode is uplink"),
        ));
    }

    Ok(InterfaceDefinition {
        bind: bind.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label}_bind is required"),
            )
        })?,
        mac: mac.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label}_mac is required"),
            )
        })?,
        ip: ip.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label}_ip is required"),
            )
        })?,
        mode,
        uplink,
    })
}

fn parse_mode(value: &str) -> io::Result<InterfaceMode> {
    match value {
        "listen" => Ok(InterfaceMode::Listen),
        "uplink" => Ok(InterfaceMode::Uplink),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid interface mode: {value}"),
        )),
    }
}
