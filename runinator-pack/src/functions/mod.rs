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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
