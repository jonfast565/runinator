//! Run on a Rust-equipped host to measure a synthetic repository, not a forecast
//! of savings for arbitrary workspaces. The legacy index comparison is a formula.
use runinator_workspace_storage::{Config, Repository, Result};
use std::fs;

fn directory_bytes(path: &std::path::Path) -> Result<u64> {
    let mut bytes = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            bytes += entry.metadata()?.len();
        }
    }
    Ok(bytes)
}
fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::init(dir.path(), Config::default())?;
    let mut tx = repo.transaction("main")?;
    let mut logical_bytes = 0u64;
    for n in 0..64u8 {
        let bytes = format!(
            "module_{n}: {{ enabled: true, notes: '{}' }}\n",
            "text ".repeat(100)
        );
        logical_bytes += bytes.len() as u64;
        tx.put(&format!("file-{n:02}.txt"), bytes.as_bytes())?;
    }
    tx.commit("synthetic small-file workload")?;
    let gc = repo.gc()?;
    repo.fsck()?;
    let stats = repo.stats()?;
    let packs = directory_bytes(&dir.path().join("packs"))?;
    let indexes = directory_bytes(&dir.path().join("indexes"))?;
    let old_index_formula = 16 + 80 * stats.objects;
    println!("logical file bytes: {logical_bytes}");
    println!("live logical objects: {}", stats.objects);
    println!("physical pack bytes: {packs}");
    println!("physical index bytes: {indexes}");
    println!("legacy 80-byte index formula for the SAME object count: {old_index_formula}");
    println!(
        "removed unreachable staging objects: {}",
        gc.removed_objects
    );
    println!("No whole-repository baseline was measured; results depend on this workload.");
    Ok(())
}
