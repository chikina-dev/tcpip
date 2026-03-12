use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    pub const BROADCAST: Self = Self([0xff; 6]);

    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    pub const fn octets(self) -> [u8; 6] {
        self.0
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

impl FromStr for MacAddr {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; 6];
        let mut parts = value.split(':');

        for byte in &mut bytes {
            let part = parts.next().ok_or("invalid MAC address")?;
            *byte = u8::from_str_radix(part, 16).map_err(|_| "invalid MAC address")?;
        }

        if parts.next().is_some() {
            return Err("invalid MAC address");
        }

        Ok(Self::new(bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtherType {
    Arp = 0x0806,
    Ipv4 = 0x0800,
}

impl EtherType {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0806 => Some(Self::Arp),
            0x0800 => Some(Self::Ipv4),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EthernetFrame {
    pub dst: MacAddr,
    pub src: MacAddr,
    pub ethertype: EtherType,
    pub payload: Vec<u8>,
}

impl EthernetFrame {
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(14 + self.payload.len());
        bytes.extend_from_slice(&self.dst.octets());
        bytes.extend_from_slice(&self.src.octets());
        bytes.extend_from_slice(&(self.ethertype as u16).to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 14 {
            return None;
        }

        let dst = MacAddr::new(bytes[0..6].try_into().ok()?);
        let src = MacAddr::new(bytes[6..12].try_into().ok()?);
        let ethertype = EtherType::from_u16(u16::from_be_bytes(bytes[12..14].try_into().ok()?))?;

        Some(Self {
            dst,
            src,
            ethertype,
            payload: bytes[14..].to_vec(),
        })
    }
}
