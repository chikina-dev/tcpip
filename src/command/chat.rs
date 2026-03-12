use std::io::{self, BufRead};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tcpip_userland::Host;
use tcpip_userland::application::dhcp_client::LeaseManager;
use tcpip_userland::link::LinkEndpoint;
use tcpip_userland::link::ethernet::MacAddr;

use crate::command::common::{
    default_bind_addr, load_switch_addr, load_uplink_for_gateway, parse_ping_command,
    parse_send_command, print_usage,
};
use crate::command::wan_config::WanConfig;

pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(args.len(), 2..=7) {
        print_usage();
        return Ok(());
    }

    let (
        bind_addr,
        switch_addr,
        local_ip,
        mut lease_state,
        mac,
        listen_port,
        src_port,
        default_gateway,
    ) = match args.len() {
        7 => {
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
                args[5].parse::<u16>()?,
                Some(args[6].parse::<std::net::Ipv4Addr>()?),
            )
        }
        6 => {
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
                args[5].parse::<u16>()?,
                None,
            )
        }
        5 => {
            let bind_addr = default_bind_addr();
            let gateway_ip = args[4].parse::<std::net::Ipv4Addr>()?;
            let switch_addr =
                load_uplink_for_gateway(gateway_ip).or_else(|_| load_switch_addr())?;
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
                args[3].parse::<u16>()?,
                Some(gateway_ip),
            )
        }
        4 => {
            if let Ok(mac) = MacAddr::from_str(&args[0]).map_err(io::Error::other) {
                let bind_addr = default_bind_addr();
                let gateway_ip = args[3].parse::<std::net::Ipv4Addr>()?;
                let switch_addr =
                    load_uplink_for_gateway(gateway_ip).or_else(|_| load_switch_addr())?;
                let listen_port = args[1].parse::<u16>()?;
                let src_port = args[2].parse::<u16>()?;
                let (local_ip, lease_manager) =
                    LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                (
                    bind_addr,
                    switch_addr,
                    local_ip,
                    Some(lease_manager),
                    mac,
                    listen_port,
                    src_port,
                    Some(gateway_ip),
                )
            } else {
                let bind_addr = default_bind_addr();
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
                    args[3].parse::<u16>()?,
                    None,
                )
            }
        }
        3 => {
            let bind_addr = default_bind_addr();
            let mac = MacAddr::from_str(&args[0]).map_err(io::Error::other)?;
            let listen_port = args[1].parse::<u16>()?;
            if let Ok(uplink_mac) = MacAddr::from_str(&args[2]).map_err(io::Error::other) {
                let config = WanConfig::load()?;
                let uplink = config.find_uplink(uplink_mac).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("uplink {uplink_mac} not found in wan.toml"),
                    )
                })?;
                let switch_addr = uplink.bind;
                let (local_ip, lease_manager) =
                    LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
                (
                    bind_addr,
                    switch_addr,
                    local_ip,
                    Some(lease_manager),
                    mac,
                    listen_port,
                    listen_port,
                    Some(uplink.ip),
                )
            } else {
                let gateway_ip = args[2].parse::<std::net::Ipv4Addr>().ok();
                let switch_addr = if let Some(gateway_ip) = gateway_ip {
                    load_uplink_for_gateway(gateway_ip).or_else(|_| load_switch_addr())?
                } else {
                    load_switch_addr()?
                };
                let (local_ip, lease_manager) =
                    LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;

                if let Ok(src_port) = args[2].parse::<u16>() {
                    (
                        bind_addr,
                        switch_addr,
                        local_ip,
                        Some(lease_manager),
                        mac,
                        listen_port,
                        src_port,
                        None,
                    )
                } else {
                    (
                        bind_addr,
                        switch_addr,
                        local_ip,
                        Some(lease_manager),
                        mac,
                        listen_port,
                        listen_port,
                        gateway_ip,
                    )
                }
            }
        }
        2 => {
            let bind_addr = default_bind_addr();
            let switch_addr = load_switch_addr()?;
            let mac = MacAddr::from_str(&args[0]).map_err(io::Error::other)?;
            let listen_port = args[1].parse::<u16>()?;
            let (local_ip, lease_manager) = LeaseManager::bootstrap(bind_addr, switch_addr, mac)?;
            (
                bind_addr,
                switch_addr,
                local_ip,
                Some(lease_manager),
                mac,
                listen_port,
                listen_port,
                None,
            )
        }
        _ => unreachable!(),
    };

    let link = LinkEndpoint::udp(bind_addr, switch_addr, mac)?;
    let actual_bind = link.local_addr()?.expect("udp addr");
    let mut host = Host::new_with_link("terminal", local_ip, mac, link);
    host.set_default_gateway(
        default_gateway.or_else(|| lease_state.as_ref().and_then(LeaseManager::default_gateway)),
    );
    host.open_udp(listen_port);
    if src_port != listen_port {
        host.open_udp(src_port);
    }

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

    println!("chat started");
    println!("bind={actual_bind} uplink={switch_addr}");
    println!("local-ip={local_ip}");
    if let Some(default_gateway) =
        default_gateway.or_else(|| lease_state.as_ref().and_then(LeaseManager::default_gateway))
    {
        println!("default-gateway={default_gateway}");
    }
    println!("listen-port={listen_port} src-port={src_port}");
    println!("commands: send <dst-ip> <dst-port> <message>, /ping <dst-ip>, /quit");

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

        while let Some(datagram) = host.recv_udp(listen_port) {
            println!(
                "recv local={} remote={}:{} {}",
                listen_port,
                datagram.peer.ip(),
                datagram.peer.port(),
                String::from_utf8_lossy(&datagram.payload)
            );
        }

        if src_port != listen_port {
            while let Some(datagram) = host.recv_udp(src_port) {
                println!(
                    "recv local={} remote={}:{} {}",
                    src_port,
                    datagram.peer.ip(),
                    datagram.peer.port(),
                    String::from_utf8_lossy(&datagram.payload)
                );
            }
        }

        while let Some(reply) = host.recv_ping_reply() {
            println!(
                "icmp {} id={} seq={} {}",
                reply.source,
                reply.identifier,
                reply.sequence,
                String::from_utf8_lossy(&reply.payload)
            );
        }

        match receiver.try_recv() {
            Ok(line) if line == "/quit" => break,
            Ok(line) if line.starts_with("/ping") => match parse_ping_command(&line) {
                Ok(dst_ip) => {
                    let _ = host.ping(dst_ip, b"ping".to_vec());
                }
                Err(message) => eprintln!("{message}"),
            },
            Ok(line) if !line.is_empty() => match parse_send_command(&line) {
                Ok((dst_ip, dst_port, payload)) => {
                    host.send_udp(src_port, dst_ip, dst_port, payload.into_bytes());
                }
                Err(message) => eprintln!("{message}"),
            },
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => break,
        }

        thread::sleep(Duration::from_millis(10));
    }

    if let Some(assigned_ip) = lease_state.as_ref().and_then(LeaseManager::release_ip) {
        tcpip_userland::application::dhcp_client::send_release(
            switch_addr,
            mac,
            assigned_ip,
            default_bind_addr(),
        );
    }

    Ok(())
}
