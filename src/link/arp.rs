use std::net::Ipv4Addr;

use super::ethernet::MacAddr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpOperation {
    Request = 1,
    Reply = 2,
}

impl ArpOperation {
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Request),
            2 => Some(Self::Reply),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArpPacket {
    pub operation: ArpOperation,
    pub sender_mac: MacAddr,
    pub sender_ip: Ipv4Addr,
    pub target_mac: MacAddr,
    pub target_ip: Ipv4Addr,
}

impl ArpPacket {
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(28);
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0x0800u16.to_be_bytes());
        bytes.push(6);
        bytes.push(4);
        bytes.extend_from_slice(&(self.operation as u16).to_be_bytes());
        bytes.extend_from_slice(&self.sender_mac.octets());
        bytes.extend_from_slice(&self.sender_ip.octets());
        bytes.extend_from_slice(&self.target_mac.octets());
        bytes.extend_from_slice(&self.target_ip.octets());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 28 {
            return None;
        }
        if u16::from_be_bytes(bytes[0..2].try_into().ok()?) != 1 {
            return None;
        }
        if u16::from_be_bytes(bytes[2..4].try_into().ok()?) != 0x0800 {
            return None;
        }
        if bytes[4] != 6 || bytes[5] != 4 {
            return None;
        }

        let sender_ip = Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[14..18]).ok()?);
        let target_ip = Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[24..28]).ok()?);

        Some(Self {
            operation: ArpOperation::from_u16(u16::from_be_bytes(bytes[6..8].try_into().ok()?))?,
            sender_mac: MacAddr::new(bytes[8..14].try_into().ok()?),
            sender_ip,
            target_mac: MacAddr::new(bytes[18..24].try_into().ok()?),
            target_ip,
        })
    }
}
