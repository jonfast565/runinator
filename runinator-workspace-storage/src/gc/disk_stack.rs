#[allow(unused_imports)]
use super::*;

pub(super) struct DiskStack<const N: usize> {
    pub(super) file: File,
    pub(super) memory: Vec<[u8; N]>,
    pub(super) memory_limit: usize,
    pub(super) spilled: bool,
    pub(super) count: u64,
}

impl<const N: usize> DiskStack<N> {
    pub(super) fn new(parent: &Path) -> Result<Self> {
        Self::with_memory_limit(parent, MEMORY_STACK_LIMIT)
    }
    pub(super) fn with_memory_limit(parent: &Path, memory_limit: usize) -> Result<Self> {
        Ok(Self {
            file: tempfile::tempfile_in(parent)?,
            memory: Vec::new(),
            memory_limit,
            spilled: false,
            count: 0,
        })
    }
    pub(super) fn push(&mut self, bytes: [u8; N]) -> Result<()> {
        if !self.spilled && self.memory.len() < self.memory_limit {
            self.memory.push(bytes);
            return Ok(());
        }
        if !self.spilled {
            for item in self.memory.drain(..) {
                self.file.write_all(&item)?;
                self.count += 1;
            }
            self.spilled = true;
        }
        let offset = self
            .count
            .checked_mul(N as u64)
            .ok_or_else(|| corrupt("traversal stack overflow"))?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&bytes)?;
        self.count += 1;
        Ok(())
    }
    pub(super) fn pop(&mut self) -> Result<Option<[u8; N]>> {
        if !self.spilled {
            return Ok(self.memory.pop());
        }
        if self.count == 0 {
            return Ok(None);
        }
        self.count -= 1;
        self.file.seek(SeekFrom::Start(self.count * N as u64))?;
        let mut bytes = [0; N];
        self.file.read_exact(&mut bytes)?;
        Ok(Some(bytes))
    }
}
