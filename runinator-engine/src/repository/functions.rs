//! packaged functions: content-addressed artifact storage plus the publish and resolve paths.
//!
//! the artifact half is deliberately thin. `BlobStore` already *is* the object store, so
//! content addressing here is just "derive the key from the digest, and don't write twice" — a
//! second `FunctionArtifactStore` trait over the same operations would only add a layer to
//! delegate through.
//!
//! the row and the bytes are written in that order for a reason: an artifact row pointing at bytes
//! that are not there yet would let a publish succeed and every invocation of it fail, whereas
//! bytes with no row are simply unreferenced storage the next upload of the same digest reuses.

use std::sync::Arc;

use runinator_blob_core::{
    BlobError, BlobStore, ByteRange, FUNCTION_ARTIFACT_BUCKET, ObjectKey, PutOptions, blob_uri,
    parse_blob_uri, sha256_hex,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::SendableError;
use runinator_models::functions::{
    ARTIFACT_MEDIA_TYPE, DEFAULT_ALIAS, FunctionAlias, FunctionArtifact, FunctionCatalogEntry,
    FunctionExport, FunctionInvocationTarget, FunctionPackage, FunctionPackageDetail,
    FunctionVersion, FunctionVersionRef, NewFunctionVersion, digest_from_hex, is_valid_digest,
};
use tokio::io::AsyncRead;
use uuid::Uuid;

use crate::errors::{
    FUNCTION_ARTIFACT_MISSING, FUNCTION_ARTIFACT_STORAGE, FUNCTION_DIGEST_MISMATCH,
    FUNCTION_INVALID_DIGEST, FUNCTION_NOT_FOUND,
};

/// an artifact's bytes, streamed rather than buffered — a package archive is the one payload here
/// that a worker fetches whole and that has no reason to sit in ws memory on the way through.
pub struct ArtifactBytes {
    pub size_bytes: u64,
    pub body: Box<dyn AsyncRead + Send + Unpin>,
}

/// the object key an artifact digest addresses.
///
/// derived rather than stored so the mapping cannot drift, and validated first: the digest arrives
/// from a request path, and an unvalidated one would be building a key out of caller input.
fn artifact_key(digest: &str) -> Result<ObjectKey, SendableError> {
    if !is_valid_digest(digest) {
        return Err(FUNCTION_INVALID_DIGEST.error(format!("'{digest}' is not a sha256 digest")));
    }
    let hex = digest.trim_start_matches("sha256:");
    // split two levels deep so one bucket listing never has to enumerate every artifact at once.
    ObjectKey::parse(&format!("sha256/{}/{}/{hex}.zip", &hex[..2], &hex[2..4]))
        .map_err(|err| FUNCTION_INVALID_DIGEST.error(err))
}

/// store artifact bytes under their digest, or do nothing when they are already stored.
///
/// the digest is verified against the bytes rather than trusted. everything downstream — pinning, a
/// worker's cache, "republishing identical bytes is free" — assumes the digest names these exact
/// bytes, and a caller that got it wrong would poison all three.
pub async fn put_artifact_if_absent<T: DatabaseImpl>(
    db: &T,
    blobs: &Arc<dyn BlobStore>,
    digest: &str,
    bytes: Vec<u8>,
) -> Result<FunctionArtifact, SendableError> {
    let key = artifact_key(digest)?;
    let actual = digest_from_hex(&sha256_hex(&bytes));
    if actual != digest {
        return Err(FUNCTION_DIGEST_MISMATCH
            .error(format!("uploaded bytes hash to {actual}, not {digest}")));
    }

    if let Some(existing) = db.fetch_function_artifact(digest).await? {
        // the row is the record of truth; a re-upload of bytes already stored is a no-op, which is
        // the property that makes republishing an unchanged package free.
        return Ok(existing);
    }

    let size_bytes = bytes.len() as i64;
    blobs
        .put(
            FUNCTION_ARTIFACT_BUCKET,
            &key,
            bytes,
            PutOptions {
                content_type: Some(ARTIFACT_MEDIA_TYPE.to_string()),
                ..PutOptions::content_addressed(digest.trim_start_matches("sha256:"))
            },
        )
        .await
        .map_err(|err| FUNCTION_ARTIFACT_STORAGE.error(err))?;

    let artifact = FunctionArtifact {
        digest: digest.to_string(),
        size_bytes,
        uri: blob_uri(FUNCTION_ARTIFACT_BUCKET, &key),
        media_type: ARTIFACT_MEDIA_TYPE.to_string(),
        created_at: chrono::Utc::now(),
    };
    db.upsert_function_artifact(&artifact).await
}

/// fetch an artifact record by digest.
pub async fn fetch_artifact<T: DatabaseImpl>(
    db: &T,
    digest: &str,
) -> Result<Option<FunctionArtifact>, SendableError> {
    db.fetch_function_artifact(digest).await
}

/// open an artifact's bytes, optionally a byte range of them.
pub async fn open_artifact<T: DatabaseImpl>(
    db: &T,
    blobs: &Arc<dyn BlobStore>,
    digest: &str,
    range: Option<ByteRange>,
) -> Result<ArtifactBytes, SendableError> {
    let Some(artifact) = db.fetch_function_artifact(digest).await? else {
        return Err(FUNCTION_ARTIFACT_MISSING.error(format!("artifact {digest} not found")));
    };
    // the uri is read back rather than re-derived, so an artifact stored under an older key layout
    // stays readable if the derivation ever changes.
    let (bucket, key) = parse_blob_uri(&artifact.uri).ok_or_else(|| {
        FUNCTION_ARTIFACT_STORAGE.error(format!("bad artifact uri {}", artifact.uri))
    })?;
    let reader = blobs
        .open(&bucket, &key, range)
        .await
        .map_err(|err: BlobError| FUNCTION_ARTIFACT_STORAGE.error(err))?;
    Ok(ArtifactBytes {
        size_bytes: reader.len(),
        body: reader.body,
    })
}

/// delete an artifact's row and its bytes. refused while a version still references it.
pub async fn delete_artifact<T: DatabaseImpl>(
    db: &T,
    blobs: &Arc<dyn BlobStore>,
    digest: &str,
) -> Result<bool, SendableError> {
    let Some(artifact) = db.fetch_function_artifact(digest).await? else {
        return Ok(false);
    };
    // the row goes first: it is what refuses the delete when a version still pins these bytes, and
    // dropping the bytes before asking would make that refusal arrive too late to help.
    if !db.delete_function_artifact(digest).await? {
        return Ok(false);
    }
    if let Some((bucket, key)) = parse_blob_uri(&artifact.uri) {
        // best effort: the row is gone, so a stranded object is unreferenced storage rather than a
        // correctness problem, and failing the call here would misreport what already happened.
        let _ = blobs.delete(&bucket, &key).await;
    }
    Ok(true)
}

/// publish one version, refusing a publish whose artifact was never uploaded.
pub async fn publish_version<T: DatabaseImpl>(
    db: &T,
    request: &NewFunctionVersion,
) -> Result<FunctionVersion, SendableError> {
    if db
        .fetch_function_artifact(&request.artifact_digest)
        .await?
        .is_none()
    {
        return Err(FUNCTION_ARTIFACT_MISSING.error(format!(
            "artifact {} must be uploaded before publishing",
            request.artifact_digest
        )));
    }
    let version = db.publish_function_version(request).await?;
    sync_provider_catalog(db, version.package_id).await?;
    // the adapter workflows are what give the http invocation path a run to start, and they are
    // generated from the same catalog the provider metadata came from, so the two cannot disagree
    // about what a package currently exports.
    super::function_adapters::sync_adapter_workflows(db, version.package_id).await?;
    Ok(version)
}

/// mirror a package's exports into the provider catalog as `functions.<pkg>` metadata.
///
/// this is what makes the catalog *durable*: workflow validation and author-time typing both read
/// provider metadata, and a worker pool scaled to zero must not make an existing workflow fail to
/// validate. providers normally self-register from a running worker; packaged functions have no
/// worker to register them, so publishing writes the row instead.
///
/// `ProviderMetadata.name` is free-form and dotted names already validate, so this needs no new
/// item type — it is an ordinary catalog row that every existing reader already understands.
pub async fn sync_provider_catalog<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
) -> Result<(), SendableError> {
    let Some(package) = db.fetch_function_package_by_id(package_id).await? else {
        return Ok(());
    };
    // every version's exports, not just the current release: a workflow pinned to version 2 must
    // still validate after version 3 ships.
    let entries: Vec<FunctionCatalogEntry> = db
        .fetch_function_catalog()
        .await?
        .into_iter()
        .filter(|entry| entry.package_id == package_id)
        .collect();
    if entries.is_empty() {
        return Ok(());
    }
    let provider_name = entries
        .first()
        .map(|entry| entry.provider_name())
        .unwrap_or_else(|| format!("functions.{}", package.name));

    // one action per export name, newest version winning — the same rule the compiler applies to
    // an unversioned call, so the catalog cannot describe a signature lowering would not pick.
    let mut newest: std::collections::BTreeMap<String, FunctionCatalogEntry> =
        std::collections::BTreeMap::new();
    for entry in entries {
        newest
            .entry(entry.export_name.clone())
            .and_modify(|current| {
                if entry.version > current.version {
                    *current = entry.clone();
                }
            })
            .or_insert(entry);
    }
    let metadata = runinator_models::providers::ProviderMetadata {
        name: provider_name.clone(),
        actions: newest
            .values()
            .map(|entry| entry.action_metadata())
            .collect(),
        metadata: Default::default(),
    };
    let item = crate::repository::provider_catalog_item(&metadata);
    crate::repository::catalog::upsert_catalog_item(db, item).await?;

    // the runtime `functions` provider too. it is normally seeded from the built-in catalog at ws
    // startup, but the adapter workflow generated below validates against it, and a publish must
    // not depend on that having happened — nor on a worker having registered anything.
    let runtime_item = crate::repository::provider_catalog_item(
        &runinator_models::functions::functions_provider_metadata(),
    );
    crate::repository::catalog::upsert_catalog_item(db, runtime_item).await?;
    Ok(())
}

