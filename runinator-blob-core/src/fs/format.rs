//! the self-describing v2 object and multipart-part file format.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::errors::BlobError;
use crate::meta::ObjectMeta;

pub(super) const OBJECT_MAGIC: [u8; 8] = *b"RUNIBL02";
pub(super) const PART_MAGIC: [u8; 8] = *b"RUNIPT02";
const TRAILER_LEN: u64 = 24;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct PartMeta {
    pub(super) size: u64,
    pub(super) sha256: String,
}

pub(super) async fn append_footer<T: Serialize>(
    file: &mut tokio::fs::File,
    payload_len: u64,
    value: &T,
    magic: [u8; 8],
) -> Result<usize, BlobError> {
    let encoded = serde_json::to_vec(value)
        .map_err(|err| BlobError::Io(format!("encoding blob footer: {err}")))?;
    file.write_all(&encoded)
        .await
        .map_err(|err| BlobError::Io(format!("writing blob footer: {err}")))?;
    file.write_all(&(encoded.len() as u64).to_le_bytes())
        .await
        .map_err(|err| BlobError::Io(format!("writing blob metadata length: {err}")))?;
    file.write_all(&payload_len.to_le_bytes())
        .await
        .map_err(|err| BlobError::Io(format!("writing blob payload length: {err}")))?;
    file.write_all(&magic)
        .await
        .map_err(|err| BlobError::Io(format!("writing blob format marker: {err}")))?;
    file.flush()
        .await
        .map_err(|err| BlobError::Io(format!("flushing blob file: {err}")))?;
    Ok(encoded.len())
}

pub(super) async fn read_object(
    file: &mut tokio::fs::File,
    path: &Path,
) -> Result<(ObjectMeta, usize), BlobError> {
    let (meta, encoded_len, payload_len): (ObjectMeta, _, _) =
        read_footer_async(file, path, OBJECT_MAGIC).await?;
    if meta.size != payload_len {
        return Err(corrupt(path, "metadata size does not match the payload"));
    }
    Ok((meta, encoded_len))
}

pub(super) async fn read_part(
    file: &mut tokio::fs::File,
    path: &Path,
) -> Result<(PartMeta, usize), BlobError> {
    let (meta, encoded_len, payload_len): (PartMeta, _, _) =
        read_footer_async(file, path, PART_MAGIC).await?;
    if meta.size != payload_len {
        return Err(corrupt(path, "part size does not match the payload"));
    }
    Ok((meta, encoded_len))
}

async fn read_footer_async<T: DeserializeOwned>(
    file: &mut tokio::fs::File,
    path: &Path,
    expected_magic: [u8; 8],
) -> Result<(T, usize, u64), BlobError> {
    let total = file
        .metadata()
        .await
        .map_err(|err| BlobError::Io(format!("stating {}: {err}", path.display())))?
        .len();
    if total < TRAILER_LEN {
        return Err(corrupt(path, "file is shorter than its trailer"));
    }
    file.seek(SeekFrom::End(-(TRAILER_LEN as i64)))
        .await
        .map_err(|err| BlobError::Io(format!("seeking {}: {err}", path.display())))?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.read_exact(&mut trailer)
        .await
        .map_err(|err| BlobError::Io(format!("reading {} trailer: {err}", path.display())))?;
    let (metadata_len, payload_len) = decode_trailer(path, total, &trailer, expected_magic)?;
    file.seek(SeekFrom::Start(payload_len))
        .await
        .map_err(|err| BlobError::Io(format!("seeking {} metadata: {err}", path.display())))?;
    let mut encoded = vec![0; metadata_len];
    file.read_exact(&mut encoded)
        .await
        .map_err(|err| BlobError::Io(format!("reading {} metadata: {err}", path.display())))?;
    let value = serde_json::from_slice(&encoded)
        .map_err(|err| corrupt(path, &format!("invalid metadata: {err}")))?;
    Ok((value, metadata_len, payload_len))
}

pub(super) fn read_object_sync(path: &Path) -> Result<(ObjectMeta, usize), BlobError> {
    let mut file = std::fs::File::open(path)
        .map_err(|err| super::paths::read_error(path, &path.display().to_string(), err))?;
    let total = file
        .metadata()
        .map_err(|err| BlobError::Io(format!("stating {}: {err}", path.display())))?
        .len();
    if total < TRAILER_LEN {
        return Err(corrupt(path, "file is shorter than its trailer"));
    }
    file.seek(SeekFrom::End(-(TRAILER_LEN as i64)))
        .map_err(|err| BlobError::Io(format!("seeking {}: {err}", path.display())))?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.read_exact(&mut trailer)
        .map_err(|err| BlobError::Io(format!("reading {} trailer: {err}", path.display())))?;
    let (metadata_len, payload_len) = decode_trailer(path, total, &trailer, OBJECT_MAGIC)?;
    file.seek(SeekFrom::Start(payload_len))
        .map_err(|err| BlobError::Io(format!("seeking {} metadata: {err}", path.display())))?;
    let mut encoded = vec![0; metadata_len];
    file.read_exact(&mut encoded)
        .map_err(|err| BlobError::Io(format!("reading {} metadata: {err}", path.display())))?;
    let meta: ObjectMeta = serde_json::from_slice(&encoded)
        .map_err(|err| corrupt(path, &format!("invalid metadata: {err}")))?;
    if meta.size != payload_len {
        return Err(corrupt(path, "metadata size does not match the payload"));
    }
    Ok((meta, metadata_len))
}

fn decode_trailer(
    path: &Path,
    total: u64,
    trailer: &[u8; TRAILER_LEN as usize],
    expected_magic: [u8; 8],
) -> Result<(usize, u64), BlobError> {
    if trailer[16..] != expected_magic {
        return Err(corrupt(path, "unknown file-format marker"));
    }
    let metadata_len_u64 = u64::from_le_bytes(trailer[..8].try_into().unwrap_or_default());
    let payload_len = u64::from_le_bytes(trailer[8..16].try_into().unwrap_or_default());
    let expected_total = payload_len
        .checked_add(metadata_len_u64)
        .and_then(|length| length.checked_add(TRAILER_LEN))
        .ok_or_else(|| corrupt(path, "length fields overflow"))?;
    if expected_total != total {
        return Err(corrupt(path, "length fields do not match the file"));
    }
    let metadata_len = usize::try_from(metadata_len_u64)
        .map_err(|_| corrupt(path, "metadata is too large for this platform"))?;
    Ok((metadata_len, payload_len))
}

fn corrupt(path: &Path, detail: &str) -> BlobError {
    BlobError::Io(format!("corrupt blob file {}: {detail}", path.display()))
}
