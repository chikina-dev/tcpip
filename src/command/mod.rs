mod chat;
mod common;
mod demo;
mod dhcp;
mod gateway;
mod http;
mod router;
mod switch;
mod wan_config;

use std::env;

pub fn dispatch() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None | Some("demo") => demo::run(),
        Some("switch") => switch::run(args.collect())?,
        Some("wan") => switch::run_wan(args.collect())?,
        Some("gateway") => gateway::run(args.collect())?,
        Some("dhcp-server") => dhcp::run_server(args.collect())?,
        Some("dhcp-client") => dhcp::run_client(args.collect())?,
        Some("chat") => chat::run(args.collect())?,
        Some("router") => router::run(args.collect())?,
        Some("http-server") => http::run_server(args.collect())?,
        Some("http-get") => http::run_get(args.collect())?,
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            common::print_usage();
        }
    }
    Ok(())
}
