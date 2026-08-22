//! compiles every `.rrx` workflow source shipped in `packs/` so the repository's own packs are
//! held to the language the compiler actually accepts.

use runinator_rexrap::{CompileOptions, compile_all_str, parse_rrx_blocks};
use std::path::{Path, PathBuf};

fn pack_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("packs");
    let mut out = Vec::new();
    collect(&root, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rrx") {
            out.push(path);
        }
    }
}

#[test]
fn every_pack_source_compiles() {
    let options = CompileOptions::default();
    let mut failures = Vec::new();
    for path in pack_sources() {
        let src = std::fs::read_to_string(&path).expect("read pack source");
        let blocks = match parse_rrx_blocks(&src) {
            Ok(blocks) => blocks,
            Err(err) => {
                failures.push(format!("{}: {err}", path.display()));
                continue;
            }
        };
        // a settings- or pipeline-only source carries no workflow block; nothing to compile.
        if blocks.workflows.trim().is_empty() {
            continue;
        }
        if let Err(err) = compile_all_str(&blocks.workflows, &options) {
            failures.push(format!("{}: {err}", path.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "packs failed to compile:\n{}",
        failures.join("\n")
    );
}
