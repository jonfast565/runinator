//! Native OCI layout transfers containing one selected revision closure.

use crate::storage::{
    self, Id, packs,
    staging::{EmptyStore, Staging},
    store::{ReadStore, WriteStore},
    view::View,
};
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    workspaces::{WorkspaceLimits, WorkspaceUsage},
};
use std::{
    io::{Read, Write},
    path::Path,
};

const CONFIG: &str = "application/vnd.runinator.workspace.config.v1+json";
const PACK: &str = "application/vnd.runinator.workspace.pack.v1";
const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

pub(crate) fn append<W: Write>(
    tar: &mut tar::Builder<W>,
    path: &str,
    length: u64,
    reader: impl Read,
) -> Result<(), SendableError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(length);
    header.set_mode(0o600);
    header.set_cksum();
    tar.append_data(&mut header, path, reader)?;
    Ok(())
}

pub(crate) fn json_blob<W: Write>(
    tar: &mut tar::Builder<W>,
    value: serde_json::Value,
    media: &str,
) -> Result<serde_json::Value, SendableError> {
    let bytes = serde_json::to_vec(&value)?;
    let digest = crate::digest(&bytes);
    append(
        tar,
        &format!("blobs/sha256/{digest}"),
        bytes.len() as u64,
        bytes.as_slice(),
    )?;
    Ok(
        serde_json::json!({"mediaType": media, "digest": format!("sha256:{digest}"), "size": bytes.len()}),
    )
}

pub fn export<S: ReadStore, W: Write>(
    store: &S,
    revision: Id,
    scratch: &Path,
    output: W,
) -> Result<(), SendableError> {
    let mut archive = tar::Builder::new(output);
    let mut layers = Vec::new();
    packs::seal(store, &EmptyStore, revision, scratch, |pack| {
        let file = std::fs::File::open(&pack.path)?;
        let length = file.metadata()?.len();
        append(
            &mut archive,
            &format!("blobs/sha256/{}", pack.id),
            length,
            file,
        )
        .map_err(|error| storage::Error::Io(std::io::Error::other(error)))?;
        layers.push(serde_json::json!({"mediaType": PACK, "digest": format!("sha256:{}", pack.id), "size": length}));
        Ok(())
    })?;
    let config = json_blob(
        &mut archive,
        serde_json::json!({"format_version":1,"revision_id":revision.to_string()}),
        CONFIG,
    )?;
    let manifest = json_blob(
        &mut archive,
        serde_json::json!({"schemaVersion":2,"mediaType":MANIFEST,"config":config,"layers":layers}),
        MANIFEST,
    )?;
    let index = serde_json::to_vec(&serde_json::json!({"schemaVersion":2,"manifests":[manifest]}))?;
    append(
        &mut archive,
        "index.json",
        index.len() as u64,
        index.as_slice(),
    )?;
    let layout = b"{\"imageLayoutVersion\":\"1.0.0\"}";
    append(
        &mut archive,
        "oci-layout",
        layout.len() as u64,
        layout.as_slice(),
    )?;
    archive.finish()?;
    Ok(())
}

pub fn import<R: Read>(
    input: R,
    scratch: &Path,
    limits: WorkspaceLimits,
) -> Result<(Staging<EmptyStore>, Id, WorkspaceUsage), SendableError> {
    let (store, revision, usage, _) = import_with_format(input, scratch, limits)?;
    Ok((store, revision, usage))
}

