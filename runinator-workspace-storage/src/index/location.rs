#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct Location {
    pub id: Id,
    pub pack: Id,
    pub offset: u64,
    pub length: u64,
    /// STANDALONE, or a member slot in a physical TinyBlock record.
    pub member: u32,
}

impl Location {
    pub fn bytes(self) -> [u8; 84] {
        let mut b = [0; 84];
        b[..32].copy_from_slice(&self.id.0);
        b[32..64].copy_from_slice(&self.pack.0);
        b[64..72].copy_from_slice(&self.offset.to_le_bytes());
        b[72..80].copy_from_slice(&self.length.to_le_bytes());
        b[80..84].copy_from_slice(&self.member.to_le_bytes());
        b
    }
    pub(super) fn decode(b: [u8; 84]) -> Self {
        Self {
            id: Id(b[..32].try_into().unwrap()),
            pack: Id(b[32..64].try_into().unwrap()),
            offset: u64::from_le_bytes(b[64..72].try_into().unwrap()),
            length: u64::from_le_bytes(b[72..80].try_into().unwrap()),
            member: u32::from_le_bytes(b[80..84].try_into().unwrap()),
        }
    }
}
