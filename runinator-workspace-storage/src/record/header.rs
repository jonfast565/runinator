#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub id: Id,
    pub kind: Kind,
    pub codec: u8,
    pub raw_len: u64,
    pub encoded_len: u64,
    pub checksum: [u8; 32],
}

impl Header {
    pub fn record_len(&self) -> Result<u64> {
        HEADER_LEN
            .checked_add(self.encoded_len)
            .ok_or_else(|| corrupt("record length overflow"))
    }
    pub fn info(&self) -> ObjectInfo {
        ObjectInfo {
            kind: self.kind,
            raw_len: self.raw_len as usize,
        }
    }
    pub fn encode(&self) -> [u8; 96] {
        let mut b = [0; 96];
        b[..8].copy_from_slice(RECORD_MAGIC);
        b[8] = self.kind as u8;
        b[9] = self.codec;
        b[16..24].copy_from_slice(&self.raw_len.to_le_bytes());
        b[24..32].copy_from_slice(&self.encoded_len.to_le_bytes());
        b[32..64].copy_from_slice(&self.id.0);
        b[64..96].copy_from_slice(&self.checksum);
        b
    }
    pub fn decode(b: [u8; 96]) -> Result<Self> {
        if &b[..8] != RECORD_MAGIC || b[10..16] != [0; 6] {
            return Err(corrupt("bad record header"));
        }
        let x = Self {
            id: Id(b[32..64].try_into().unwrap()),
            kind: b[8].try_into()?,
            codec: b[9],
            raw_len: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            encoded_len: u64::from_le_bytes(b[24..32].try_into().unwrap()),
            checksum: b[64..96].try_into().unwrap(),
        };
        if x.codec > 1
            || x.raw_len > MAX_OBJECT as u64
            || x.encoded_len > MAX_OBJECT as u64 + 65536
            || (x.codec == 0 && x.raw_len != x.encoded_len)
            || (x.kind == Kind::TinyBlock
                && (x.raw_len > crate::tiny::BLOCK_LIMIT as u64
                    || x.encoded_len > crate::tiny::BLOCK_LIMIT as u64 + 65536))
            || (x.kind == Kind::MetadataBlock
                && (x.raw_len > crate::metadata_block::BLOCK_LIMIT as u64
                    || x.encoded_len > crate::metadata_block::BLOCK_LIMIT as u64 + 65536))
            || (x.kind == Kind::ChunkBlock && x.raw_len > crate::codec::MAX_OBJECT as u64)
        {
            return Err(corrupt("invalid record sizes/codec"));
        }
        Ok(x)
    }
}
