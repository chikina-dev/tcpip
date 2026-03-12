use std::collections::HashSet;
use std::io::{self, BufRead};
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tcpip_userland::Host;
use tcpip_userland::application::dhcp_client::{LeaseManager, send_release};
use tcpip_userland::application::http::{HttpRequest, build_get_request, http_message_complete};
use tcpip_userland::link::LinkEndpoint;
use tcpip_userland::link::ethernet::MacAddr;

use crate::command::common::{
    default_bind_addr, load_switch_addr, load_uplink_for_gateway, print_usage, route_request,
};
use crate::command::wan_config::WanConfig;

pub fn run_server(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(args.len(), 2 | 3 | 5) {
        print_usage();
        return Ok(());
    }

    let (bind_addr, switch_addr, local_ip, mut lease_state, mac, listen_port, default_gateway) =
        match args.len() {
            5 => {
                let bind_addr = SocketAddr::from_str(&args[0])?;
                let switch_addr = SocketAddr::from_str(&args[1])?;
                let mac = MacAddr::from_str(&args[3]).map_err(io::Error::other)?;
                let (local_ip, lease_state) = if args[2] == "auto" {
                    let (local_ip, lease_manager) =
                        LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                    (local_ip, Some(lease_manager))
                } else {
                    (args[2].parse()?, None)
                };
                (
                    bind_addr,
                    switch_addr,
                    local_ip,
                    lease_state,
                    mac,
                    args[4].parse::<u16>()?,
                    None,
                )
            }
            3 => {
                let bind_addr = default_bind_addr();
                if let Ok(uplink_mac) = MacAddr::from_str(&args[2]).map_err(io::Error::other) {
                    let config = WanConfig::load()?;
                    let uplink = config.find_uplink(uplink_mac).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::NotFound,
                            format!("uplink {uplink_mac} not found in wan.toml"),
                        )
                    })?;
                    let switch_addr = uplink.bind;
                    let mac = MacAddr::from_str(&args[0]).map_err(io::Error::other)?;
                    let (local_ip, lease_manager) =
                        LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                    (
                        bind_addr,
                        switch_addr,
                        local_ip,
                        Some(lease_manager),
                        mac,
                        args[1].parse::<u16>()?,
                        Some(uplink.ip),
                    )
                } else {
                    let switch_addr = load_switch_addr()?;
                    let mac = MacAddr::from_str(&args[1]).map_err(io::Error::other)?;
                    let (local_ip, lease_state) = if args[0] == "auto" {
                        let (local_ip, lease_manager) =
                            LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                        (local_ip, Some(lease_manager))
                    } else {
                        (args[0].parse()?, None)
                    };
                    (
                        bind_addr,
                        switch_addr,
                        local_ip,
                        lease_state,
                        mac,
                        args[2].parse::<u16>()?,
                        None,
                    )
                }
            }
            2 => {
                let bind_addr = default_bind_addr();
                let switch_addr = load_switch_addr()?;
                let mac = MacAddr::from_str(&args[0]).map_err(io::Error::other)?;
                let (local_ip, lease_manager) =
                    LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                (
                    bind_addr,
                    switch_addr,
                    local_ip,
                    Some(lease_manager),
                    mac,
                    args[1].parse::<u16>()?,
                    None,
                )
            }
            _ => unreachable!(),
        };

    let link = LinkEndpoint::udp(bind_addr, switch_addr, mac)?;
    let actual_bind = link.local_addr()?.expect("udp addr");
    let mut host = Host::new_with_link("http-server", local_ip, mac, link);
    host.set_default_gateway(
        default_gateway.or_else(|| lease_state.as_ref().and_then(LeaseManager::default_gateway)),
    );
    host.listen_tcp(listen_port);

    let mut open_connections = HashSet::new();
    let mut request_buffers = std::collections::HashMap::new();

    println!("http server started");
    println!("bind={actual_bind} uplink={switch_addr}");
    println!("local-ip={local_ip} tcp-port={listen_port}");
    if let Some(default_gateway) =
        default_gateway.or_else(|| lease_state.as_ref().and_then(LeaseManager::default_gateway))
    {
        println!("default-gateway={default_gateway}");
    }
    println!("routes: / and /hello");
    println!("commands: /quit");

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

    loop {
        if let Some(lease_manager) = lease_state.as_mut() {
            let next_uplink = default_gateway
                .or_else(|| lease_manager.default_gateway())
                .and_then(|gateway_ip| load_uplink_for_gateway(gateway_ip).ok())
                .or_else(|| load_switch_addr().ok());
            if let Some(switch_addr) = next_uplink {
                lease_manager.update_switch_addr(switch_addr);
            }
            lease_manager.maintain(&mut host);
        }
        match receiver.try_recv() {
            Ok(line) if line == "/quit" => break,
            Ok(line) if !line.is_empty() => eprintln!("unknown http-server command: {line}"),
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => break,
        }

        let _ = host.tick();

        while let Some(connection) = host.accept_tcp(listen_port) {
            println!(
                "accepted remote={}:{}",
                connection.remote_ip, connection.remote_port
            );
            open_connections.insert(connection);
        }

        let connections: Vec<_> = open_connections.iter().copied().collect();
        for connection in connections {
            while let Some(chunk) = host.recv_tcp(connection) {
                let buffer = request_buffers.entry(connection).or_insert_with(Vec::new);
                buffer.extend_from_slice(&chunk);

                if !http_message_complete(buffer) {
                    continue;
                }

                let request = HttpRequest::parse(buffer).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP request")
                })?;
                let response = route_request(&request);
                let response_bytes = response.encode();
                let _ = host.send_tcp(connection, response_bytes);
                println!(
                    "served {} {} -> {}",
                    request.method, request.path, response.status_code
                );
                let _ = host.drop_tcp(connection);
                request_buffers.remove(&connection);
                open_connections.remove(&connection);
                break;
            }
        }

        thread::sleep(Duration::from_millis(10));
    }

    if let Some(assigned_ip) = lease_state.as_ref().and_then(LeaseManager::release_ip) {
        send_release(switch_addr, mac, assigned_ip, default_bind_addr());
    }

    Ok(())
}

