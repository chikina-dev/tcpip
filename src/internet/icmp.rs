#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IcmpKind {
    EchoReply = 0,
    EchoRequest = 8,
}

impl IcmpKind {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::EchoReply),
            8 => Some(Self::EchoRequest),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcmpPacket {
    pub kind: IcmpKind,
    pub identifier: u16,
    pub sequence: u16,
    pub payload: Vec<u8>,
}

impl IcmpPacket {
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + self.payload.len());
        bytes.push(self.kind as u8);
        bytes.push(0);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&self.identifier.to_be_bytes());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        Some(Self {
            kind: IcmpKind::from_u8(bytes[0])?,
            identifier: u16::from_be_bytes(bytes[4..6].try_into().ok()?),
            sequence: u16::from_be_bytes(bytes[6..8].try_into().ok()?),
            payload: bytes[8..].to_vec(),
        })
    }
}
