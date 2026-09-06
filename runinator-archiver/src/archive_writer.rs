//! compressed archive publication before source-row deletion.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use chrono::Utc;
use flate2::{Compression, write::GzEncoder};
use runinator_models::errors::SendableError;
use runinator_store::archive::{ArchiveRow, ArchiveTable};
use serde_json::json;
use tracing::info;
use uuid::Uuid;

pub(crate) const ARCHIVE_FILE_EXTENSION: &str = "jsonl.gz";

pub(crate) fn write_archive_jsonl_files(
    root: &Path,
    rows: &[ArchiveRow],
) -> Result<(), SendableError> {
    let mut groups = BTreeMap::<(String, ArchiveTable), Vec<&ArchiveRow>>::new();
    for row in rows {
        groups
            .entry((row.created_at.format("%F").to_string(), row.table))
            .or_default()
            .push(row);
    }
    for ((day, table), rows) in groups {
        let dir = root.join(&day);
        create_archive_directory(&dir)?;
        let final_path = dir.join(format!(
            "{table}-{}.{}",
            Uuid::new_v4(),
            ARCHIVE_FILE_EXTENSION
        ));
        let tmp_path = temp_path(&final_path);
        let file = File::create(&tmp_path)?;
        let mut encoder = GzEncoder::new(BufWriter::new(file), Compression::default());
        let archived_at = Utc::now().to_rfc3339();
        for row in rows {
            let line = json!({
                "schema_version": 1,
                "archived_at": archived_at,
                "source_table": row.table.as_str(),
                "primary_key": { "id": row.primary_key.to_string() },
                "created_at": row.created_at.timestamp(),
                "row": row.row,
            });
            serde_json::to_writer(&mut encoder, &line)?;
            encoder.write_all(b"\n")?;
        }
        let file = finish_archive(encoder)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp_path, &final_path)?;
        // unix permits syncing directory entries; other platforms still flush and sync file data.
        #[cfg(unix)]
        File::open(&dir)?.sync_all()?;
        info!(path = %final_path.display(), table = %table, "wrote archive file");
    }
    Ok(())
}

pub(crate) fn create_archive_directory(path: &Path) -> io::Result<()> {
    // include newly created ancestors and their existing parent in the durability boundary.
    let mut directories = vec![path.to_path_buf()];
    let mut cursor = path;
    while !cursor.try_exists()? {
        let parent = cursor
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        directories.push(parent.to_path_buf());
        cursor = parent;
    }
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    for directory in directories {
        File::open(directory)?.sync_all()?;
    }
    Ok(())
}

fn finish_archive<W: Write>(encoder: GzEncoder<BufWriter<W>>) -> io::Result<W> {
    let mut writer = encoder.finish()?;
    writer.flush()?;
    writer.into_inner().map_err(|error| error.into_error())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".tmp");
    PathBuf::from(value)
}

#[cfg(test)]
#[path = "archive_writer_tests.rs"]
mod tests;