/// every package, newest first.
pub async fn fetch_packages<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<FunctionPackage>, SendableError> {
    db.fetch_function_packages().await
}

/// one package with its versions, aliases, and the exports of its default alias.
pub async fn fetch_package_detail<T: DatabaseImpl>(
    db: &T,
    org_id: Option<Uuid>,
    namespace: Option<&str>,
    name: &str,
) -> Result<Option<FunctionPackageDetail>, SendableError> {
    let Some(package) = db.fetch_function_package(org_id, namespace, name).await? else {
        return Ok(None);
    };
    let versions = db.fetch_function_versions(package.id).await?;
    let aliases = db.fetch_function_aliases(package.id).await?;
    // exports of whatever the default alias points at, falling back to the newest version so a
    // package published without moving an alias still shows what it contains.
    let default_version = aliases
        .iter()
        .find(|alias| alias.name == DEFAULT_ALIAS)
        .map(|alias| alias.version_id)
        .or_else(|| versions.first().map(|version| version.id));
    let exports = match default_version {
        Some(version_id) => db.fetch_function_exports(version_id).await?,
        None => Vec::new(),
    };
    Ok(Some(FunctionPackageDetail {
        package,
        versions,
        aliases,
        exports,
    }))
}

/// the package one export belongs to, walking export -> version -> package.
///
/// here rather than in the handler because it is three chained row reads, which is orchestration by
/// the definition `AGENTS.md` uses; the handler keeps the authorization decision it makes with the
/// answer.
pub async fn fetch_export_package<T: DatabaseImpl>(
    db: &T,
    export_id: Uuid,
) -> Result<Option<FunctionPackage>, SendableError> {
    let Some(export) = db.fetch_function_export(export_id).await? else {
        return Ok(None);
    };
    let Some(version) = db.fetch_function_version(export.version_id).await? else {
        return Ok(None);
    };
    db.fetch_function_package_by_id(version.package_id).await
}

