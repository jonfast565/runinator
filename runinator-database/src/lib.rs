use std::sync::Arc;

use chrono::Utc;
use interfaces::DatabaseImpl;
use log::{info, warn};
use runinator_models::auth::{ApiKey, ApiKeyRecord};
use runinator_models::errors::SendableError;
use runinator_models::settings::SettingKind;
use uuid::Uuid;

pub mod backend;
mod common;
pub mod errors;
pub mod interfaces;
mod mappers;
pub mod mysql;
mod operations;
pub mod postgres;
mod queries;
pub mod sqlite;

#[derive(Debug, Clone, Default)]
pub struct BootstrapOptions {
    pub auth_jwt_secret: Option<String>,
    pub auth_bootstrap_admin: Option<String>,
    pub auth_bootstrap_service_api_key: Option<String>,
    pub auth_bootstrap_service_api_key_name: Option<String>,
}

pub async fn bootstrap_database(
    pool: &Arc<impl DatabaseImpl>,
    options: &BootstrapOptions,
) -> Result<(), SendableError> {
    info!("Run bootstrap scripts");
    let scripts: Vec<String> = Vec::new();
    pool.run_init_scripts(&scripts).await?;
    ensure_jwt_secret(pool.as_ref(), options.auth_jwt_secret.clone()).await?;
    if let Some(spec) = options.auth_bootstrap_admin.as_deref() {
        seed_bootstrap_admin(pool.as_ref(), spec).await?;
    }
    if let Some(raw_key) = options.auth_bootstrap_service_api_key.as_deref() {
        seed_bootstrap_service_api_key(
            pool.as_ref(),
            options
                .auth_bootstrap_service_api_key_name
                .as_deref()
                .unwrap_or(DEFAULT_BOOTSTRAP_SERVICE_API_KEY_NAME),
            raw_key,
        )
        .await?;
    }
    Ok(())
}

/// settings-store coordinates for the persisted, replica-shared signing secret.
const SECRET_SCOPE: &str = "auth";
const SECRET_NAME: &str = "jwt_secret";
const DEFAULT_BOOTSTRAP_SERVICE_API_KEY_NAME: &str = "bootstrap-service";

pub async fn ensure_jwt_secret<T: DatabaseImpl>(
    db: &T,
    explicit: Option<String>,
) -> Result<Vec<u8>, SendableError> {
    if let Some(secret) = explicit.filter(|s| !s.is_empty()) {
        let bytes = secret.into_bytes();
        db.upsert_setting(
            SettingKind::Secret,
            SECRET_SCOPE.into(),
            SECRET_NAME.into(),
            bytes.clone(),
            Utc::now().timestamp(),
        )
        .await?;
        return Ok(bytes);
    }
    if let Some(record) = db
        .fetch_setting(SettingKind::Secret, SECRET_SCOPE.into(), SECRET_NAME.into())
        .await?
    {
        if !record.value.is_empty() {
            return Ok(record.value);
        }
    }
    let generated = runinator_auth::random_secret(48);
    db.upsert_setting(
        SettingKind::Secret,
        SECRET_SCOPE.into(),
        SECRET_NAME.into(),
        generated.clone(),
        Utc::now().timestamp(),
    )
    .await?;
    Ok(generated)
}

pub async fn load_jwt_secret<T: DatabaseImpl>(db: &T) -> Result<Vec<u8>, SendableError> {
    let secret = db
        .fetch_setting(SettingKind::Secret, SECRET_SCOPE.into(), SECRET_NAME.into())
        .await?
        .filter(|record| !record.value.is_empty())
        .map(|record| record.value);
    secret.ok_or_else(|| {
        Box::new(std::io::Error::other(
            "missing auth jwt secret; run runinator-bootstrap before starting runinator-ws",
        )) as SendableError
    })
}

pub async fn seed_bootstrap_admin<T: DatabaseImpl>(
    db: &T,
    spec: &str,
) -> Result<(), SendableError> {
    if db.count_users().await? > 0 {
        return Ok(());
    }
    let Some((username, password)) = spec.split_once(':') else {
        warn!("RUNINATOR_AUTH_BOOTSTRAP_ADMIN must be 'username:password'; skipping seed");
        return Ok(());
    };
    let hash = runinator_auth::hash_password(password)
        .map_err(|err| -> SendableError { Box::new(std::io::Error::other(err)) })?;
    db.create_user(username.to_string(), None, true, Some(hash))
        .await?;
    info!("Seeded bootstrap admin user '{username}'");
    Ok(())
}

pub async fn seed_bootstrap_service_api_key<T: DatabaseImpl>(
    db: &T,
    name: &str,
    raw_key: &str,
) -> Result<(), SendableError> {
    let Some((prefix, _)) = raw_key.split_once('.') else {
        warn!(
            "RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY must be '<prefix>.<secret>'; skipping seed"
        );
        return Ok(());
    };
    if prefix.is_empty() {
        warn!(
            "RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY must include a non-empty prefix; skipping seed"
        );
        return Ok(());
    }
    if db
        .fetch_api_key_by_prefix(prefix.to_string())
        .await?
        .is_some()
    {
        return Ok(());
    }

    let record = ApiKeyRecord {
        key: ApiKey {
            id: Some(Uuid::now_v7()),
            name: name.to_string(),
            user_id: None,
            is_service: true,
            key_prefix: prefix.to_string(),
            last_used_at: None,
            expires_at: None,
            disabled: false,
            created_at: Utc::now(),
        },
        is_admin: true,
        key_hash: runinator_auth::hash_secret(raw_key),
    };
    db.create_api_key(record).await?;
    info!("Seeded bootstrap service api key '{name}'");
    Ok(())
}
