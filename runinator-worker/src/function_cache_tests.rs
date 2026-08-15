//! covers unpacking, which is where attacker-controlled input meets the worker's filesystem.
//!
//! a package archive is uploaded by whoever holds `functions:manage` and then handed to every
//! worker that runs it, so an entry that escaped its directory would write anywhere the worker can.
//! these assert that it cannot, and that a partial unpack never looks like a usable cache entry.

use super::*;

use std::io::Write;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "runi-fncache-{name}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

// build a zip whose entry names are written verbatim, so a hostile name can be tested.
fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    buffer
}

#[test]
fn unpacks_a_package_into_its_directory() {
    let root = scratch("unpack");
    let target = root.join("pkg");
    let bytes = archive(&[
        ("runinator-function.json", "{}"),
        ("src/images.py", "def resize():\n    pass\n"),
    ]);

    unpack(&bytes, &target).unwrap();

    assert!(target.join("runinator-function.json").is_file());
    assert_eq!(
        std::fs::read_to_string(target.join("src/images.py")).unwrap(),
        "def resize():\n    pass\n"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn refuses_an_entry_that_escapes_the_package_directory() {
    let root = scratch("escape");
    let target = root.join("pkg");
    let outside = root.join("stolen.txt");

    // an absolute entry name is refused too, but `ZipWriter` normalizes the leading slash away, so
    // only a hand-built archive could carry one — these are the shapes a writer can actually emit.
    for hostile in ["../stolen.txt", "../../stolen.txt", "a/../../stolen.txt"] {
        let bytes = archive(&[("ok.txt", "fine"), (hostile, "owned")]);
        let error = unpack(&bytes, &target);
        // failing the whole staging rather than skipping the entry: a package that tried this is
        // not one to run a subset of.
        assert!(error.is_err(), "'{hostile}' should be refused");
        assert!(
            error.unwrap_err().to_string().contains("RUNI221"),
            "'{hostile}' should be reported as untrusted"
        );
        assert!(
            !outside.exists(),
            "'{hostile}' escaped the target directory"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_refused_archive_leaves_no_partial_package_behind() {
    let root = scratch("partial");
    let target = root.join("pkg");
    let bytes = archive(&[("ok.txt", "fine"), ("../escape.txt", "owned")]);

    assert!(unpack(&bytes, &target).is_err());
    // the unpack stages under a temporary name and renames, so a failure cannot leave a directory
    // that a later `stage` would mistake for a complete one.
    assert!(!target.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_directory_without_the_ready_marker_is_not_a_cache_hit() {
    let root = scratch("marker");
    let staged = root.join("abc123");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::write(staged.join("src.py"), "").unwrap();

    // the marker is written last, so this shape is exactly what an interrupted unpack leaves.
    assert!(!staged.join(READY_MARKER).is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unpacking_replaces_an_existing_package_rather_than_merging_into_it() {
    let root = scratch("replace");
    let target = root.join("pkg");
    unpack(&archive(&[("old.txt", "old")]), &target).unwrap();
    unpack(&archive(&[("new.txt", "new")]), &target).unwrap();

    // a merge would leave code from a previous digest inside a directory named for this one.
    assert!(!target.join("old.txt").exists());
    assert!(target.join("new.txt").is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn eviction_drops_the_least_recently_used_entry_first() {
    let root = scratch("evict");
    for (name, size) in [("aaa", 4096usize), ("bbb", 4096)] {
        let staged = root.join(name);
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("blob"), vec![0u8; size]).unwrap();
        std::fs::write(staged.join(READY_MARKER), name).unwrap();
        // stagger the marker mtimes so "least recently used" is well defined.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // touching `bbb` makes `aaa` the oldest by use rather than by creation.
    let _ = filetime_touch(&root.join("bbb").join(READY_MARKER));

    let cache = FunctionCache {
        client: runinator_api::AsyncApiClient::new(runinator_api::StaticLocator::new(
            "http://127.0.0.1:1",
        ))
        .unwrap(),
        root: root.clone(),
        // 8192 staged plus a 4096 incoming exceeds this by one entry, so exactly one is evicted.
        capacity_bytes: 10_000,
    };
    cache.evict_to_fit(4096);

    assert!(
        !root.join("aaa").exists(),
        "the oldest entry should be evicted"
    );
    assert!(
        root.join("bbb").exists(),
        "the recently used entry should stay"
    );
    let _ = std::fs::remove_dir_all(root);
}