/// every package holding a version that published this artifact digest.
///
/// an artifact is content-addressed and therefore shared: two packages that published identical
/// bytes have the same digest, so this returns all of them and lets the caller decide which it may
/// see.
pub async fn packages_with_artifact<T: DatabaseImpl>(
    db: &T,
    digest: &str,
) -> Result<Vec<FunctionPackage>, SendableError> {
    let mut out = Vec::new();
    for package in db.fetch_function_packages().await? {
        let versions = db.fetch_function_versions(package.id).await?;
        if versions
            .iter()
            .any(|version| version.artifact_digest == digest)
        {
            out.push(package);
        }
    }
    Ok(out)
}

/// a package's versions, newest first.
pub async fn fetch_package_versions<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
) -> Result<Vec<FunctionVersion>, SendableError> {
    db.fetch_function_versions(package_id).await
}

/// archive a package and remove only its authoring catalog mirror.
pub async fn delete_package<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
) -> Result<bool, SendableError> {
    // read the name before the rows are gone, or the catalog row cannot be found to remove.
    let package = db.fetch_function_package_by_id(package_id).await?;
    let deleted = db.delete_function_package(package_id).await?;
    if deleted && let Some(package) = package {
        let provider_name = match &package.namespace {
            Some(namespace) => format!("functions.{namespace}.{}", package.name),
            None => format!("functions.{}", package.name),
        };
        // best effort: the package is gone either way, and a stale catalog row is a validation
        // annoyance rather than a correctness problem — while failing here would misreport a
        // delete that already happened.
        let _ = crate::repository::catalog::delete_catalog_item(
            db,
            &crate::repository::provider_catalog_uri(&provider_name),
        )
        .await;
    }
    Ok(deleted)
}

/// restore an archived package and rebuild its derived catalog mirror.
pub async fn restore_package<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
) -> Result<bool, SendableError> {
    let restored = db.restore_function_package(package_id).await?;
    if restored {
        sync_provider_catalog(db, package_id).await?;
        super::function_adapters::sync_adapter_workflows(db, package_id).await?;
    }
    Ok(restored)
}

