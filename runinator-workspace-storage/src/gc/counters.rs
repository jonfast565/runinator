#[allow(unused_imports)]
use super::*;

pub(super) struct Counters {
    pub(super) dir: TempDir,
    pub(super) memory: HashMap<u64, u64>,
    pub(super) memory_limit: usize,
    pub(super) table: Option<File>,
    pub(super) table_slots: usize,
    pub(super) table_entries: usize,
}

impl Counters {
    pub(super) fn new(parent: &Path) -> Result<Self> {
        Self::with_memory_limit(parent, MEMORY_COUNTER_LIMIT)
    }
    pub(super) fn with_memory_limit(parent: &Path, memory_limit: usize) -> Result<Self> {
        Ok(Self {
            dir: TempDir::new_in(parent)?,
            memory: HashMap::new(),
            memory_limit,
            table: None,
            table_slots: 0,
            table_entries: 0,
        })
    }
    pub(super) fn get(&self, n: u64) -> Result<u64> {
        let Some(table) = &self.table else {
            return Ok(self.memory.get(&n).copied().unwrap_or(0));
        };
        Ok(Self::find_slot(table, self.table_slots, n)?.1.unwrap_or(0))
    }
    pub(super) fn increment(&mut self, n: u64) -> Result<()> {
        if self.table.is_none()
            && (self.memory.contains_key(&n) || self.memory.len() < self.memory_limit)
        {
            let value = self.memory.entry(n).or_default();
            *value = value
                .checked_add(1)
                .ok_or_else(|| corrupt("link count overflow"))?;
            return Ok(());
        }
        if self.table.is_none() {
            self.spill()?;
        }
        let (mut slot, current) = Self::find_slot(
            self.table
                .as_ref()
                .ok_or_else(|| corrupt("missing counter table"))?,
            self.table_slots,
            n,
        )?;
        if current.is_none()
            && self.table_entries.saturating_add(1).saturating_mul(10) >= self.table_slots * 7
        {
            self.grow()?;
            slot = Self::find_slot(
                self.table
                    .as_ref()
                    .ok_or_else(|| corrupt("missing counter table"))?,
                self.table_slots,
                n,
            )?
            .0;
        }
        let value = current
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| corrupt("link count overflow"))?;
        Self::write_slot(
            self.table
                .as_mut()
                .ok_or_else(|| corrupt("missing counter table"))?,
            slot,
            n,
            value,
        )?;
        if current.is_none() {
            self.table_entries += 1;
        }
        Ok(())
    }
    pub(super) fn slot(n: u64, slots: usize) -> usize {
        n.wrapping_mul(0x9e37_79b9_7f4a_7c15) as usize & (slots - 1)
    }
    pub(super) fn find_slot(file: &File, slots: usize, n: u64) -> Result<(usize, Option<u64>)> {
        let mut slot = Self::slot(n, slots);
        for _ in 0..slots {
            let mut bytes = [0; COUNTER_SLOT_BYTES as usize];
            crate::io_util::read_at(file, &mut bytes, slot as u64 * COUNTER_SLOT_BYTES)?;
            if bytes[0] == 0 {
                return Ok((slot, None));
            }
            if u64::from_le_bytes(bytes[1..9].try_into().unwrap()) == n {
                return Ok((
                    slot,
                    Some(u64::from_le_bytes(bytes[9..17].try_into().unwrap())),
                ));
            }
            slot = (slot + 1) & (slots - 1);
        }
        Err(corrupt("counter table is full"))
    }
    pub(super) fn write_slot(file: &mut File, slot: usize, n: u64, value: u64) -> Result<()> {
        let mut bytes = [0; COUNTER_SLOT_BYTES as usize];
        bytes[0] = 1;
        bytes[1..9].copy_from_slice(&n.to_le_bytes());
        bytes[9..17].copy_from_slice(&value.to_le_bytes());
        file.seek(SeekFrom::Start(slot as u64 * COUNTER_SLOT_BYTES))?;
        file.write_all(&bytes)?;
        Ok(())
    }
    pub(super) fn spill(&mut self) -> Result<()> {
        self.table_slots =
            (self.memory_limit.max(self.memory.len()).max(1) * 2).next_power_of_two();
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(self.table_slots as u64 * COUNTER_SLOT_BYTES)?;
        for (&n, &value) in &self.memory {
            let (slot, _) = Self::find_slot(&table, self.table_slots, n)?;
            Self::write_slot(&mut table, slot, n, value)?;
        }
        self.table_entries = self.memory.len();
        self.memory.clear();
        self.table = Some(table);
        Ok(())
    }
    pub(super) fn grow(&mut self) -> Result<()> {
        let old_slots = self.table_slots;
        let new_slots = old_slots
            .checked_mul(2)
            .ok_or_else(|| corrupt("counter table size overflow"))?;
        let old = self
            .table
            .take()
            .ok_or_else(|| corrupt("missing counter table"))?;
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(new_slots as u64 * COUNTER_SLOT_BYTES)?;
        for slot in 0..old_slots {
            let mut bytes = [0; COUNTER_SLOT_BYTES as usize];
            crate::io_util::read_at(&old, &mut bytes, slot as u64 * COUNTER_SLOT_BYTES)?;
            if bytes[0] == 0 {
                continue;
            }
            let n = u64::from_le_bytes(bytes[1..9].try_into().unwrap());
            let value = u64::from_le_bytes(bytes[9..17].try_into().unwrap());
            let (slot, _) = Self::find_slot(&table, new_slots, n)?;
            Self::write_slot(&mut table, slot, n, value)?;
        }
        self.table_slots = new_slots;
        self.table = Some(table);
        Ok(())
    }
}
