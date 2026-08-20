//! function packages: the client-side half of publishing packaged code.
//!
//! a package is a directory holding a [`manifest::MANIFEST_FILE`] plus whatever code it ships. this
//! module reads the manifest, archives the directory deterministically, and produces the publish
//! request the web service records. as with workflow packs, everything here runs on the client —
//! the backend only ever sees an archive it can address by digest and a request describing it.

mod archive;
mod glob;
mod manifest;

pub use archive::{DEFAULT_EXCLUDES, FunctionArchive, archive_contents, archive_directory};
pub use manifest::{FunctionManifest, MANIFEST_FILE};

use std::path::Path;

use runinator_models::functions::NewFunctionVersion;

use crate::errors::Result;

/// a package directory read and archived: everything a publish needs.
#[derive(Debug, Clone)]
pub struct FunctionSource {
    pub manifest: FunctionManifest,
    pub archive: FunctionArchive,
}

impl FunctionSource {
    /// read, validate, and archive a package directory.
    pub fn load(directory: &Path) -> Result<Self> {
        let manifest = FunctionManifest::load(directory)?;
        let archive = archive_directory(directory, &manifest.exclude)?;
        Ok(Self { manifest, archive })
    }

    /// the publish request for this source's archive.
    pub fn publish_request(&self) -> NewFunctionVersion {
        self.manifest.publish_request(&self.archive.digest)
    }

    /// the fully qualified package name, `namespace.name` or just `name`.
    pub fn qualified_name(&self) -> String {
        match &self.manifest.namespace {
            Some(namespace) => format!("{namespace}.{}", self.manifest.name),
            None => self.manifest.name.clone(),
        }
    }
}

/// true when a directory looks like a function package.
pub fn is_function_source(path: &Path) -> bool {
    path.is_dir() && path.join(MANIFEST_FILE).is_file()
}

/// find the function packages a pack carries.
///
/// a pack directory may hold function packages beside its `.rexrap` files — each is a subdirectory with
/// its own manifest. searched one level deep only: a package's *own* subdirectories are its code,
/// and recursing into them would treat a vendored dependency that happened to carry a manifest as a
/// second package to publish.
pub fn discover_function_sources(pack_root: &Path) -> Result<Vec<FunctionSource>> {
    let root = if pack_root.is_dir() {
        pack_root.to_path_buf()
    } else {
        match pack_root.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Ok(Vec::new()),
        }
    };

    // the pack root may itself be one package.
    if is_function_source(&root) {
        return Ok(vec![FunctionSource::load(&root)?]);
    }

    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_function_source(path))
        .collect();
    // sorted so a pack compiles to the same publish order every time, which keeps an apply
    // reproducible and its logs diffable.
    paths.sort();

    paths
        .iter()
        .map(|path| FunctionSource::load(path))
        .collect()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
