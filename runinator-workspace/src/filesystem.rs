//! Conventional OCI filesystem projection; named results remain in native exports only.
use crate::storage::{
    self,
    cache::ByteCache,
    model::{FileObject, InodeData, Kind},
    pages,
    store::{ReadStore, load},
    view::View,
};
use runinator_models::errors::SendableError;
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

pub fn export<S: ReadStore, W: Write>(
    store: &S,
    revision: storage::Id,
    scratch: &Path,
    output: W,
) -> Result<(), SendableError> {
    let view = View::new(store, revision)?;
    let mut layer = tempfile::tempfile_in(scratch)?;
    {
        let mut tar = tar::Builder::new(&mut layer);
        let mut links = BTreeMap::new();
        append(&view, "", &mut tar, &mut links)?;
        tar.finish()?;
    }
    let length = layer.stream_position()?;
    layer.seek(SeekFrom::Start(0))?;
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = layer.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let digest = storage::Id(hash.finalize().into()).to_string();
    layer.seek(SeekFrom::Start(0))?;
    let mut archive = tar::Builder::new(output);
    crate::native::append(
        &mut archive,
        &format!("blobs/sha256/{digest}"),
        length,
        layer,
    )?;
    let config = crate::native::json_blob(
        &mut archive,
        serde_json::json!({"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[format!("sha256:{digest}")]},"config":{}}),
        "application/vnd.oci.image.config.v1+json",
    )?;
    let manifest = crate::native::json_blob(
        &mut archive,
        serde_json::json!({"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":config,"layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":format!("sha256:{digest}"),"size":length}]}),
        "application/vnd.oci.image.manifest.v1+json",
    )?;
    let index = serde_json::to_vec(
        &serde_json::json!({"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[manifest]}),
    )?;
    crate::native::append(
        &mut archive,
        "index.json",
        index.len() as u64,
        index.as_slice(),
    )?;
    let layout = b"{\"imageLayoutVersion\":\"1.0.0\"}";
    crate::native::append(
        &mut archive,
        "oci-layout",
        layout.len() as u64,
        layout.as_slice(),
    )?;
    archive.finish()?;
    Ok(())
}
fn append<S: ReadStore, W: Write>(
    view: &View<S>,
    path: &str,
    tar: &mut tar::Builder<W>,
    links: &mut BTreeMap<u64, String>,
) -> Result<(), SendableError> {
    let (number, inode) = view.stat(path)?;
    let mut header = tar::Header::new_gnu();
    header.set_mode(inode.metadata.mode & 0o777);
    header.set_mtime(inode.metadata.modified_ns.max(0) as u64 / 1_000_000_000);
    header.set_size(0);
    match inode.data {
        InodeData::Directory(_) => {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_cksum();
            tar.append_data(
                &mut header,
                if path.is_empty() { "." } else { path },
                std::io::empty(),
            )?;
            let mut after = None;
            loop {
                let entries = view.directory(path, after.as_deref(), 256)?;
                if entries.is_empty() {
                    break;
                }
                for entry in entries {
                    let child = if path.is_empty() {
                        entry.name.clone()
                    } else {
                        format!("{path}/{}", entry.name)
                    };
                    append(view, &child, tar, links)?;
                    after = Some(entry.name);
                }
            }
        }
        InodeData::Symlink(target) => {
            header.set_entry_type(tar::EntryType::Symlink);
            tar.append_link(&mut header, path, target)?;
        }
        InodeData::File(id) => {
            if let Some(source) = links.get(&number) {
                header.set_entry_type(tar::EntryType::Link);
                tar.append_link(&mut header, path, source)?;
            } else {
                let file: FileObject = load(&view.store, id, Kind::File)?;
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(file.size);
                header.set_cksum();
                let cache = ByteCache::new(8 * 1024 * 1024);
                tar.append_data(
                    &mut header,
                    path,
                    pages::FileReader::new(&view.store, &cache, id)?,
                )?;
                links.insert(number, path.into());
            }
        }
    }
    Ok(())
}
