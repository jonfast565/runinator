//! Run with: cargo run --example workspace
use runinator_workspace_storage::{Config, Repository, Result, pages};
use std::io::{self, Read};
fn main() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let repo = Repository::init(directory.path(), Config::default())?;
    let page_size = runinator_workspace_storage::Layout::default().page_size as u64;
    let mut transaction = repo.transaction("main")?;
    transaction.mkdir_all("audio/takes")?;
    // No multi-megabyte input Vec: this reader generates bytes incrementally.
    transaction.put(
        "audio/takes/first.raw",
        io::repeat(0x2a).take(3 * page_size + 400),
    )?;
    transaction.hard_link("audio/takes/first.raw", "audio/current.raw")?;
    let first_revision = transaction.commit("initial recording")?;
    let old = repo.snapshot_at(first_revision)?;
    let old_file = old.file_id("audio/takes/first.raw")?;
    let mut transaction = repo.transaction("main")?;
    // An overwrite, NOT a byte insertion. Only page 1 must be rechunked.
    transaction.write("audio/current.raw", page_size + 10, b"changed samples")?;
    let second_revision = transaction.commit("edit through hard link")?;
    let current = repo.snapshot_at(second_revision)?;
    let new_file = current.file_id("audio/takes/first.raw")?;
    assert_eq!(
        pages::page_id(&old, old_file, 0)?,
        pages::page_id(&current, new_file, 0)?
    );
    assert_eq!(
        pages::page_id(&old, old_file, 2)?,
        pages::page_id(&current, new_file, 2)?
    );
    assert_ne!(
        pages::page_id(&old, old_file, 1)?,
        pages::page_id(&current, new_file, 1)?
    );
    assert_eq!(
        current.read_range("audio/takes/first.raw", page_size + 10, 15)?,
        b"changed samples"
    );
    assert_eq!(
        old.read_range("audio/takes/first.raw", page_size + 10, 15)?,
        vec![0x2a; 15]
    );
    println!("first revision:  {first_revision}");
    println!("second revision: {second_revision}");
    println!("page cache: {:?}", repo.page_cache().stats()?);
    drop(current);
    drop(old);
    // Active snapshots/transactions intentionally block GC.
    println!("verified reachable objects: {}", repo.fsck()?);
    println!("compaction: {:?}", repo.gc()?);
    Ok(())
}
