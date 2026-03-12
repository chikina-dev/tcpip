#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UdpPacket {
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: Vec<u8>,
}

impl UdpPacket {
    pub fn encode(&self) -> Vec<u8> {
        let len = 8 + self.payload.len();
        let mut bytes = Vec::with_capacity(len);
        bytes.extend_from_slice(&self.src_port.to_be_bytes());
        bytes.extend_from_slice(&self.dst_port.to_be_bytes());
        bytes.extend_from_slice(&(len as u16).to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        let len = u16::from_be_bytes(bytes[4..6].try_into().ok()?) as usize;
        if bytes.len() < len || len < 8 {
            return None;
        }
        Some(Self {
            src_port: u16::from_be_bytes(bytes[0..2].try_into().ok()?),
            dst_port: u16::from_be_bytes(bytes[2..4].try_into().ok()?),
            payload: bytes[8..len].to_vec(),
        })
    }
}
