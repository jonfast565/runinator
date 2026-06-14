use std::sync::Arc;

use chrono::Utc;
use interfaces::DatabaseImpl;
use log::{info, warn};
use runinator_models::errors::SendableError;
use runinator_models::settings::SettingKind;

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
    Ok(())
}

/// settings-store coordinates for the persisted, replica-shared signing secret.
const SECRET_SCOPE: &str = "auth";
const SECRET_NAME: &str = "jwt_secret";

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
