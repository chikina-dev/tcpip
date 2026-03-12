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
use tcpip_userland::link::switch::LearningSwitch;

use crate::command::common::{
    clear_switch_addr, default_bind_addr, print_leases, print_mac_table, print_ports, print_usage,
    store_switch_addr, switch_state_path,
};

pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(args.len(), 0 | 1 | 4 | 5) {
        print_usage();
        return Ok(());
    }

    let default_gateway_ip = Ipv4Addr::new(10, 0, 0, 254);
    let default_gateway_mac = MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0xfe]);
    let default_pool_start = Ipv4Addr::new(10, 0, 0, 10);
    let default_pool_end = Ipv4Addr::new(10, 0, 0, 99);

    let (bind_addr, gateway_ip, gateway_mac, pool_start, pool_end) = match args.len() {
        0 => (
            default_bind_addr(),
            default_gateway_ip,
            default_gateway_mac,
            default_pool_start,
            default_pool_end,
        ),
        1 => (
            SocketAddr::from_str(&args[0])?,
            default_gateway_ip,
            default_gateway_mac,
            default_pool_start,
            default_pool_end,
        ),
        4 => (
            default_bind_addr(),
            Ipv4Addr::from_str(&args[0])?,
            MacAddr::from_str(&args[1]).map_err(io::Error::other)?,
            Ipv4Addr::from_str(&args[2])?,
            Ipv4Addr::from_str(&args[3])?,
        ),
        5 => (
            SocketAddr::from_str(&args[0])?,
            Ipv4Addr::from_str(&args[1])?,
            MacAddr::from_str(&args[2]).map_err(io::Error::other)?,
            Ipv4Addr::from_str(&args[3])?,
            Ipv4Addr::from_str(&args[4])?,
        ),
        _ => unreachable!(),
    };

    let socket = std::net::UdpSocket::bind(bind_addr)?;
    socket.set_nonblocking(true)?;
    let actual_bind = socket.local_addr()?;
    store_switch_addr(actual_bind)?;

    let gateway_link = LinkEndpoint::udp(default_bind_addr(), actual_bind, gateway_mac)?;
    let gateway_link_addr = gateway_link.local_addr()?.expect("udp addr");
    let mut host = Host::new_with_link("gateway", gateway_ip, gateway_mac, gateway_link);
    host.open_udp(DHCP_SERVER_PORT);
    let mut dhcp = DhcpServer::new(pool_start, pool_end, gateway_ip)?;

    let mut switch = LearningSwitch::default();
    let mut buffer = [0u8; 2048];
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

    println!("gateway started");
    println!("switch-bind={actual_bind}");
    println!("gateway-bind={gateway_link_addr}");
    println!("gateway-ip={gateway_ip}");
    println!("dhcp-pool={}..={}", pool_start, pool_end);
    println!("lease-seconds={}", dhcp.lease_seconds());
    println!("state-file={}", switch_state_path().display());
    println!("commands: show mac, show ports, show leases, /quit");

    loop {
        for event in dhcp.expire() {
            println!("{}", event.describe());
        }

        match receiver.try_recv() {
            Ok(line) if line == "/quit" => break,
            Ok(line) if line == "show mac" => print_mac_table(&switch),
            Ok(line) if line == "show ports" => print_ports(&switch),
            Ok(line) if line == "show leases" => print_leases(&dhcp.leases()),
            Ok(line) if !line.is_empty() => eprintln!("unknown gateway command: {line}"),
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => break,
        }

        let _ = host.tick();
        while let Some(datagram) = host.recv_udp(DHCP_SERVER_PORT) {
            if let Some(event) = dhcp.handle_datagram(&mut host, &datagram.payload) {
                println!("{}", event.describe());
            }
        }

        loop {
            match socket.recv_from(&mut buffer) {
                Ok((len, ingress_port)) if len >= 7 && buffer[0] == 1 => {
                    if let Ok(mac_bytes) = <[u8; 6]>::try_from(&buffer[1..7]) {
                        let mac = MacAddr::new(mac_bytes);
                        if switch.register_port(mac, ingress_port) {
                            println!("learned {mac} on {ingress_port}");
                        }
                    }
                }
                Ok((len, ingress_port)) if len > 1 && buffer[0] == 0 => {
                    let Some(frame) =
                        tcpip_userland::link::ethernet::EthernetFrame::decode(&buffer[1..len])
                    else {
                        continue;
                    };
                    let egress_ports = switch.forward(ingress_port, &frame);
                    for egress_port in egress_ports {
                        let _ = socket.send_to(&buffer[..len], egress_port);
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    clear_switch_addr(actual_bind)?;
    Ok(())
}
