pub const FLAG_FIN: u8 = 0x01;
pub const FLAG_SYN: u8 = 0x02;
pub const FLAG_ACK: u8 = 0x10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TcpPacket {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub payload: Vec<u8>,
}

impl TcpPacket {
    pub fn encode(&self) -> Vec<u8> {
        let header_len = 20usize;
        let mut bytes = Vec::with_capacity(header_len + self.payload.len());
        bytes.extend_from_slice(&self.src_port.to_be_bytes());
        bytes.extend_from_slice(&self.dst_port.to_be_bytes());
        bytes.extend_from_slice(&self.seq.to_be_bytes());
        bytes.extend_from_slice(&self.ack.to_be_bytes());
        bytes.push((5u8) << 4);
        bytes.push(self.flags);
        bytes.extend_from_slice(&self.window.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 20 {
            return None;
        }
        let header_len = ((bytes[12] >> 4) as usize) * 4;
        if header_len < 20 || bytes.len() < header_len {
            return None;
        }
        Some(Self {
            src_port: u16::from_be_bytes(bytes[0..2].try_into().ok()?),
            dst_port: u16::from_be_bytes(bytes[2..4].try_into().ok()?),
            seq: u32::from_be_bytes(bytes[4..8].try_into().ok()?),
            ack: u32::from_be_bytes(bytes[8..12].try_into().ok()?),
            flags: bytes[13],
            window: u16::from_be_bytes(bytes[14..16].try_into().ok()?),
            payload: bytes[header_len..].to_vec(),
        })
    }
}
