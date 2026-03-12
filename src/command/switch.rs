use std::io::{self, BufRead};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tcpip_userland::link::ethernet::MacAddr;
use tcpip_userland::link::switch::LearningSwitch;

use crate::command::common::{
    clear_switch_addr, default_bind_addr, print_mac_table, print_ports, print_usage,
    store_switch_addr, switch_state_path,
};

pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    run_named(args, "l2 switch")
}

pub fn run_wan(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    run_named(args, "wan")
}

fn run_named(args: Vec<String>, label: &str) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() > 1 {
        print_usage();
        return Ok(());
    }

    let bind_addr = if let Some(arg) = args.first() {
        SocketAddr::from_str(arg)?
    } else {
        default_bind_addr()
    };
    let socket = std::net::UdpSocket::bind(bind_addr)?;
    socket.set_nonblocking(true)?;
    let actual_bind = socket.local_addr()?;
    store_switch_addr(actual_bind)?;

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

    println!("{label} started");
    println!("bind={actual_bind}");
    println!("state-file={}", switch_state_path().display());
    println!("commands: show mac, show ports, /quit");

    loop {
        match receiver.try_recv() {
            Ok(line) if line == "/quit" => break,
            Ok(line) if line == "show mac" => print_mac_table(&switch),
            Ok(line) if line == "show ports" => print_ports(&switch),
            Ok(line) if !line.is_empty() => eprintln!("unknown switch command: {line}"),
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => break,
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
