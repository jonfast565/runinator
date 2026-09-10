//! OCI interoperability.
//!
//! Two projections are supported:
//! * native workspace artifacts preserve the internal Merkle/FastCDC graph;
//! * standard OCI images materialize a revision as a conventional rootfs tar layer.
//!
//! Internal object identity remains BLAKE3. OCI descriptors and DiffIDs are SHA-256.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::{
    Error, Id,
    codec::MAX_OBJECT,
    error::{Result, corrupt, invalid},
    gc, io_util,
    model::{FileObject, Inode, InodeData, Kind, Metadata, Workspace},
    namespace, pages,
    projection::{self, PathNode, PathProjection},
    radix, record,
    repository::{Repository, Snapshot},
    store::{ReadStore, WriteStore, load},
    transaction::Edit,
};

const INDEX: &str = "application/vnd.oci.image.index.v1+json";
const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const IMAGE_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const LAYER_TAR: &str = "application/vnd.oci.image.layer.v1.tar";
const LAYER_GZIP: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const LAYER_ZSTD: &str = "application/vnd.oci.image.layer.v1.tar+zstd";
const ARTIFACT: &str = "application/vnd.runinator.workspace.v1";
const CONFIG: &str = "application/vnd.runinator.workspace.config.v1+json";
const PACK: &str = "application/vnd.runinator.workspace.pack.v1";

struct Sha256Writer<W> {
    inner: W,
    hasher: Sha256,
}

impl<W> Sha256Writer<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    fn digest(self) -> Id {
        Id(self.hasher.finalize().into())
    }
}

impl<W: Write> Write for Sha256Writer<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub media_type: String,
    pub digest: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ImageOptions {
    pub os: String,
    pub architecture: String,
    pub reference_name: String,
    pub labels: BTreeMap<String, String>,
}
impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            os: "linux".into(),
            architecture: "amd64".into(),
            reference_name: "latest".into(),
            labels: BTreeMap::new(),
        }
    }
}

/// Controls Merkle-aware history projection into conventional OCI layers.
///
/// `max_layers` bounds the exported OCI stack. If the native revision chain is
/// longer, the oldest selected revision is emitted as a full checkpoint and
/// only newer revisions become delta layers. A value of 1 is equivalent to the
/// flattened `export_image()` representation.
#[derive(Debug, Clone)]
pub struct MerkleImageOptions {
    pub image: ImageOptions,
    pub max_layers: usize,
    pub include_empty_layers: bool,
}

