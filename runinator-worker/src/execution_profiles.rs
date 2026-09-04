//! Just-in-time staging for centrally managed execution profiles.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::{
    errors::SendableError,
    execution_profiles::{
        ExecutionProfileBinding, ExecutionProfileHealth, MaterializedExecutionProfile,
    },
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_ARCHIVE_BYTES: usize = 10 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_000;

pub struct ProfileLease {
    pub context: MaterializedExecutionProfile,
    pub credential_scopes: Vec<String>,
    root: PathBuf,
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.root.display(), %error, "failed to clean execution profile directory");
        }
    }
}

pub async fn materialize(
    client: &AsyncApiClient<StaticLocator>,
    effect_id: uuid::Uuid,
    workflow_run_id: uuid::Uuid,
    binding: &ExecutionProfileBinding,
) -> Result<ProfileLease, SendableError> {
    let profile = if binding.id().is_nil() {
        client
            .resolve_execution_profile_for_run(binding.name(), workflow_run_id)
            .await?
    } else {
        client
            .fetch_execution_profile_for_run(binding.id(), workflow_run_id)
            .await?
    };
    if !profile.enabled {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "execution profile is disabled",
        )));
    }
    if matches!(
        profile.health,
        ExecutionProfileHealth::Unpublished
            | ExecutionProfileHealth::Testing
            | ExecutionProfileHealth::Error
            | ExecutionProfileHealth::Expired
            | ExecutionProfileHealth::Disabled
    ) {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "execution profile is not usable ({})",
                profile.health.as_str()
            ),
        )));
    }
    if profile
        .expires_at
        .is_some_and(|value| value <= chrono::Utc::now())
    {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "execution profile has expired",
        )));
    }
    let revision = profile.current_revision.ok_or_else(|| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "execution profile has not been published",
        )) as SendableError
    })?;
    let expected = profile.current_digest.as_deref().ok_or_else(|| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "execution profile has no current digest",
        )) as SendableError
    })?;
    let bytes = client
        .download_execution_profile_for_run(profile.id, revision, workflow_run_id)
        .await?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "execution profile archive exceeds 10 MiB",
        )));
    }
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "execution profile digest mismatch",
        )));
    }
    let root = std::env::temp_dir()
        .join("runinator-worker-profiles")
        .join(effect_id.to_string());
    let _ = fs::remove_dir_all(&root);
    let home = profile.exposure.home_overlay.then(|| root.join("home"));
    let content_root = home.as_deref().unwrap_or(&root).to_path_buf();
    if let Err(error) = (|| -> Result<(), SendableError> {
        fs::create_dir_all(&content_root)?;
        set_dir_permissions(&root)?;
        set_dir_permissions(&content_root)?;
        unpack(&bytes, &content_root, profile.id, &profile.config_digest)
    })() {
        let _ = fs::remove_dir_all(&root);
        return Err(error);
    }
    let root_text = root.to_string_lossy().into_owned();
    let home_text = home
        .as_ref()
        .map(|value| value.to_string_lossy().into_owned());
    let environment = profile
        .exposure
        .environment
        .into_iter()
        .map(|(key, value)| {
            let value = value.replace("${PROFILE_ROOT}", &root_text).replace(
                "${PROFILE_HOME}",
                home_text.as_deref().unwrap_or(&root_text),
            );
            (key, value)
        })
        .collect::<BTreeMap<_, _>>();
    Ok(ProfileLease {
        context: MaterializedExecutionProfile {
            profile_id: profile.id,
            revision,
            root: root_text,
            home: home_text,
            environment,
        },
        credential_scopes: profile.credential_scopes,
        root,
    })
}

#[derive(Deserialize)]
struct BundleManifest {
    version: u32,
    profile_id: uuid::Uuid,
    config_digest: String,
    files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    path: String,
    sha256: String,
    size: usize,
}

fn unpack(
    bytes: &[u8],
    target: &Path,
    expected_profile_id: uuid::Uuid,
    expected_config_digest: &str,
) -> Result<(), SendableError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > MAX_ENTRIES {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "execution profile archive has too many entries",
        )));
    }
    let mut seen = HashSet::new();
    let mut extracted = BTreeMap::<String, (String, usize)>::new();
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "execution profile archive contains an unsafe path",
                )
            })?
            .to_path_buf();
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
            || !seen.insert(path.clone())
        {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "execution profile archive contains an invalid or duplicate path",
            )));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "execution profile archive contains a symbolic link",
            )));
        }
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_EXPANDED_BYTES {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "execution profile archive expands beyond 32 MiB",
            )));
        }
        let output = target.join(&path);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            set_dir_permissions(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
            set_dir_permissions(parent)?;
        }
        let mut file = fs::File::create(&output)?;
        std::io::copy(&mut entry.by_ref().take(MAX_EXPANDED_BYTES + 1), &mut file)?;
        set_file_permissions(&output)?;
        if path != Path::new(".runinator-profile.json") {
            let contents = fs::read(&output)?;
            extracted.insert(
                path.to_string_lossy().replace('\\', "/"),
                (format!("{:x}", Sha256::digest(&contents)), contents.len()),
            );
        }
    }
    let manifest_path = target.join(".runinator-profile.json");
    let manifest: BundleManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    if manifest.version != 1
        || manifest.profile_id != expected_profile_id
        || manifest.config_digest != expected_config_digest
        || manifest.files.len() != extracted.len()
        || manifest
            .files
            .iter()
            .any(|file| extracted.get(&file.path) != Some(&(file.sha256.clone(), file.size)))
    {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "execution profile manifest verification failed",
        )));
    }
    fs::remove_file(manifest_path)?;
    Ok(())
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
#[cfg(unix)]
fn set_file_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

pub fn cleanup_abandoned() {
    let root = std::env::temp_dir().join("runinator-worker-profiles");
    if let Err(error) = fs::remove_dir_all(&root)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path = %root.display(), %error, "failed to clean abandoned execution profiles");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(profile_id: uuid::Uuid, file: &[u8], declared: &[u8]) -> Vec<u8> {
        let manifest = serde_json::json!({
            "version": 1,
            "profile_id": profile_id,
            "config_digest": "config",
            "files": [{
                "path": ".tool/session.json",
                "sha256": format!("{:x}", Sha256::digest(declared)),
                "size": declared.len(),
            }],
        });
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file(".runinator-profile.json", options)
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer.start_file(".tool/session.json", options).unwrap();
        writer.write_all(file).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn manifest_is_verified_and_removed_after_unpacking() {
        let id = uuid::Uuid::new_v4();
        let root =
            std::env::temp_dir().join(format!("runinator-unpack-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        unpack(&archive(id, b"token", b"token"), &root, id, "config").unwrap();
        assert_eq!(fs::read(root.join(".tool/session.json")).unwrap(), b"token");
        assert!(!root.join(".runinator-profile.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_tampering_is_rejected() {
        let id = uuid::Uuid::new_v4();
        let root =
            std::env::temp_dir().join(format!("runinator-unpack-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        assert!(unpack(&archive(id, b"tampered", b"token"), &root, id, "config").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