/// Accept one native workspace or conventional OCI image manifest.
pub fn import_with_format<R: Read>(
    input: R,
    scratch: &Path,
    limits: WorkspaceLimits,
) -> Result<(Staging<EmptyStore>, Id, WorkspaceUsage, &'static str), SendableError> {
    limits.validate()?;
    let directory = tempfile::tempdir_in(scratch)?;
    let transfer_limit = limits
        .max_bytes
        .checked_mul(3)
        .and_then(|n| {
            limits
                .max_entries
                .checked_mul(8192)
                .and_then(|entries| n.checked_add(entries))
        })
        .and_then(|n| n.checked_add(128 * 1024 * 1024))
        .ok_or_else(|| WORKSPACE_INVALID.error("import scratch budget overflow"))?;
    let mut total = 0u64;
    let mut paths = std::collections::BTreeSet::new();
    let mut archive = tar::Archive::new(input);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let normalized = path
            .to_str()
            .ok_or_else(|| WORKSPACE_INVALID.error("OCI paths must be UTF-8"))?
            .trim_start_matches("./");
        if entry.header().entry_type().is_dir()
            && ["", ".", "blobs", "blobs/", "blobs/sha256", "blobs/sha256/"].contains(&normalized)
        {
            continue;
        }
        let path = std::path::PathBuf::from(normalized);
        let name = path
            .to_str()
            .ok_or_else(|| WORKSPACE_INVALID.error("invalid native archive path"))?;
        let blob = name
            .strip_prefix("blobs/sha256/")
            .and_then(|name| name.parse::<Id>().ok());
        if !entry.header().entry_type().is_file()
            || (name != "index.json" && name != "oci-layout" && blob.is_none())
            || !paths.insert(path.clone())
        {
            return Err(WORKSPACE_INVALID.error("unexpected or duplicate native archive entry"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| WORKSPACE_INVALID.error("archive size overflow"))?;
        if total > transfer_limit || (blob.is_none() && entry.size() > 1024 * 1024) {
            return Err(WORKSPACE_INVALID.error("native archive exceeds transfer budget"));
        }
        let destination = directory.path().join(&path);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        std::io::copy(&mut entry, &mut file)?;
        if let Some(expected) = blob {
            use sha2::{Digest, Sha256};
            let mut file = std::fs::File::open(destination)?;
            let mut hasher = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            if Id(hasher.finalize().into()) != expected {
                return Err(WORKSPACE_INVALID.error("native blob checksum mismatch"));
            }
        }
    }
    fn json(path: &Path) -> Result<serde_json::Value, SendableError> {
        if std::fs::metadata(path)?.len() > 4 * 1024 * 1024 {
            return Err(WORKSPACE_INVALID.error("OCI metadata is too large"));
        }
        Ok(serde_json::from_reader(std::io::BufReader::new(
            std::fs::File::open(path)?,
        ))?)
    }
    fn descriptor(
        root: &Path,
        value: &serde_json::Value,
        media: &str,
    ) -> Result<std::path::PathBuf, SendableError> {
        if value["mediaType"] != media {
            return Err(WORKSPACE_INVALID.error("unsupported OCI media type"));
        }
        let id = value["digest"]
            .as_str()
            .and_then(|s| s.strip_prefix("sha256:"))
            .ok_or_else(|| WORKSPACE_INVALID.error("invalid OCI digest"))?
            .parse::<Id>()?;
        let path = root.join(format!("blobs/sha256/{id}"));
        if value["size"].as_u64() != Some(std::fs::metadata(&path)?.len()) {
            return Err(WORKSPACE_INVALID.error("OCI descriptor size mismatch"));
        }
        Ok(path)
    }
    if json(&directory.path().join("oci-layout"))?["imageLayoutVersion"] != "1.0.0" {
        return Err(WORKSPACE_INVALID.error("unsupported OCI layout"));
    }
    let index = json(&directory.path().join("index.json"))?;
    let manifests = index["manifests"]
        .as_array()
        .filter(|items| items.len() == 1)
        .ok_or_else(|| WORKSPACE_INVALID.error("select one OCI manifest before importing"))?;
    let manifest = json(&descriptor(directory.path(), &manifests[0], MANIFEST)?)?;
    if manifest["config"]["mediaType"] == "application/vnd.oci.image.config.v1+json" {
        let edit = storage::oci::import_image_edit(
            Staging::new(EmptyStore, scratch)?,
            directory.path(),
            scratch,
            transfer_limit,
        )?;
        let revision = edit.finish("OCI filesystem import", None)?;
        let view = View::new(&edit.store, revision)?;
        let usage = crate::revision::usage(&view)?;
        limits.check(usage)?;
        crate::revision::validate_links(&view)?;
        storage::gc::verify_roots(&edit.store, &[revision], scratch, false)?;
        return Ok((edit.store, revision, usage, "oci"));
    }
    let config = json(&descriptor(directory.path(), &manifest["config"], CONFIG)?)?;
    if config["format_version"] != 1 {
        return Err(WORKSPACE_INVALID.error("unsupported workspace format"));
    }
    let revision: Id = config["revision_id"]
        .as_str()
        .ok_or_else(|| WORKSPACE_INVALID.error("missing revision"))?
        .parse()?;
    let store = Staging::new(EmptyStore, scratch)?;
    for layer in manifest["layers"]
        .as_array()
        .ok_or_else(|| WORKSPACE_INVALID.error("missing native packs"))?
    {
        let path = descriptor(directory.path(), layer, PACK)?;
        if std::fs::metadata(&path)?.len() > 80 * 1024 * 1024 {
            return Err(WORKSPACE_INVALID.error("native pack exceeds 80 MiB"));
        }
        storage::record::visit_pack(&path, |id, object| {
            if store.put(object.kind, &object.bytes)? != id {
                return Err(storage::Error::Corrupt("logical ID mismatch".into()));
            }
            Ok(())
        })?;
    }
    storage::gc::verify_roots(&store, &[revision], scratch, false)?;
    let view = View::new(&store, revision)?;
    let usage = crate::revision::usage(&view)?;
    limits.check(usage)?;
    crate::revision::validate_results(&view)?;
    crate::revision::validate_links(&view)?;
    Ok((store, revision, usage, "native"))
}