impl Default for MerkleImageOptions {
    fn default() -> Self {
        Self {
            image: ImageOptions::default(),
            max_layers: 16,
            include_empty_layers: false,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageIndex {
    schema_version: u32,
    #[serde(default)]
    media_type: String,
    manifests: Vec<Descriptor>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact_type: Option<String>,
    config: Descriptor,
    layers: Vec<Descriptor>,
}

#[derive(Serialize, Deserialize)]
struct RootFs {
    #[serde(rename = "type")]
    kind: String,
    diff_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct OciImageConfig {
    architecture: String,
    os: String,
    rootfs: RootFs,
    config: RuntimeConfig,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactConfig {
    format_version: u32,
    revision: String,
}

fn descriptor(kind: &str, id: Id, size: u64) -> Descriptor {
    Descriptor {
        media_type: kind.into(),
        digest: format!("sha256:{id}"),
        size,
        annotations: BTreeMap::new(),
    }
}

fn put_json<T: Serialize>(blobs: &Path, kind: &str, value: &T) -> Result<Descriptor> {
    let bytes = serde_json::to_vec(value)?;
    let mut temp = NamedTempFile::new_in(blobs)?;
    temp.write_all(&bytes)?;
    let id = io_util::install(temp, blobs, "")?;
    Ok(descriptor(kind, id, bytes.len() as u64))
}

fn begin_layout(destination: &Path) -> Result<(tempfile::TempDir, PathBuf)> {
    if destination.try_exists()? {
        return Err(Error::Exists(destination.display().to_string()));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let work = tempfile::Builder::new()
        .prefix(".pws-oci-")
        .tempdir_in(parent)?;
    let blobs = work.path().join("blobs/sha256");
    fs::create_dir_all(&blobs)?;
    Ok((work, blobs))
}

fn finish_layout(
    work: tempfile::TempDir,
    destination: &Path,
    mut manifest_desc: Descriptor,
    reference_name: &str,
) -> Result<()> {
    manifest_desc.annotations.insert(
        "org.opencontainers.image.ref.name".into(),
        reference_name.into(),
    );
    let index = ImageIndex {
        schema_version: 2,
        media_type: INDEX.into(),
        manifests: vec![manifest_desc],
    };
    io_util::atomic_replace(
        &work.path().join("oci-layout"),
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    )?;
    io_util::atomic_replace(
        &work.path().join("index.json"),
        &serde_json::to_vec(&index)?,
    )?;
    io_util::sync_dir(&work.path().join("blobs/sha256"))?;
    io_util::sync_dir(&work.path().join("blobs"))?;
    io_util::sync_dir(work.path())?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if destination.try_exists()? {
        return Err(Error::Exists(destination.display().to_string()));
    }
    fs::rename(work.path(), destination)?;
    io_util::sync_dir(parent)
}

/// Native artifact export. This is lossless for the paged-workspace graph but
/// is not a conventional container root filesystem.
pub fn export(snapshot: &Snapshot<'_>, destination: impl AsRef<Path>) -> Result<()> {
    export_artifact(snapshot, destination)
}

pub fn export_artifact(snapshot: &Snapshot<'_>, destination: impl AsRef<Path>) -> Result<()> {
    let destination = destination.as_ref();
    let (work, blobs) = begin_layout(destination)?;
    let marks = gc::mark(
        snapshot,
        &[snapshot.id],
        &snapshot.repository.root.join("tmp"),
    )?;
    let mut pack = NamedTempFile::new_in(&blobs)?;
    let mut tiny = crate::tiny::Pending::default();
    let mut chunks = crate::chunkblock::Pending::default();
    pack.write_all(record::PACK_MAGIC)?;
    marks.visit(|id| {
        let object = snapshot.get(id)?;
        if crate::tiny::eligible(object.kind, &object.bytes)? {
            if tiny.would_overflow(object.bytes.len()) {
                flush_tiny_export(&mut pack, &mut tiny)?;
            }
            tiny.push(id, &object.bytes)?;
        } else if object.kind == Kind::Chunk {
            if chunks.would_overflow(object.bytes.len()) {
                flush_chunk_export(&mut pack, &mut chunks)?;
            }
            chunks.push(id, &object.bytes)?;
        } else {
            let (written, _) = record::write(&mut pack, object.kind, &object.bytes)?;
            if written != id {
                return Err(corrupt("export changed an object identity"));
            }
        }
        Ok(())
    })?;
    flush_tiny_export(&mut pack, &mut tiny)?;
    flush_chunk_export(&mut pack, &mut chunks)?;
    let size = pack.as_file().metadata()?.len();
    let pack_id = io_util::install(pack, &blobs, "")?;
    let config = put_json(
        &blobs,
        CONFIG,
        &ArtifactConfig {
            format_version: 1,
            revision: snapshot.id.to_string(),
        },
    )?;
    let manifest = Manifest {
        schema_version: 2,
        media_type: MANIFEST.into(),
        artifact_type: Some(ARTIFACT.into()),
        config,
        layers: vec![descriptor(PACK, pack_id, size)],
    };
    let manifest_desc = put_json(&blobs, MANIFEST, &manifest)?;
    finish_layout(work, destination, manifest_desc, "snapshot")
}

fn flush_tiny_export<W: Write>(writer: &mut W, pending: &mut crate::tiny::Pending) -> Result<()> {
    if let Some((raw, _)) = pending.take()? {
        record::write(writer, Kind::TinyBlock, &raw)?;
    }
    Ok(())
}
fn flush_chunk_export<W: Write>(
    writer: &mut W,
    pending: &mut crate::chunkblock::Pending,
) -> Result<()> {
    if let Some((raw, _)) = pending.take()? {
        record::write(writer, Kind::ChunkBlock, &raw)?;
    }
    Ok(())
}

fn clean_tar_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(x) => {
                let s = x
                    .to_str()
                    .ok_or_else(|| invalid("OCI paths must be UTF-8"))?;
                if s.is_empty() || s.contains('\0') || s.contains('\\') {
                    return Err(invalid("invalid OCI path component"));
                }
                parts.push(s.to_owned());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid("OCI path escapes the virtual root"));
            }
        }
    }
    if parts.is_empty() {
        return Ok(String::new());
    }
    Ok(parts.join("/"))
}

const OCI_UID_XATTR: &str = "pws.oci.uid";
const OCI_GID_XATTR: &str = "pws.oci.gid";

fn metadata_id(metadata: &Metadata, key: &str) -> u64 {
    metadata
        .xattrs
        .get(key)
        .and_then(|b| b.as_slice().try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

fn set_tar_metadata(header: &mut tar::Header, metadata: &Metadata) {
    header.set_mode(metadata.mode & 0o7777);
    header.set_uid(metadata_id(metadata, OCI_UID_XATTR));
    header.set_gid(metadata_id(metadata, OCI_GID_XATTR));
    let seconds = if metadata.modified_ns <= 0 {
        0
    } else {
        (metadata.modified_ns as u64) / 1_000_000_000
    };
    header.set_mtime(seconds);
}

fn append_snapshot_tree<W: Write>(
    snapshot: &Snapshot<'_>,
    builder: &mut tar::Builder<W>,
    directory: &str,
    hardlinks: &mut HashMap<u64, String>,
) -> Result<()> {
    let mut children = Vec::new();
    snapshot.list(directory, &mut |name, inode| {
        children.push((name.to_owned(), inode));
        Ok(())
    })?;
    children.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, inode_number) in children {
        let path = if directory.is_empty() {
            name
        } else {
            format!("{directory}/{name}")
        };
        let (_, inode) = snapshot.stat(&path)?;
        match &inode.data {
            InodeData::Directory(_) => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                set_tar_metadata(&mut header, &inode.metadata);
                header.set_size(0);
                header.set_cksum();
                builder.append_data(&mut header, format!("{path}/"), std::io::empty())?;
                append_snapshot_tree(snapshot, builder, &path, hardlinks)?;
            }
            InodeData::Symlink(target) => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Symlink);
                set_tar_metadata(&mut header, &inode.metadata);
                header.set_size(0);
                header.set_link_name(target)?;
                header.set_cksum();
                builder.append_data(&mut header, &path, std::io::empty())?;
            }
            InodeData::File(file) => {
                if let Some(first) = hardlinks.get(&inode_number) {
                    let mut header = tar::Header::new_gnu();
                    header.set_entry_type(tar::EntryType::Link);
                    header.set_mode(inode.metadata.mode & 0o7777);
                    header.set_size(0);
                    header.set_link_name(first)?;
                    header.set_cksum();
                    builder.append_data(&mut header, &path, std::io::empty())?;
                    continue;
                }
                hardlinks.insert(inode_number, path.clone());
                let object: FileObject = load(snapshot, *file, Kind::File)?;
                let size = object.size;
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                set_tar_metadata(&mut header, &inode.metadata);
                header.set_size(size);
                header.set_cksum();
                builder.append_data(&mut header, &path, snapshot.reader(&path)?)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct RevisionState {
    workspace: Workspace,
    projection: PathProjection,
}

fn load_revision_state(snapshot: &Snapshot<'_>, id: Id) -> Result<RevisionState> {
    let revision: crate::model::Revision = load(snapshot, id, Kind::Revision)?;
    let workspace: Workspace = load(snapshot, revision.workspace, Kind::Workspace)?;
    let projection: PathProjection = load(snapshot, revision.projection, Kind::PathProjection)?;
    Ok(RevisionState {
        workspace,
        projection,
    })
}

#[derive(Clone)]
struct InventoryEntry {
    inode_number: u64,
    inode: Inode,
}

fn inode_object_id(
    snapshot: &Snapshot<'_>,
    state: &RevisionState,
    inode_number: u64,
) -> Result<Id> {
    radix::get(
        snapshot,
        state.workspace.inodes,
        &inode_number.to_be_bytes(),
    )?
    .ok_or_else(|| corrupt(format!("dangling inode {inode_number}")))
}

fn path_depth(path: &str) -> usize {
    path.bytes().filter(|&b| b == b'/').count() + 1
}

#[derive(Default)]
struct DeltaPlan {
    whiteouts: Vec<String>,
    emit: Vec<String>,
}

fn append_whiteout<W: Write>(builder: &mut tar::Builder<W>, deleted_path: &str) -> Result<()> {
    let (parent, name) = parent_and_name(deleted_path);
    let path = if parent.is_empty() {
        format!(".wh.{name}")
    } else {
        format!("{parent}/.wh.{name}")
    };
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(0);
    header.set_cksum();
    builder.append_data(&mut header, path, std::io::empty())?;
    Ok(())
}

fn append_inventory_entry<W: Write>(
    snapshot: &Snapshot<'_>,
    builder: &mut tar::Builder<W>,
    path: &str,
    entry: &InventoryEntry,
    hardlinks: &mut HashMap<u64, String>,
) -> Result<()> {
    match &entry.inode.data {
        InodeData::Directory(_) => {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            set_tar_metadata(&mut header, &entry.inode.metadata);
            header.set_size(0);
            header.set_cksum();
            builder.append_data(&mut header, format!("{path}/"), std::io::empty())?;
        }
        InodeData::Symlink(target) => {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            set_tar_metadata(&mut header, &entry.inode.metadata);
            header.set_size(0);
            header.set_link_name(target)?;
            header.set_cksum();
            builder.append_data(&mut header, path, std::io::empty())?;
        }
        InodeData::File(file) => {
            if let Some(first) = hardlinks.get(&entry.inode_number) {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Link);
                set_tar_metadata(&mut header, &entry.inode.metadata);
                header.set_size(0);
                header.set_link_name(first)?;
                header.set_cksum();
                builder.append_data(&mut header, path, std::io::empty())?;
                return Ok(());
            }
            hardlinks.insert(entry.inode_number, path.to_owned());
            let object: FileObject = load(snapshot, *file, Kind::File)?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            set_tar_metadata(&mut header, &entry.inode.metadata);
            header.set_size(object.size);
            header.set_cksum();
            builder.append_data(
                &mut header,
                path,
                pages::FileReader::new(snapshot, &snapshot.repository.pages, *file)?,
            )?;
        }
    }
    Ok(())
}

fn projection_inode<S: ReadStore + ?Sized>(store: &S, node: &PathNode) -> Result<Inode> {
    load(store, node.inode_id, Kind::Inode)
}

fn join_projection_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

fn mark_projection_entry(
    store: &Snapshot<'_>,
    node: &PathNode,
    path: &str,
    emit: &mut BTreeSet<String>,
    changed_hardlinks: &mut BTreeSet<u64>,
) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    emit.insert(path.to_owned());
    let inode = projection_inode(store, node)?;
    if inode.links > 1 && matches!(inode.data, InodeData::File(_)) {
        changed_hardlinks.insert(node.inode_number);
    }
    Ok(())
}

fn add_projection_subtree(
    snapshot: &Snapshot<'_>,
    id: Id,
    path: &str,
    emit: &mut BTreeSet<String>,
    changed_hardlinks: &mut BTreeSet<u64>,
) -> Result<()> {
    let node: PathNode = load(snapshot, id, Kind::PathNode)?;
    mark_projection_entry(snapshot, &node, path, emit, changed_hardlinks)?;
    for (name, child) in node.entries(snapshot)? {
        let child_path = join_projection_path(path, &name);
        add_projection_subtree(snapshot, child, &child_path, emit, changed_hardlinks)?;
    }
    Ok(())
}

/// Merkle-diff two path projection nodes. Equal node IDs skip an entire
/// subtree without opening directory radixes, inode objects, files, pages, or
/// FastCDC chunks.
fn diff_projection_node(
    snapshot: &Snapshot<'_>,
    old_id: Id,
    new_id: Id,
    path: &str,
    whiteouts: &mut BTreeSet<String>,
    emit: &mut BTreeSet<String>,
    changed_hardlinks: &mut BTreeSet<u64>,
) -> Result<()> {
    if old_id == new_id {
        return Ok(());
    }

    let old: PathNode = load(snapshot, old_id, Kind::PathNode)?;
    let current: PathNode = load(snapshot, new_id, Kind::PathNode)?;
    let old_inode = projection_inode(snapshot, &old)?;
    let current_inode = projection_inode(snapshot, &current)?;
    let old_dir = matches!(old_inode.data, InodeData::Directory(_));
    let new_dir = matches!(current_inode.data, InodeData::Directory(_));

    // The projection node identity includes inode number + inode object ID.
    // A changed directory node may differ only because a descendant changed;
    // in that case there is no need to re-emit the directory entry itself.
    if old.inode_number != current.inode_number || old.inode_id != current.inode_id {
        mark_projection_entry(snapshot, &current, path, emit, changed_hardlinks)?;
    }

    match (old_dir, new_dir) {
        (true, true) => {
            let old_children: BTreeMap<_, _> = old.entries(snapshot)?.into_iter().collect();
            let current_children: BTreeMap<_, _> = current.entries(snapshot)?.into_iter().collect();
            let mut names = BTreeSet::new();
            names.extend(old_children.keys().cloned());
            names.extend(current_children.keys().cloned());
            for name in names {
                let child_path = join_projection_path(path, &name);
                match (old_children.get(&name), current_children.get(&name)) {
                    (Some(&a), Some(&b)) => diff_projection_node(
                        snapshot,
                        a,
                        b,
                        &child_path,
                        whiteouts,
                        emit,
                        changed_hardlinks,
                    )?,
                    (Some(_), None) => {
                        // One whiteout at the removed subtree root is enough.
                        whiteouts.insert(child_path);
                    }
                    (None, Some(&b)) => {
                        add_projection_subtree(snapshot, b, &child_path, emit, changed_hardlinks)?
                    }
                    (None, None) => unreachable!(),
                }
            }
        }
        (false, true) => {
            // The new directory entry replaces the old non-directory. Emit its
            // complete new subtree, but not a whiteout for the replaced path.
            for (name, child) in current.entries(snapshot)? {
                let child_path = join_projection_path(path, &name);
                add_projection_subtree(snapshot, child, &child_path, emit, changed_hardlinks)?;
            }
        }
        (true, false) => {
            // Replacing the directory root removes its lower-layer descendants
            // by OCI replacement semantics; descendant whiteouts are redundant.
        }
        (false, false) => {}
    }
    Ok(())
}

fn projection_delta_plan(
    snapshot: &Snapshot<'_>,
    old: &RevisionState,
    current: &RevisionState,
) -> Result<DeltaPlan> {
    if old.projection.root == current.projection.root {
        return Ok(DeltaPlan::default());
    }

    let mut whiteouts = BTreeSet::new();
    let mut emit = BTreeSet::new();
    let mut changed_hardlinks = BTreeSet::new();
    diff_projection_node(
        snapshot,
        old.projection.root,
        current.projection.root,
        "",
        &mut whiteouts,
        &mut emit,
        &mut changed_hardlinks,
    )?;

    // Hard-linked leaves can occur in unrelated directory subtrees. The
    // projection's inode->path-list index lets us recover those aliases without
    // scanning the namespace.
    for inode in changed_hardlinks {
        for path in projection::hardlink_paths(snapshot, &current.projection, inode)? {
            emit.insert(path);
        }
    }

    let mut whiteouts: Vec<_> = whiteouts.into_iter().collect();
    whiteouts.sort_by(|a, b| path_depth(a).cmp(&path_depth(b)).then_with(|| a.cmp(b)));
    let mut emit: Vec<_> = emit.into_iter().collect();
    emit.sort_by(|a, b| path_depth(a).cmp(&path_depth(b)).then_with(|| a.cmp(b)));
    Ok(DeltaPlan { whiteouts, emit })
}

fn inventory_entry_for_path(
    snapshot: &Snapshot<'_>,
    state: &RevisionState,
    path: &str,
) -> Result<InventoryEntry> {
    let (inode_number, inode) = namespace::stat(snapshot, &state.workspace, path)?;
    Ok(InventoryEntry {
        inode_number,
        inode,
    })
}

fn append_delta_tree<W: Write>(
    snapshot: &Snapshot<'_>,
    old: &RevisionState,
    current: &RevisionState,
    builder: &mut tar::Builder<W>,
) -> Result<bool> {
    if old.projection.root == current.projection.root {
        return Ok(false);
    }

    let plan = projection_delta_plan(snapshot, old, current)?;
    if plan.whiteouts.is_empty() && plan.emit.is_empty() {
        return Ok(false);
    }

    for path in &plan.whiteouts {
        append_whiteout(builder, path)?;
    }

    let mut hardlinks = HashMap::new();
    for path in &plan.emit {
        let entry = inventory_entry_for_path(snapshot, current, path)?;
        append_inventory_entry(snapshot, builder, path, &entry, &mut hardlinks)?;
    }
    Ok(true)
}

fn append_state_tree<W: Write>(
    snapshot: &Snapshot<'_>,
    state: &RevisionState,
    builder: &mut tar::Builder<W>,
    directory: &str,
    hardlinks: &mut HashMap<u64, String>,
) -> Result<()> {
    let mut children = Vec::new();
    namespace::list(snapshot, &state.workspace, directory, &mut |name, inode| {
        children.push((name.to_owned(), inode));
        Ok(())
    })?;
    children.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, inode_number) in children {
        let path = if directory.is_empty() {
            name
        } else {
            format!("{directory}/{name}")
        };
        let inode_id = inode_object_id(snapshot, state, inode_number)?;
        let inode: Inode = load(snapshot, inode_id, Kind::Inode)?;
        let entry = InventoryEntry {
            inode_number,
            inode,
        };
        append_inventory_entry(snapshot, builder, &path, &entry, hardlinks)?;
        if matches!(&entry.inode.data, InodeData::Directory(_)) {
            append_state_tree(snapshot, state, builder, &path, hardlinks)?;
        }
    }
    Ok(())
}

fn install_tar_layer(
    work: &Path,
    blobs: &Path,
    build: impl FnOnce(&mut tar::Builder<&mut Sha256Writer<&mut NamedTempFile>>) -> Result<bool>,
) -> Result<Option<(Descriptor, String)>> {
    let mut tar_tmp = NamedTempFile::new_in(work)?;
    let mut tar_writer = Sha256Writer::new(&mut tar_tmp);
    let nonempty = {
        let mut builder = tar::Builder::new(&mut tar_writer);
        let nonempty = build(&mut builder)?;
        builder.finish()?;
        nonempty
    };
    if !nonempty {
        return Ok(None);
    }
    let diff_id = tar_writer.digest();
    tar_tmp.as_file().sync_all()?;

    let mut compressed = NamedTempFile::new_in(blobs)?;
    let mut compressed_writer = Sha256Writer::new(&mut compressed);
    {
        let input = File::open(tar_tmp.path())?;
        let mut encoder = zstd::stream::write::Encoder::new(&mut compressed_writer, 3)?;
        std::io::copy(&mut std::io::BufReader::new(input), &mut encoder)?;
        encoder.finish()?;
    }
    let layer_id = compressed_writer.digest();
    let layer_size = compressed.as_file().metadata()?.len();
    io_util::install_known(compressed, blobs, "", layer_id)?;
    Ok(Some((
        descriptor(LAYER_ZSTD, layer_id, layer_size),
        format!("sha256:{diff_id}"),
    )))
}

fn collect_revision_chain(snapshot: &Snapshot<'_>, max_layers: usize) -> Result<Vec<Id>> {
    if max_layers == 0 {
        return Err(invalid("OCI max_layers must be at least 1"));
    }
    let mut ids = Vec::with_capacity(max_layers);
    let mut cursor = Some(snapshot.id);
    while let Some(id) = cursor {
        ids.push(id);
        if ids.len() == max_layers {
            break;
        }
        let revision: crate::model::Revision = load(snapshot, id, Kind::Revision)?;
        cursor = revision.parent;
    }
    ids.reverse();
    Ok(ids)
}

/// Exports the most recent native revisions as a bounded conventional OCI layer
/// stack. The oldest selected revision is a complete checkpoint; each later
/// revision becomes a Merkle-aware delta against its parent.
pub fn export_image_merkle(
    snapshot: &Snapshot<'_>,
    destination: impl AsRef<Path>,
    options: &MerkleImageOptions,
) -> Result<()> {
    let image = &options.image;
    if image.os.is_empty() || image.architecture.is_empty() || image.reference_name.is_empty() {
        return Err(invalid(
            "OCI image platform/reference fields may not be empty",
        ));
    }
    if options.max_layers == 0 || options.max_layers > 65536 {
        return Err(invalid("OCI max_layers must be from 1 through 65536"));
    }

    let destination = destination.as_ref();
    let (work, blobs) = begin_layout(destination)?;
    let revisions = collect_revision_chain(snapshot, options.max_layers)?;
    let mut layers = Vec::new();
    let mut diff_ids = Vec::new();

    // A bounded history projection starts from a complete checkpoint so that
    // applying the emitted layer stack to an empty rootfs always reconstructs
    // the target revision.
    let base = load_revision_state(snapshot, revisions[0])?;
    if let Some((mut descriptor, diff_id)) = install_tar_layer(work.path(), &blobs, |builder| {
        let mut hardlinks = HashMap::new();
        append_state_tree(snapshot, &base, builder, "", &mut hardlinks)?;
        Ok(true)
    })? {
        descriptor.annotations.insert(
            "io.paged-workspace.revision".into(),
            revisions[0].to_string(),
        );
        descriptor
            .annotations
            .insert("io.paged-workspace.layer-kind".into(), "checkpoint".into());
        layers.push(descriptor);
        diff_ids.push(diff_id);
    }

    for pair in revisions.windows(2) {
        let old = load_revision_state(snapshot, pair[0])?;
        let current = load_revision_state(snapshot, pair[1])?;
        let built = install_tar_layer(work.path(), &blobs, |builder| {
            append_delta_tree(snapshot, &old, &current, builder)
        })?;
        match built {
            Some((mut descriptor, diff_id)) => {
                descriptor
                    .annotations
                    .insert("io.paged-workspace.revision".into(), pair[1].to_string());
                descriptor
                    .annotations
                    .insert("io.paged-workspace.layer-kind".into(), "delta".into());
                layers.push(descriptor);
                diff_ids.push(diff_id);
            }
            None if options.include_empty_layers => {
                // OCI's empty tar stream is still a valid layer. Emit it only
                // when the caller explicitly wants one layer per selected revision.
                if let Some((descriptor, diff_id)) =
                    install_tar_layer(work.path(), &blobs, |_builder| Ok(true))?
                {
                    layers.push(descriptor);
                    diff_ids.push(diff_id);
                }
            }
            None => {}
        }
    }

    let config = OciImageConfig {
        architecture: image.architecture.clone(),
        os: image.os.clone(),
        rootfs: RootFs {
            kind: "layers".into(),
            diff_ids,
        },
        config: RuntimeConfig {
            labels: image.labels.clone(),
        },
    };
    let config_desc = put_json(&blobs, IMAGE_CONFIG, &config)?;
    let manifest = Manifest {
        schema_version: 2,
        media_type: MANIFEST.into(),
        artifact_type: None,
        config: config_desc,
        layers,
    };
    let manifest_desc = put_json(&blobs, MANIFEST, &manifest)?;
    finish_layout(work, destination, manifest_desc, &image.reference_name)
}

/// Exports a revision as a conventional single-layer OCI rootfs image.
/// The layer is zstd-compressed and uses the standard OCI filesystem media type.
pub fn export_image(
    snapshot: &Snapshot<'_>,
    destination: impl AsRef<Path>,
    options: &ImageOptions,
) -> Result<()> {
    if options.os.is_empty() || options.architecture.is_empty() || options.reference_name.is_empty()
    {
        return Err(invalid(
            "OCI image platform/reference fields may not be empty",
        ));
    }
    let destination = destination.as_ref();
    let (work, blobs) = begin_layout(destination)?;

    // First produce the canonical uncompressed tar stream so its SHA-256 is the DiffID.
    let mut tar_tmp = NamedTempFile::new_in(work.path())?;
    let mut tar_writer = Sha256Writer::new(&mut tar_tmp);
    {
        let mut builder = tar::Builder::new(&mut tar_writer);
        let mut hardlinks = HashMap::new();
        append_snapshot_tree(snapshot, &mut builder, "", &mut hardlinks)?;
        builder.finish()?;
    }
    let diff_id = tar_writer.digest();
    tar_tmp.as_file().sync_all()?;

    // Compress to the stored OCI layer blob. Descriptor digest hashes compressed bytes.
    let mut compressed = NamedTempFile::new_in(&blobs)?;
    let mut compressed_writer = Sha256Writer::new(&mut compressed);
    {
        let input = File::open(tar_tmp.path())?;
        let mut encoder = zstd::stream::write::Encoder::new(&mut compressed_writer, 3)?;
        std::io::copy(&mut std::io::BufReader::new(input), &mut encoder)?;
        encoder.finish()?;
    }
    let layer_id = compressed_writer.digest();
    let layer_size = compressed.as_file().metadata()?.len();
    io_util::install_known(compressed, &blobs, "", layer_id)?;

    let config = OciImageConfig {
        architecture: options.architecture.clone(),
        os: options.os.clone(),
        rootfs: RootFs {
            kind: "layers".into(),
            diff_ids: vec![format!("sha256:{diff_id}")],
        },
        config: RuntimeConfig {
            labels: options.labels.clone(),
        },
    };
    let config_desc = put_json(&blobs, IMAGE_CONFIG, &config)?;
    let manifest = Manifest {
        schema_version: 2,
        media_type: MANIFEST.into(),
        artifact_type: None,
        config: config_desc,
        layers: vec![descriptor(LAYER_ZSTD, layer_id, layer_size)],
    };
    let manifest_desc = put_json(&blobs, MANIFEST, &manifest)?;
    finish_layout(work, destination, manifest_desc, &options.reference_name)
}

fn checked_blob(root: &Path, d: &Descriptor, expected: Option<&str>) -> Result<PathBuf> {
    if let Some(kind) = expected
        && d.media_type != kind
    {
        return Err(corrupt(format!(
            "unexpected OCI media type: {}",
            d.media_type
        )));
    }
    let text = d
        .digest
        .strip_prefix("sha256:")
        .ok_or_else(|| corrupt("only OCI SHA-256 digests are accepted"))?;
    let id: Id = text.parse()?;
    let path = root.join("blobs/sha256").join(id.to_string());
    let actual = fs::canonicalize(&path)?;
    if !actual.starts_with(root) {
        return Err(corrupt("OCI blob resolves outside layout"));
    }
    let meta = fs::metadata(&actual)?;
    if !meta.is_file() || meta.len() != d.size {
        return Err(corrupt("OCI descriptor size mismatch"));
    }
    io_util::verify_file(&actual, id)?;
    Ok(actual)
}

fn read_descriptor<T: for<'de> Deserialize<'de>>(
    root: &Path,
    d: &Descriptor,
    kind: &str,
) -> Result<T> {
    let path = checked_blob(root, d, Some(kind))?;
    Ok(serde_json::from_slice(&io_util::read_limited(
        &path, MAX_OBJECT,
    )?)?)
}

fn open_layout(source: impl AsRef<Path>) -> Result<(PathBuf, Manifest)> {
    let root = fs::canonicalize(source)?;
    let layout: serde_json::Value =
        serde_json::from_slice(&io_util::read_limited(&root.join("oci-layout"), 4096)?)?;
    if layout.get("imageLayoutVersion").and_then(|v| v.as_str()) != Some("1.0.0") {
        return Err(corrupt("unsupported OCI layout"));
    }
    let index: ImageIndex = serde_json::from_slice(&io_util::read_limited(
        &root.join("index.json"),
        MAX_OBJECT,
    )?)?;
    if index.schema_version != 2
        || (!index.media_type.is_empty() && index.media_type != INDEX)
        || index.manifests.len() != 1
    {
        return Err(invalid("import expects exactly one OCI manifest"));
    }
    let manifest: Manifest = read_descriptor(&root, &index.manifests[0], MANIFEST)?;
    if manifest.schema_version != 2
        || manifest.media_type != MANIFEST
        || manifest.layers.len() > 65536
    {
        return Err(corrupt("unsupported OCI manifest"));
    }
    Ok((root, manifest))
}

/// Native artifact import. `expected=None` refuses to replace an existing ref.
pub fn import(
    repo: &Repository,
    source: impl AsRef<Path>,
    name: &str,
    expected: Option<Id>,
) -> Result<Id> {
    import_artifact(repo, source, name, expected)
}

pub fn import_artifact(
    repo: &Repository,
    source: impl AsRef<Path>,
    name: &str,
    expected: Option<Id>,
) -> Result<Id> {
    let (root, manifest) = open_layout(source)?;
    if manifest.artifact_type.as_deref() != Some(ARTIFACT) || manifest.layers.is_empty() {
        return Err(corrupt("not a supported paged-workspace OCI artifact"));
    }
    let config: ArtifactConfig = read_descriptor(&root, &manifest.config, CONFIG)?;
    if config.format_version != 1 {
        return Err(corrupt("unsupported workspace format"));
    }
    let revision: Id = config.revision.parse()?;
    let transaction = repo.transaction(name)?;
    if transaction.expected != expected {
        return Err(Error::Conflict);
    }
    for layer in &manifest.layers {
        let path = checked_blob(&root, layer, Some(PACK))?;
        record::visit_pack(&path, |id, object| {
            let stored = transaction.store.put(object.kind, &object.bytes)?;
            if stored != id {
                return Err(corrupt("imported identity changed"));
            }
            Ok(())
        })?;
    }
    gc::verify_graph(&transaction.store, &[revision], &repo.root.join("tmp"))?;
    transaction.publish_revision(revision)
}

fn parent_and_name(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn remove_tree<S: WriteStore>(tx: &mut Edit<S>, path: &str) -> Result<()> {
    let inode = match tx.stat(path) {
        Ok((_, inode)) => inode,
        Err(Error::NotFound(_)) => return Ok(()),
        Err(e) => return Err(e),
    };
    if matches!(&inode.data, InodeData::Directory(_)) {
        let names = tx.list_names(path)?;
        for name in names {
            let child = if path.is_empty() {
                name
            } else {
                format!("{path}/{name}")
            };
            remove_tree(tx, &child)?;
        }
    }
    tx.unlink(path)
}

fn metadata_from_tar(header: &tar::Header) -> Metadata {
    let mut xattrs = BTreeMap::new();
    if let Ok(uid) = header.uid() {
        xattrs.insert(OCI_UID_XATTR.into(), uid.to_le_bytes().to_vec());
    }
    if let Ok(gid) = header.gid() {
        xattrs.insert(OCI_GID_XATTR.into(), gid.to_le_bytes().to_vec());
    }
    Metadata {
        mode: header.mode().unwrap_or(0o644),
        created_ns: 0,
        modified_ns: header
            .mtime()
            .map(|s| (s as i64).saturating_mul(1_000_000_000))
            .unwrap_or(0),
        xattrs,
    }
}

fn copy_layer<R: Read>(mut input: R, dir: &Path) -> Result<(NamedTempFile, Id)> {
    let mut out = NamedTempFile::new_in(dir)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    out.as_file().sync_all()?;
    Ok((out, Id(hasher.finalize().into())))
}

fn is_whiteout(path: &str) -> bool {
    let (_, name) = parent_and_name(path);
    name.starts_with(".wh.")
}

fn apply_whiteout<S: WriteStore>(tx: &mut Edit<S>, path: &str) -> Result<()> {
    let (parent, name) = parent_and_name(path);
    if name == ".wh..wh..opq" {
        for child in tx.list_names(parent)? {
            let child_path = if parent.is_empty() {
                child
            } else {
                format!("{parent}/{child}")
            };
            remove_tree(tx, &child_path)?;
        }
        return Ok(());
    }
    if let Some(target) = name.strip_prefix(".wh.") {
        let target = if parent.is_empty() {
            target.to_owned()
        } else {
            format!("{parent}/{target}")
        };
        remove_tree(tx, &target)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FsKind {
    File,
    Directory,
    Symlink,
}

fn inode_kind(inode: &Inode) -> FsKind {
    match inode.data {
        InodeData::File(_) => FsKind::File,
        InodeData::Directory(_) => FsKind::Directory,
        InodeData::Symlink(_) => FsKind::Symlink,
    }
}

fn prepare_path<S: WriteStore>(tx: &mut Edit<S>, path: &str, desired: FsKind) -> Result<()> {
    match tx.stat(path) {
        Ok((_, inode)) if desired != FsKind::Directory || inode_kind(&inode) != desired => {
            remove_tree(tx, path)
        }
        Ok(_) | Err(Error::NotFound(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

fn apply_tar_pass<R: Read, S: WriteStore>(
    tx: &mut Edit<S>,
    reader: R,
    whiteouts_only: bool,
) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    for item in archive.entries()? {
        let mut entry = item?;
        let path = clean_tar_path(&entry.path()?)?;
        if path.is_empty() {
            if !entry.header().entry_type().is_dir() {
                return Err(invalid("OCI root must be a directory"));
            }
            if !whiteouts_only {
                tx.set_metadata("", metadata_from_tar(entry.header()))?;
            }
            continue;
        }
        let whiteout = is_whiteout(&path);
        if whiteouts_only {
            if whiteout {
                apply_whiteout(tx, &path)?;
            }
            continue;
        }
        if whiteout {
            continue;
        }

        let (parent, _) = parent_and_name(&path);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            prepare_path(tx, &path, FsKind::Directory)?;
            tx.mkdir_all(&path)?;
            tx.set_metadata(&path, metadata_from_tar(entry.header()))?;
            continue;
        }
        if kind.is_file() {
            prepare_path(tx, &path, FsKind::File)?;
            if !parent.is_empty() {
                tx.mkdir_all(parent)?;
            }
            let metadata = metadata_from_tar(entry.header());
            tx.put(&path, &mut entry)?;
            tx.set_metadata(&path, metadata)?;
            continue;
        }
        if kind.is_symlink() {
            prepare_path(tx, &path, FsKind::Symlink)?;
            if !parent.is_empty() {
                tx.mkdir_all(parent)?;
            }
            let metadata = metadata_from_tar(entry.header());
            let target = entry
                .link_name()?
                .ok_or_else(|| corrupt("symlink has no target"))?;
            let target = target
                .to_str()
                .ok_or_else(|| invalid("symlink target must be UTF-8"))?;
            tx.symlink(&path, target)?;
            tx.set_metadata(&path, metadata)?;
            continue;
        }
        if kind.is_hard_link() {
            if tx.stat(&path).is_ok() {
                remove_tree(tx, &path)?;
            }
            if !parent.is_empty() {
                tx.mkdir_all(parent)?;
            }
            let target = entry
                .link_name()?
                .ok_or_else(|| corrupt("hard link has no target"))?;
            let target = clean_tar_path(&target)?;
            tx.hard_link(&target, &path)?;
            continue;
        }
        return Err(invalid(format!(
            "unsupported OCI tar entry type for {path}"
        )));
    }
    Ok(())
}

fn apply_tar_file<S: WriteStore>(tx: &mut Edit<S>, path: &Path) -> Result<()> {
    // Whiteouts conceptually apply to the lower rootfs before additions from
    // this same layer, independent of tar entry ordering.
    apply_tar_pass(tx, File::open(path)?, true)?;
    apply_tar_pass(tx, File::open(path)?, false)
}

/// Imports a conventional OCI image rootfs into a workspace branch. OCI layer
/// whiteouts are applied in layer order. The imported image becomes one native
/// workspace revision; internal FastCDC deduplication happens while tar entries stream in.
pub fn import_image(
    repo: &Repository,
    source: impl AsRef<Path>,
    name: &str,
    message: &str,
) -> Result<Id> {
    if repo.head(name)?.is_some() {
        return Err(Error::Exists(format!("ref already exists: {name}")));
    }
    let (root, manifest) = open_layout(source)?;
    if manifest.artifact_type.is_some() {
        return Err(invalid("OCI artifact is not a filesystem image"));
    }
    let config: OciImageConfig = read_descriptor(&root, &manifest.config, IMAGE_CONFIG)?;
    if config.rootfs.kind != "layers" || config.rootfs.diff_ids.len() != manifest.layers.len() {
        return Err(corrupt("OCI config rootfs does not match manifest layers"));
    }
    let mut tx = repo.transaction(name)?;
    for (layer, expected_diff) in manifest.layers.iter().zip(&config.rootfs.diff_ids) {
        let expected: Id = expected_diff
            .strip_prefix("sha256:")
            .ok_or_else(|| corrupt("OCI DiffID must use SHA-256"))?
            .parse()?;
        let path = checked_blob(&root, layer, None)?;
        let (tar_file, actual) = match layer.media_type.as_str() {
            LAYER_TAR => copy_layer(File::open(path)?, &repo.root.join("tmp"))?,
            LAYER_GZIP => {
                let decoder = flate2::read::GzDecoder::new(File::open(path)?);
                copy_layer(decoder, &repo.root.join("tmp"))?
            }
            LAYER_ZSTD => {
                let decoder = zstd::stream::read::Decoder::new(File::open(path)?)?;
                copy_layer(decoder, &repo.root.join("tmp"))?
            }
            other => {
                return Err(invalid(format!(
                    "unsupported OCI layer media type: {other}"
                )));
            }
        };
        if actual != expected {
            return Err(corrupt("OCI layer DiffID mismatch"));
        }
        apply_tar_file(&mut tx, tar_file.path())?;
    }
    tx.commit(message)
}

/// Import a conventional rootfs into an unpublished edit with a cumulative expansion budget.
pub fn import_image_edit<S: WriteStore>(
    store: S,
    source: &Path,
    scratch: &Path,
    max_expanded_bytes: u64,
) -> Result<Edit<S>> {
    let (root, manifest) = open_layout(source)?;
    if manifest.artifact_type.is_some() {
        return Err(invalid("OCI artifact is not a filesystem image"));
    }
    let config: OciImageConfig = read_descriptor(&root, &manifest.config, IMAGE_CONFIG)?;
    if config.rootfs.kind != "layers" || config.rootfs.diff_ids.len() != manifest.layers.len() {
        return Err(corrupt("OCI layer count mismatch"));
    }
    let mut tx = Edit::new(store, None, crate::Layout::default(), 16 * 1024 * 1024)?;
    let mut remaining = max_expanded_bytes;
    for (layer, expected_diff) in manifest.layers.iter().zip(&config.rootfs.diff_ids) {
        let expected: Id = expected_diff
            .strip_prefix("sha256:")
            .ok_or_else(|| corrupt("OCI DiffID must use SHA-256"))?
            .parse()?;
        let path = checked_blob(&root, layer, None)?;
        let input: Box<dyn Read> = match layer.media_type.as_str() {
            LAYER_TAR => Box::new(File::open(path)?),
            LAYER_GZIP => Box::new(flate2::read::GzDecoder::new(File::open(path)?)),
            LAYER_ZSTD => Box::new(zstd::stream::read::Decoder::new(File::open(path)?)?),
            _ => return Err(invalid("unsupported OCI layer media type")),
        };
        let (file, digest) = copy_layer(input.take(remaining.saturating_add(1)), scratch)?;
        remaining = remaining
            .checked_sub(file.as_file().metadata()?.len())
            .ok_or_else(|| invalid("OCI expanded layers exceed scratch budget"))?;
        if digest != expected {
            return Err(corrupt("OCI layer DiffID mismatch"));
        }
        apply_tar_file(&mut tx, file.path())?;
    }
    Ok(tx)
}
