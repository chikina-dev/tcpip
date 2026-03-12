use std::net::Ipv4Addr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpProtocol {
    Icmp = 1,
    Tcp = 6,
    Udp = 17,
}

impl IpProtocol {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Icmp),
            6 => Some(Self::Tcp),
            17 => Some(Self::Udp),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ipv4Packet {
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub protocol: IpProtocol,
    pub ttl: u8,
    pub payload: Vec<u8>,
}

impl Ipv4Packet {
    pub fn encode(&self) -> Vec<u8> {
        let total_len = 20 + self.payload.len();
        let mut bytes = Vec::with_capacity(total_len);
        bytes.push((4 << 4) | 5);
        bytes.push(0);
        bytes.extend_from_slice(&(total_len as u16).to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.push(self.ttl);
        bytes.push(self.protocol as u8);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&self.src.octets());
        bytes.extend_from_slice(&self.dst.octets());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 20 {
            return None;
        }
        let version = bytes[0] >> 4;
        let ihl = bytes[0] & 0x0f;
        if version != 4 || ihl < 5 {
            return None;
        }
        let header_len = (ihl as usize) * 4;
        let total_len = u16::from_be_bytes(bytes[2..4].try_into().ok()?) as usize;
        if bytes.len() < total_len || total_len < header_len {
            return None;
        }
        let src = Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[12..16]).ok()?);
        let dst = Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[16..20]).ok()?);

        Some(Self {
            src,
            dst,
            protocol: IpProtocol::from_u8(bytes[9])?,
            ttl: bytes[8],
            payload: bytes[header_len..total_len].to_vec(),
        })
    }
}
