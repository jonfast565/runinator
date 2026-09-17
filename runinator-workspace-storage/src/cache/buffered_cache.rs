#[allow(unused_imports)]
use super::*;

pub struct BufferedCache {
    pub(super) capacity: usize,
    pub(super) state: Mutex<(usize, HashMap<Id, Object>)>,
}

impl BufferedCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new((0, HashMap::new())),
        }
    }

    pub(super) fn retain(&self, id: Id, object: &Object) -> Result<()> {
        let size = object.bytes.len().saturating_add(128);
        if size <= self.capacity {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if !state.1.contains_key(&id) {
                while state.0 > self.capacity - size {
                    let key = state.1.keys().next().copied().ok_or(Error::Poisoned)?;
                    if let Some(evicted) = state.1.remove(&key) {
                        state.0 -= evicted.bytes.len().saturating_add(128);
                    }
                }
                state.0 += size;
                state.1.insert(id, object.clone());
            }
        }
        Ok(())
    }
}