/// point an alias at a version, named either by number or by another alias.
pub async fn set_alias<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
    alias: &str,
    target: &FunctionVersionRef,
) -> Result<FunctionAlias, SendableError> {
    let version = resolve_version(db, package_id, target).await?;
    let alias = db.set_function_alias(package_id, alias, version.id).await?;
    // an alias movement changes which entries carry which alias, which is part of what the catalog
    // reports; re-syncing keeps the two from drifting.
    sync_provider_catalog(db, package_id).await?;
    Ok(alias)
}

/// delete an alias, leaving the version it named untouched.
pub async fn delete_alias<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
    alias: &str,
) -> Result<bool, SendableError> {
    db.delete_function_alias(package_id, alias).await
}

/// resolve a version reference within a package.
pub async fn resolve_version<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
    reference: &FunctionVersionRef,
) -> Result<FunctionVersion, SendableError> {
    let version = match reference {
        FunctionVersionRef::Exact(number) => {
            db.fetch_function_version_by_number(package_id, *number)
                .await?
        }
        FunctionVersionRef::Alias(name) => match db.fetch_function_alias(package_id, name).await? {
            Some(alias) => db.fetch_function_version(alias.version_id).await?,
            None => None,
        },
    };
    version.ok_or_else(|| {
        FUNCTION_NOT_FOUND.error(match reference {
            FunctionVersionRef::Exact(number) => format!("version {number} not found"),
            FunctionVersionRef::Alias(name) => format!("alias '{name}' not found"),
        })
    })
}

/// resolve one export by package, version reference, and export name — the invocation path.
pub async fn resolve_export<T: DatabaseImpl>(
    db: &T,
    package: &FunctionPackage,
    reference: &FunctionVersionRef,
    export_name: &str,
) -> Result<(FunctionVersion, FunctionExport), SendableError> {
    let version = resolve_version(db, package.id, reference).await?;
    let export = db
        .fetch_function_exports(version.id)
        .await?
        .into_iter()
        .find(|export| export.name == export_name)
        .ok_or_else(|| {
            FUNCTION_NOT_FOUND.error(format!(
                "'{}' has no export '{export_name}' in version {}",
                package.name, version.version
            ))
        })?;
    Ok((version, export))
}

/// everything a worker needs to run one export, resolved from its id.
///
/// this is the invocation path's single read. a version is immutable, so the answer never changes
/// and a worker caches it for as long as it caches the code itself.
pub async fn resolve_invocation_target<T: DatabaseImpl>(
    db: &T,
    export_id: Uuid,
) -> Result<Option<FunctionInvocationTarget>, SendableError> {
    let Some(export) = db.fetch_function_export(export_id).await? else {
        return Ok(None);
    };
    let Some(version) = db.fetch_function_version(export.version_id).await? else {
        return Ok(None);
    };
    let Some(package) = db.fetch_function_package_by_id(version.package_id).await? else {
        return Ok(None);
    };
    Ok(Some(FunctionInvocationTarget {
        package_name: package.name,
        namespace: package.namespace,
        version: version.version,
        artifact_digest: version.artifact_digest,
        runtime: version.runtime,
        export,
    }))
}

/// how long an unreferenced artifact is kept before a sweep may remove it.
///
/// a grace period rather than an immediate delete: republishing identical bytes reuses the artifact,
/// so an artifact that is unreferenced *right now* may be referenced again minutes later by a
/// re-apply of the same pack. deleting it immediately would turn a cheap no-op upload into a real
/// one, and would race a publish that has stored its bytes but not yet written its version row.
pub const ARTIFACT_RETENTION_HOURS: i64 = 24;

/// remove artifacts no version references and that are older than the retention window.
///
/// returns the digests removed. bytes are deleted through [`delete_artifact`], which refuses any
/// artifact a version still pins — so a row that gained a reference between the scan and the delete
/// is skipped rather than orphaning a live version.
pub async fn sweep_unreferenced_artifacts<T: DatabaseImpl>(
    db: &T,
    blobs: &Arc<dyn BlobStore>,
    retention_hours: i64,
) -> Result<Vec<String>, SendableError> {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(retention_hours.max(0));
    let mut removed = Vec::new();

    for artifact in db.fetch_unreferenced_function_artifacts().await? {
        if artifact.created_at > cutoff {
            continue;
        }

        match delete_artifact(db, blobs, &artifact.digest).await {
            Ok(true) => removed.push(artifact.digest),
            // refused because something referenced it after the scan, which is the outcome the
            // guard exists for; not an error for the sweep.
            Ok(false) => {}
            Err(err) => {
                log::warn!(
                    "could not sweep function artifact {}: {err}",
                    artifact.digest
                );
            }
        }
    }
    Ok(removed)
}

/// the flattened catalog of every published export.
pub async fn fetch_catalog<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<FunctionCatalogEntry>, SendableError> {
    db.fetch_function_catalog().await
}
