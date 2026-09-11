//! measure the full native workspace lifecycle against a real directory.

use runinator_models::workspaces::WorkspaceLimits;
use runinator_workspace::{
    native, revision,
    storage::{
        packs,
        staging::{EmptyStore, Staging},
    },
};
use std::{collections::BTreeMap, path::Path, time::Instant};

fn elapsed(label: &str, started: Instant) {
    eprintln!("{label}: {:.3}s", started.elapsed().as_secs_f64());
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let source = std::env::args()
        .nth(1)
        .ok_or("usage: workspace_perf <directory>")?;
    let source = Path::new(&source);
    let scratch = tempfile::tempdir()?;
    let total = Instant::now();

    let started = Instant::now();
    let staging = Staging::new(EmptyStore, scratch.path())?;
    let (edit, usage) = revision::capture(
        staging,
        source,
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        scratch.path(),
    )?;
    let revision = edit.finish("benchmark", None)?;
    elapsed("capture", started);

    let started = Instant::now();
    let mut pack_bytes = 0;
    let mut pack_count = 0;
    packs::seal(&edit.store, &EmptyStore, revision, scratch.path(), |pack| {
        pack_bytes += std::fs::metadata(pack.path)?.len();
        pack_count += 1;
        Ok(())
    })?;
    elapsed("seal", started);

    let started = Instant::now();
    let mut archive = Vec::new();
    native::export(&edit.store, revision, scratch.path(), &mut archive)?;
    elapsed("native export", started);

    let started = Instant::now();
    let (packed, imported, _) = native::import_packed(
        archive.as_slice(),
        scratch.path(),
        WorkspaceLimits::default(),
    )?;
    assert_eq!(imported, revision);
    elapsed("native import/index/validate", started);

    let started = Instant::now();
    let delta_scratch = tempfile::tempdir()?;
    let staging = Staging::new_deduplicating(&packed, delta_scratch.path())?;
    let (delta, _) = revision::capture_from(
        staging,
        Some(revision),
        source,
        &BTreeMap::new(),
        WorkspaceLimits::default(),
        delta_scratch.path(),
    )?;
    let delta_revision = delta.finish("delta benchmark", Some(revision))?;
    elapsed("unchanged delta capture", started);

    let started = Instant::now();
    let mut delta_bytes = 0;
    packs::seal(
        &delta.store,
        &packed,
        delta_revision,
        delta_scratch.path(),
        |pack| {
            delta_bytes += std::fs::metadata(pack.path)?.len();
            Ok(())
        },
    )?;
    elapsed("delta seal", started);

    let started = Instant::now();
    let destination = tempfile::tempdir()?;
    let view = runinator_workspace::storage::view::View::new(&packed, revision)?;
    revision::materialize(&view, destination.path())?;
    elapsed("materialize", started);

    println!(
        "revision={revision} entries={} logical_bytes={} objects={} packs={} pack_bytes={} archive_bytes={} delta_bytes={} total={:.3}s",
        usage.entries,
        usage.logical_bytes,
        packed.object_count(),
        pack_count,
        pack_bytes,
        archive.len(),
        delta_bytes,
        total.elapsed().as_secs_f64(),
    );
    Ok(())
}
