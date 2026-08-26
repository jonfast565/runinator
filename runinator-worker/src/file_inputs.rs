//! Materialize VM file descriptors for a provider action.
//!
//! Providers receive a portable descriptor plus a transient `local_path`; they never need object
//! store credentials and cannot use a descriptor to fetch a different object's bytes.

use std::path::PathBuf;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::{
    errors::SendableError,
    files::{FileDescriptor, validate_relative_path, with_local_path},
    value::{Map, Value},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub async fn materialize(
    client: &AsyncApiClient<StaticLocator>,
    effect_id: Uuid,
    value: Value,
) -> Result<Value, SendableError> {
    let root = std::env::temp_dir()
        .join("runinator-worker-files")
        .join(effect_id.to_string());
    materialize_value(client, &root, value).await
}

async fn materialize_value(
    client: &AsyncApiClient<StaticLocator>,
    root: &PathBuf,
    value: Value,
) -> Result<Value, SendableError> {
    if let Ok(descriptor) = FileDescriptor::from_value(&value) {
        return materialize_file(client, root, &descriptor).await;
    }
    match value {
        Value::Array(values) => {
            let mut materialized = Vec::with_capacity(values.len());
            for value in values {
                materialized.push(Box::pin(materialize_value(client, root, value)).await?);
            }
            Ok(Value::Array(materialized))
        }
        Value::Object(values) => {
            let mut materialized = Map::new();
            for (key, value) in values {
                materialized.insert(key, Box::pin(materialize_value(client, root, value)).await?);
            }
            Ok(Value::Object(materialized))
        }
        value => Ok(value),
    }
}

async fn materialize_file(
    client: &AsyncApiClient<StaticLocator>,
    root: &PathBuf,
    descriptor: &FileDescriptor,
) -> Result<Value, SendableError> {
    validate_relative_path(&descriptor.path).map_err(|message| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            message,
        )) as SendableError
    })?;
    let bytes = client.download_workflow_file(descriptor.id).await?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(&descriptor.sha256) {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "workflow file {} did not match its declared sha256",
                descriptor.path
            ),
        )));
    }
    let target = root.join(&descriptor.path);
    let parent = target.parent().ok_or_else(|| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workflow file path has no parent",
        )) as SendableError
    })?;
    tokio::fs::create_dir_all(parent).await?;
    tokio::fs::write(&target, bytes).await?;
    Ok(with_local_path(
        descriptor,
        target.to_string_lossy().into_owned(),
    ))
}
