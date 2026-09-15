use std::{fs, path::Path};

use crate::types::DynError;
pub use runinator_tui::supervisor::{ProcessSnapshot, StateSnapshot};

pub fn write_snapshot(path: &Path, snapshot: &StateSnapshot) -> Result<(), DynError> {
    let temp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(snapshot)?;
    fs::write(&temp, body)?;
    fs::rename(&temp, path)?;
    Ok(())
}

pub fn read_snapshot(path: &Path) -> Result<StateSnapshot, DynError> {
    let data = fs::read_to_string(path)?;
    let snapshot: StateSnapshot = serde_json::from_str(&data)?;
    Ok(snapshot)
}
