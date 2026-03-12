use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use super::ethernet::{EthernetFrame, MacAddr};

#[derive(Default)]
pub struct LearningSwitch {
    forwarding_table: HashMap<MacAddr, SocketAddr>,
    ports: HashSet<SocketAddr>,
}

impl LearningSwitch {
    pub fn register_port(&mut self, mac: MacAddr, port: SocketAddr) -> bool {
        self.ports.insert(port);
        self.forwarding_table.insert(mac, port) != Some(port)
    }

    pub fn forward(&mut self, ingress_port: SocketAddr, frame: &EthernetFrame) -> Vec<SocketAddr> {
        self.ports.insert(ingress_port);
        self.forwarding_table.insert(frame.src, ingress_port);

        if frame.dst == MacAddr::BROADCAST {
            return self
                .ports
                .iter()
                .copied()
                .filter(|port| *port != ingress_port)
                .collect();
        }

        match self.forwarding_table.get(&frame.dst).copied() {
            Some(port) if port != ingress_port => vec![port],
            Some(_) => Vec::new(),
            None => self
                .ports
                .iter()
                .copied()
                .filter(|port| *port != ingress_port)
                .collect(),
        }
    }

    pub fn mac_table(&self) -> Vec<(MacAddr, SocketAddr)> {
        let mut entries: Vec<_> = self
            .forwarding_table
            .iter()
            .map(|(mac, port)| (*mac, *port))
            .collect();
        entries.sort_by_key(|(mac, port)| (mac.octets(), port.to_string()));
        entries
    }

    pub fn ports(&self) -> Vec<SocketAddr> {
        let mut ports: Vec<_> = self.ports.iter().copied().collect();
        ports.sort_by_key(|port| port.to_string());
        ports
    }
}
