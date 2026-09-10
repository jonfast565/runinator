//! Single-spool staging behavior.

use super::*;
use crate::store::MemoryStore;

#[test]
fn stages_many_objects_in_one_physical_file() -> Result<()> {
    let scratch = tempfile::tempdir()?;
    let stage = Staging::new(EmptyStore, scratch.path())?;
    let mut ids = Vec::new();
    for value in 0..1_000u32 {
        ids.push(stage.put(Kind::File, &value.to_le_bytes())?);
    }
    let state = stage.state.lock().map_err(|_| Error::Poisoned)?;
    assert_eq!(state.objects.len(), 1_000);
    assert!(state.file.metadata()?.len() > 0);
    drop(state);
    for (value, id) in (0..1_000u32).zip(ids) {
        assert_eq!(&*stage.get(id)?.bytes, &value.to_le_bytes());
    }
    Ok(())
}

#[test]
fn complete_local_base_can_be_deduplicated_without_staging() -> Result<()> {
    let base = MemoryStore::default();
    let id = base.put(Kind::File, b"already present")?;
    let scratch = tempfile::tempdir()?;
    let stage = Staging::new_deduplicating(base, scratch.path())?;
    assert_eq!(stage.put(Kind::File, b"already present")?, id);
    assert!(!stage.is_staged(id)?);
    assert_eq!(&*stage.get(id)?.bytes, b"already present");
    Ok(())
}
