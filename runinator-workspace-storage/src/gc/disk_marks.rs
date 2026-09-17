#[allow(unused_imports)]
use super::*;

pub struct DiskMarks {
    pub(super) dir: TempDir,
    pub(super) memory: HashSet<Id>,
    pub(super) memory_limit: usize,
    pub(super) table: Option<File>,
    pub(super) table_slots: usize,
    pub count: u64,
}

impl DiskMarks {
    pub fn new(parent: &Path) -> Result<Self> {
        Self::with_memory_limit(parent, MEMORY_MARK_LIMIT)
    }
    pub(super) fn with_memory_limit(parent: &Path, memory_limit: usize) -> Result<Self> {
        Ok(Self {
            dir: TempDir::new_in(parent)?,
            memory: HashSet::new(),
            memory_limit,
            table: None,
            table_slots: 0,
            count: 0,
        })
    }
    pub fn contains(&self, id: Id) -> Result<bool> {
        if self.table.is_none() {
            return Ok(self.memory.contains(&id));
        }
        self.contains_disk(id)
    }
    pub fn insert(&mut self, id: Id) -> Result<bool> {
        if self.table.is_none() && self.memory.len() < self.memory_limit {
            if self.memory.insert(id) {
                self.count += 1;
                return Ok(true);
            }
            return Ok(false);
        }
        if self.table.is_none() {
            self.spill()?;
        }
        if self.count.saturating_add(1).saturating_mul(10) >= self.table_slots as u64 * 7 {
            if self.contains_disk(id)? {
                return Ok(false);
            }
            self.grow()?;
        }
        if self.insert_disk(id)? {
            self.count += 1;
            return Ok(true);
        }
        Ok(false)
    }
    pub(super) fn slot(id: Id, slots: usize) -> usize {
        let hash = u64::from_le_bytes(id.0[..8].try_into().unwrap()) as usize;
        hash & (slots - 1)
    }
    pub(super) fn contains_disk(&self, id: Id) -> Result<bool> {
        let file = self
            .table
            .as_ref()
            .ok_or_else(|| corrupt("missing mark table"))?;
        let mut slot = Self::slot(id, self.table_slots);
        for _ in 0..self.table_slots {
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(file, &mut bytes, slot as u64 * MARK_SLOT_BYTES)?;
            if bytes[0] == 0 {
                return Ok(false);
            }
            if bytes[1..] == id.0 {
                return Ok(true);
            }
            slot = (slot + 1) & (self.table_slots - 1);
        }
        Ok(false)
    }
    pub(super) fn insert_into(file: &mut File, slots: usize, id: Id) -> Result<bool> {
        let mut slot = Self::slot(id, slots);
        for _ in 0..slots {
            let offset = slot as u64 * MARK_SLOT_BYTES;
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(file, &mut bytes, offset)?;
            if bytes[0] == 0 {
                bytes[0] = 1;
                bytes[1..].copy_from_slice(&id.0);
                file.seek(SeekFrom::Start(offset))?;
                file.write_all(&bytes)?;
                return Ok(true);
            }
            if bytes[1..] == id.0 {
                return Ok(false);
            }
            slot = (slot + 1) & (slots - 1);
        }
        Err(corrupt("mark table is full"))
    }
    pub(super) fn insert_disk(&mut self, id: Id) -> Result<bool> {
        Self::insert_into(
            self.table
                .as_mut()
                .ok_or_else(|| corrupt("missing mark table"))?,
            self.table_slots,
            id,
        )
    }
    pub(super) fn spill(&mut self) -> Result<()> {
        let slots = self.memory_limit.max(self.memory.len()).max(1) * 2;
        self.table_slots = slots.next_power_of_two();
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(self.table_slots as u64 * MARK_SLOT_BYTES)?;
        for &id in &self.memory {
            Self::insert_into(&mut table, self.table_slots, id)?;
        }
        self.memory.clear();
        self.table = Some(table);
        Ok(())
    }
    pub(super) fn grow(&mut self) -> Result<()> {
        let old_slots = self.table_slots;
        let slots = old_slots
            .checked_mul(2)
            .ok_or_else(|| corrupt("mark table size overflow"))?;
        let old = self
            .table
            .take()
            .ok_or_else(|| corrupt("missing mark table"))?;
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(slots as u64 * MARK_SLOT_BYTES)?;
        for slot in 0..old_slots {
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(&old, &mut bytes, slot as u64 * MARK_SLOT_BYTES)?;
            if bytes[0] != 0 {
                Self::insert_into(&mut table, slots, Id(bytes[1..].try_into().unwrap()))?;
            }
        }
        self.table_slots = slots;
        self.table = Some(table);
        Ok(())
    }
    pub fn visit<F: FnMut(Id) -> Result<()>>(&self, mut f: F) -> Result<()> {
        let Some(table) = &self.table else {
            for &id in &self.memory {
                f(id)?;
            }
            return Ok(());
        };
        for slot in 0..self.table_slots {
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(table, &mut bytes, slot as u64 * MARK_SLOT_BYTES)?;
            if bytes[0] != 0 {
                f(Id(bytes[1..].try_into().unwrap()))?;
            }
        }
        Ok(())
    }
}
