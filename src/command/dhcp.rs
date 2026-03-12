use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use tcpip_userland::Host;
use tcpip_userland::application::dhcp::DHCP_SERVER_PORT;
use tcpip_userland::application::dhcp_client::acquire;
use tcpip_userland::application::dhcp_server::DhcpServer;
use tcpip_userland::link::LinkEndpoint;
use tcpip_userland::link::ethernet::MacAddr;

use crate::command::common::{default_bind_addr, load_switch_addr, print_usage};

pub fn run_server(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(args.len(), 4 | 6) {
        print_usage();
        return Ok(());
    }

    let (bind_addr, switch_addr, server_ip, mac, pool_start, pool_end) = if args.len() == 6 {
        (
            SocketAddr::from_str(&args[0])?,
            SocketAddr::from_str(&args[1])?,
            Ipv4Addr::from_str(&args[2])?,
            MacAddr::from_str(&args[3]).map_err(io::Error::other)?,
            Ipv4Addr::from_str(&args[4])?,
            Ipv4Addr::from_str(&args[5])?,
        )
    } else {
        (
            default_bind_addr(),
            load_switch_addr()?,
            Ipv4Addr::from_str(&args[0])?,
            MacAddr::from_str(&args[1]).map_err(io::Error::other)?,
            Ipv4Addr::from_str(&args[2])?,
            Ipv4Addr::from_str(&args[3])?,
        )
    };

    let link = LinkEndpoint::udp(bind_addr, switch_addr, mac)?;
    let actual_bind = link.local_addr()?.expect("udp addr");
    let mut host = Host::new_with_link("dhcp-server", server_ip, mac, link);
    host.open_udp(DHCP_SERVER_PORT);
    let mut server = DhcpServer::new(pool_start, pool_end, server_ip)?;

    println!("dhcp server started");
    println!("bind={actual_bind} switch={switch_addr}");
    println!("server-ip={server_ip}");
    println!("pool={}..={}", pool_start, pool_end);
    println!("lease-seconds={}", server.lease_seconds());

    loop {
        for event in server.expire() {
            println!("{}", event.describe());
        }

        let _ = host.tick();
        while let Some(datagram) = host.recv_udp(DHCP_SERVER_PORT) {
            if let Some(event) = server.handle_datagram(&mut host, &datagram.payload) {
                println!("{}", event.describe());
            }
        }

        thread::sleep(Duration::from_millis(10));
    }
}

pub fn run_client(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(args.len(), 1 | 3) {
        print_usage();
        return Ok(());
    }

    let (bind_addr, switch_addr, mac) = if args.len() == 3 {
        (
            SocketAddr::from_str(&args[0])?,
            SocketAddr::from_str(&args[1])?,
            MacAddr::from_str(&args[2]).map_err(io::Error::other)?,
        )
    } else {
        (
            default_bind_addr(),
            load_switch_addr()?,
            MacAddr::from_str(&args[0]).map_err(io::Error::other)?,
        )
    };

    let lease = acquire(bind_addr, switch_addr, mac)?;
    println!("dhcp client started");
    println!("bind={} switch={switch_addr}", lease.actual_bind);
    println!("mac={mac}");
    println!("assigned-ip={}", lease.assigned_ip);
    println!("default-gateway={}", lease.gateway_ip);
    println!("lease-seconds={}", lease.lease_duration.as_secs());
    Ok(())
}