pub fn run_get(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(args.len(), 4 | 5 | 7) {
        print_usage();
        return Ok(());
    }

    let (
        bind_addr,
        switch_addr,
        local_ip,
        mut lease_state,
        peer_ip,
        mac,
        server_port,
        path,
        default_gateway,
    ) = match args.len() {
        7 => {
            let bind_addr = SocketAddr::from_str(&args[0])?;
            let switch_addr = SocketAddr::from_str(&args[1])?;
            let mac = MacAddr::from_str(&args[4]).map_err(io::Error::other)?;
            let (local_ip, lease_state) = if args[2] == "auto" {
                let (local_ip, lease_manager) =
                    LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                (local_ip, Some(lease_manager))
            } else {
                (args[2].parse()?, None)
            };
            (
                bind_addr,
                switch_addr,
                local_ip,
                lease_state,
                Ipv4Addr::from_str(&args[3])?,
                mac,
                args[5].parse::<u16>()?,
                args[6].clone(),
                None,
            )
        }
        5 => {
            let bind_addr = default_bind_addr();
            if let Ok(uplink_mac) = MacAddr::from_str(&args[4]).map_err(io::Error::other) {
                let config = WanConfig::load()?;
                let uplink = config.find_uplink(uplink_mac).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("uplink {uplink_mac} not found in wan.toml"),
                    )
                })?;
                let switch_addr = uplink.bind;
                let mac = MacAddr::from_str(&args[1]).map_err(io::Error::other)?;
                let (local_ip, lease_manager) =
                    LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                (
                    bind_addr,
                    switch_addr,
                    local_ip,
                    Some(lease_manager),
                    Ipv4Addr::from_str(&args[0])?,
                    mac,
                    args[2].parse::<u16>()?,
                    args[3].clone(),
                    Some(uplink.ip),
                )
            } else {
                let switch_addr = load_switch_addr()?;
                let mac = MacAddr::from_str(&args[2]).map_err(io::Error::other)?;
                let (local_ip, lease_state) = if args[0] == "auto" {
                    let (local_ip, lease_manager) =
                        LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                    (local_ip, Some(lease_manager))
                } else {
                    (args[0].parse()?, None)
                };
                (
                    bind_addr,
                    switch_addr,
                    local_ip,
                    lease_state,
                    Ipv4Addr::from_str(&args[1])?,
                    mac,
                    args[3].parse::<u16>()?,
                    args[4].clone(),
                    None,
                )
            }
        }
        4 => {
            let bind_addr = default_bind_addr();
            let switch_addr = load_switch_addr()?;
            let mac = MacAddr::from_str(&args[1]).map_err(io::Error::other)?;
            let (local_ip, lease_manager) = LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
            (
                bind_addr,
                switch_addr,
                local_ip,
                Some(lease_manager),
                Ipv4Addr::from_str(&args[0])?,
                mac,
                args[2].parse::<u16>()?,
                args[3].clone(),
                None,
            )
        }
        _ => unreachable!(),
    };

    let link = LinkEndpoint::udp(bind_addr, switch_addr, mac)?;
    let actual_bind = link.local_addr()?.expect("udp addr");
    let mut host = Host::new_with_link("http-client", local_ip, mac, link);
    host.set_default_gateway(
        default_gateway.or_else(|| lease_state.as_ref().and_then(LeaseManager::default_gateway)),
    );
    let connection = host.connect_tcp(peer_ip, server_port);
    let request = build_get_request(&path, &peer_ip.to_string());
    let started = Instant::now();
    let mut request_sent = false;
    let mut response_buffer = Vec::new();

    loop {
        if let Some(lease_manager) = lease_state.as_mut() {
            let next_uplink = default_gateway
                .or_else(|| lease_manager.default_gateway())
                .and_then(|gateway_ip| load_uplink_for_gateway(gateway_ip).ok())
                .or_else(|| load_switch_addr().ok());
            if let Some(switch_addr) = next_uplink {
                lease_manager.update_switch_addr(switch_addr);
            }
            lease_manager.maintain(&mut host);
        }
        let _ = host.tick();

        if !request_sent && host.is_tcp_established(connection) {
            println!("connected to {}:{}", peer_ip, server_port);
            let _ = host.send_tcp(connection, request.clone());
            println!("bind={actual_bind} uplink={switch_addr}");
            println!("sent GET {}", path);
            request_sent = true;
        }

        while let Some(chunk) = host.recv_tcp(connection) {
            response_buffer.extend_from_slice(&chunk);
        }

        if http_message_complete(&response_buffer) {
            println!("{}", String::from_utf8_lossy(&response_buffer));
            let _ = host.drop_tcp(connection);
            if let Some(assigned_ip) = lease_state.as_ref().and_then(LeaseManager::release_ip) {
                send_release(switch_addr, mac, assigned_ip, default_bind_addr());
            }
            return Ok(());
        }

        if started.elapsed() > Duration::from_secs(5) {
            if let Some(assigned_ip) = lease_state.as_ref().and_then(LeaseManager::release_ip) {
                send_release(switch_addr, mac, assigned_ip, default_bind_addr());
            }
            return Err(io::Error::new(io::ErrorKind::TimedOut, "HTTP response timed out").into());
        }

        thread::sleep(Duration::from_millis(10));
    }
}
