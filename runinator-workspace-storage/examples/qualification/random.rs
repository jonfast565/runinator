#[allow(unused_imports)]
use super::*;

pub(super) struct Random {
    pub(super) remaining: u64,
    pub(super) state: u64,
}

impl Read for Random {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let n = self.remaining.min(output.len() as u64) as usize;
        for chunk in output[..n].chunks_mut(8) {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            chunk.copy_from_slice(&self.state.to_le_bytes()[..chunk.len()]);
        }
        self.remaining -= n as u64;
        Ok(n)
    }
}
